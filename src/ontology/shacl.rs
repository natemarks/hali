//! SHACL validation through rudof's `shacl` crate (spec §2.7). This is the only code that talks
//! to rudof.
//!
//! hali never hands Turtle text to rudof's parsers: the triples are copied from the store. The
//! shapes graph is the core graph plus the extensions graph; the data graph is every graph.

use crate::storage::{CORE_GRAPH, EXTENSIONS_GRAPH, Store};
use oxigraph::model::{NamedNodeRef, NamedOrBlankNodeRef, Quad, QuadRef, Term};
use rudof_rdf::rdf_core::term::Object;
use rudof_rdf::rdf_impl::OxigraphInMemory;
use serde::Serialize;
use shacl::ir::IRSchema;
use shacl::rdf::ShaclParser;
use shacl::types::Severity;
use shacl::validator::processor::{GraphValidation, ShaclProcessor};
use shacl::validator::report::ValidationResult;
use shacl::validator::store::Graph;
use shacl::validator::{ShaclConfig, ShaclValidationMode};

const SH: &str = "http://www.w3.org/ns/shacl#";

/// One SHACL validation result, as `--json` prints it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Violation {
    /// The focus node, in N-Triples form.
    pub focus_node: String,
    /// The source shape's IRI; `None` for a blank-node shape.
    pub shape: Option<String>,
    /// The predicate IRI, a SPARQL-like path for complex paths, `None` for node-level results.
    pub path: Option<String>,
    /// The constraint component IRI.
    pub constraint: String,
    /// The `sh:` local name of the severity, e.g. `Violation`.
    pub severity: String,
    /// The value node, in N-Triples form.
    pub value: Option<String>,
    pub message: String,
}

/// The shapes cannot be used: they hold SHACL-SPARQL from `extensions.ttl`, or do not compile.
#[derive(Debug)]
pub struct ShapesInvalid;

/// Validates the store, plus `extra` quads not (yet) in it, against the shapes in the store.
/// The results are unordered.
pub fn validate(store: &Store, extra: &[Quad]) -> Result<Vec<Violation>, ShapesInvalid> {
    let schema = shapes(store)?;

    let mut data = OxigraphInMemory::new();
    for quad in store.iter() {
        add(
            &mut data,
            quad.expect("reading the in-memory store").as_ref(),
        );
    }
    for quad in extra {
        add(&mut data, quad.as_ref());
    }
    let graph = Graph::try_from(data).expect("building the in-memory SHACL data graph");
    let report = GraphValidation::new(graph)
        .validate(
            &schema,
            &ShaclValidationMode::Native,
            &ShaclConfig::default(),
        )
        .map_err(|_| ShapesInvalid)?;
    Ok(report
        .results()
        .iter()
        .map(|r| violation(store, r))
        .collect())
}

/// Parses and compiles the shapes graph. SHACL-SPARQL from `extensions.ttl` is refused: it could
/// reach the network (SERVICE) and parse SPARQL XML results.
fn shapes(store: &Store) -> Result<IRSchema, ShapesInvalid> {
    let extensions = NamedNodeRef::new_unchecked(EXTENSIONS_GRAPH);
    for predicate in ["select", "ask"] {
        let predicate = format!("{SH}{predicate}");
        let found = store
            .quads_for_pattern(
                None,
                Some(NamedNodeRef::new_unchecked(&predicate)),
                None,
                Some(extensions.into()),
            )
            .next()
            .is_some();
        if found {
            return Err(ShapesInvalid);
        }
    }

    let mut rdf = OxigraphInMemory::new();
    for graph in [CORE_GRAPH, EXTENSIONS_GRAPH] {
        let graph = NamedNodeRef::new_unchecked(graph);
        for quad in store.quads_for_pattern(None, None, None, Some(graph.into())) {
            add(
                &mut rdf,
                quad.expect("reading the in-memory store").as_ref(),
            );
        }
    }
    let ast = ShaclParser::new(rdf).parse().map_err(|_| ShapesInvalid)?;
    IRSchema::compile(&ast).map_err(|_| ShapesInvalid)
}

fn add(rdf: &mut OxigraphInMemory, quad: QuadRef<'_>) {
    rdf.add_triple_ref(quad.subject, quad.predicate, quad.object)
        .expect("adding to an in-memory graph cannot fail");
}

fn violation(store: &Store, result: &ValidationResult) -> Violation {
    let shape = match result.source() {
        Some(Object::Iri(iri)) => Some(iri.to_string()),
        _ => None,
    };
    let constraint = result.constraint_component().to_string();
    let message = english(result)
        .or_else(|| shape.as_deref().and_then(|s| shape_message(store, s)))
        .unwrap_or_else(|| {
            let local = constraint.rsplit(['#', '/']).next().unwrap_or(&constraint);
            let local = local.strip_suffix("ConstraintComponent").unwrap_or(local);
            format!("{local} constraint not satisfied")
        });
    Violation {
        focus_node: nt(result.focus_node()),
        shape,
        path: result.path().map(ToString::to_string),
        constraint,
        severity: match result.severity() {
            Severity::Generic(iri) => {
                let iri = iri.to_string();
                iri.rsplit(['#', '/']).next().unwrap_or(&iri).to_string()
            }
            known => known.to_string(),
        },
        value: result.value().map(nt),
        message,
    }
}

/// The result's `@en` message. rudof's own engine text is untagged, so it is never picked.
fn english(result: &ValidationResult) -> Option<String> {
    result
        .message()
        .iter()
        .find(|(lang, _)| {
            lang.as_ref()
                .is_some_and(|l| l.as_str().eq_ignore_ascii_case("en"))
        })
        .map(|(_, message)| message.clone())
}

/// A shape's `sh:message` in the shapes graph: `@en`, else untagged.
fn shape_message(store: &Store, shape: &str) -> Option<String> {
    let predicate = format!("{SH}message");
    let mut untagged = None;
    for graph in [CORE_GRAPH, EXTENSIONS_GRAPH] {
        let quads = store.quads_for_pattern(
            Some(NamedOrBlankNodeRef::from(NamedNodeRef::new_unchecked(
                shape,
            ))),
            Some(NamedNodeRef::new_unchecked(&predicate)),
            None,
            Some(NamedNodeRef::new_unchecked(graph).into()),
        );
        for quad in quads {
            if let Term::Literal(l) = quad.expect("reading the in-memory store").object {
                match l.language() {
                    Some(lang) if lang.eq_ignore_ascii_case("en") => {
                        return Some(l.value().to_string());
                    }
                    None => untagged = untagged.or_else(|| Some(l.value().to_string())),
                    Some(_) => {}
                }
            }
        }
    }
    untagged
}

/// A term in N-Triples form.
fn nt(object: &Object) -> String {
    oxigraph::model::Term::from(object.clone()).to_string()
}
