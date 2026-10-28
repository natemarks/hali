//! Goal tests for SHACL validation in `core` (#7).
//!
//! Assumed API:
//!   `hali::ontology::SHAPES_TTL: &str`
//!   `hali::storage::load(&Path) -> Result<Store, hali::storage::Error>` (loads SHAPES_TTL into
//!   the core graph `https://w3id.org/hali/graph/core`)
//!   `hali::core::validate::validate(&Store, offset: usize, limit: usize)
//!        -> Result<Report, hali::core::Error>`
//!   `Report { conforms: bool, results: Vec<Violation>, total: usize, truncated: bool,
//!             next_offset: Option<usize> }`
//!   `Violation { focus_node: String, shape: Option<String>, path: Option<String>,
//!                constraint: String, severity: String, value: Option<String>, message: String }`
//!   `hali::core::concept::add(..)` can return an Error with `rule == "SHACL_VIOLATION"` or
//!   `"SHAPES_INVALID"`; `Error::exit_code()` is 1 and 3.
//!   Fixtures sit in tests/fixtures/shacl/<name>/.

use hali::core::validate::{Report, Violation, validate};
use hali::core::{Context, FixedClock, SeededIdGen, concept};
use hali::storage::Store;
use oxigraph::model::{GraphNameRef, Literal, NamedNode, NamedNodeRef, QuadRef};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const HALI: &str = "https://w3id.org/hali#";
const SH: &str = "http://www.w3.org/ns/shacl#";
const SKOS: &str = "http://www.w3.org/2004/02/skos/core#";
const CORE_GRAPH: &str = "https://w3id.org/hali/graph/core";
const VOCABULARY_GRAPH: &str = "https://w3id.org/hali/graph/vocabulary";

fn ctx() -> Context {
    Context::new(
        "en",
        Box::new(FixedClock::new("2026-10-24T09:30:00Z")),
        Box::new(SeededIdGen::new(1)),
    )
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn load(name: &str) -> Store {
    hali::storage::load(&fixture(name)).unwrap()
}

fn report(name: &str) -> Report {
    match validate(&load(name), 0, 1000) {
        Ok(report) => report,
        Err(e) => panic!("{name}: {} {}", e.rule, e.message),
    }
}

/// Every field of a violation, in output order, null as "".
fn key(v: &Violation) -> Vec<String> {
    vec![
        v.focus_node.clone(),
        v.shape.clone().unwrap_or_default(),
        v.path.clone().unwrap_or_default(),
        v.constraint.clone(),
        v.severity.clone(),
        v.value.clone().unwrap_or_default(),
        v.message.clone(),
    ]
}

/// The sort key the contract names: (focus_node, shape, path, constraint, value, message).
fn sort_key(v: &Violation) -> Vec<String> {
    let k = key(v);
    vec![
        k[0].clone(),
        k[1].clone(),
        k[2].clone(),
        k[3].clone(),
        k[5].clone(),
        k[6].clone(),
    ]
}

fn nt(local: &str) -> String {
    format!("<{HALI}{local}>")
}

/// Asserts the report holds exactly one violation of `shape` / `constraint` and returns it.
fn only(report: &Report, shape: &str, constraint: &str) -> Violation {
    assert!(!report.conforms);
    assert_eq!(
        report.total,
        1,
        "{:?}",
        report.results.iter().map(key).collect::<Vec<_>>()
    );
    assert_eq!(report.results.len(), 1);
    let v = report.results[0].clone();
    assert_eq!(v.shape.as_deref(), Some(shape));
    assert_eq!(v.constraint, format!("{SH}{constraint}ConstraintComponent"));
    assert_eq!(v.severity, "Violation");
    v
}

/// Inserts a conforming-looking Concept straight into the store, bypassing `concept::add`.
fn insert_concept(store: &Store, id: &str, label: &str) {
    let graph = NamedNode::new_unchecked(VOCABULARY_GRAPH);
    let subject = NamedNode::new_unchecked(format!("{HALI}{id}"));
    let rdf_type = NamedNodeRef::new_unchecked("http://www.w3.org/1999/02/22-rdf-syntax-ns#type");
    let concept = NamedNode::new_unchecked(format!("{SKOS}Concept"));
    let in_scheme = NamedNodeRef::new_unchecked("http://www.w3.org/2004/02/skos/core#inScheme");
    let topics = NamedNode::new_unchecked(format!("{HALI}topics"));
    let pref = NamedNodeRef::new_unchecked("http://www.w3.org/2004/02/skos/core#prefLabel");
    let lit = Literal::new_language_tagged_literal(label, "en").unwrap();
    store
        .insert(QuadRef::new(&subject, rdf_type, &concept, &graph))
        .unwrap();
    store
        .insert(QuadRef::new(&subject, in_scheme, &topics, &graph))
        .unwrap();
    store
        .insert(QuadRef::new(&subject, pref, &lit, &graph))
        .unwrap();
}

/// Every quad of the store, as a sorted set of strings.
fn snapshot(store: &Store) -> BTreeSet<String> {
    store.iter().map(|q| q.unwrap().to_string()).collect()
}

// ---- criterion 1: core shapes embedded, named, in the core graph -----------------------

fn shape_triples() -> Vec<oxigraph::model::Triple> {
    oxttl::TurtleParser::new()
        .for_slice(hali::ontology::SHAPES_TTL.as_bytes())
        .map(|t| t.expect("SHAPES_TTL is valid Turtle"))
        .collect()
}

#[test]
fn shapes_are_embedded_and_loaded_into_the_core_graph() {
    let triples = shape_triples();
    assert!(!triples.is_empty());
    let dir = tempfile::tempdir().unwrap();
    let store = hali::storage::load(dir.path()).unwrap();
    let core = NamedNode::new_unchecked(CORE_GRAPH);
    for t in &triples {
        assert!(
            store
                .contains(t.as_ref().in_graph(GraphNameRef::NamedNode(core.as_ref())))
                .unwrap(),
            "{t} is not in the core graph"
        );
    }
    let node_shape = format!("{SH}NodeShape");
    for shape in ["ConceptShape", "LabelUniqueShape"] {
        let subject = NamedNode::new_unchecked(format!("{HALI}{shape}"));
        assert!(
            triples.iter().any(|t| t.subject == subject.clone().into()
                && t.predicate.as_str() == "http://www.w3.org/1999/02/22-rdf-syntax-ns#type"
                && t.object == NamedNode::new_unchecked(node_shape.clone()).into()),
            "{shape} must be a sh:NodeShape"
        );
    }
}

#[test]
fn core_shapes_have_no_blank_nodes() {
    for t in shape_triples() {
        assert!(
            !matches!(t.subject, oxigraph::model::NamedOrBlankNode::BlankNode(_)),
            "blank subject in {t}"
        );
        assert!(
            !matches!(t.object, oxigraph::model::Term::BlankNode(_)),
            "blank object in {t}"
        );
    }
}

// ---- criterion 2: extension shapes ----------------------------------------------------

#[test]
fn extension_shape_conforming_store_conforms() {
    let r = report("shacl/extension-shape");
    assert!(
        r.conforms,
        "{:?}",
        r.results.iter().map(key).collect::<Vec<_>>()
    );
    assert_eq!(r.total, 0);
    assert!(r.results.is_empty());
    assert!(!r.truncated);
    assert_eq!(r.next_offset, None);
}

#[test]
fn extension_shapes_are_applied() {
    let store = load("shacl/extension-shape");
    insert_concept(&store, "noalias", "no alias");
    let r = validate(&store, 0, 50).unwrap();
    let v = only(&r, "http://example.org/shapes#AltLabelRequired", "MinCount");
    assert_eq!(v.focus_node, nt("noalias"));
    assert_eq!(v.path.as_deref(), Some(format!("{SKOS}altLabel").as_str()));
    assert_eq!(v.value, None);
    assert_eq!(v.message, "Every Concept needs an alias");
}

// ---- criterion 3: SHAPES_INVALID ------------------------------------------------------

fn assert_shapes_invalid(name: &str) {
    let store = load(name);
    let before = snapshot(&store);
    let err = validate(&store, 0, 50).expect_err("validate must fail");
    assert_eq!(err.rule, "SHAPES_INVALID", "{name}");
    assert_eq!(err.exit_code(), 3);
    assert_eq!(
        err.message,
        "extensions.ttl has SHACL shapes that hali cannot use"
    );
    assert_eq!(*err.details, serde_json::json!({"file": "extensions.ttl"}));
    let text = serde_json::to_string(&err).unwrap();
    assert!(!text.contains("SECRET-CANARY-4242"), "{text}");

    let mut store = store;
    let add = concept::add(&ctx(), &mut store, "brand new", &[]).expect_err("add must fail");
    assert_eq!(add.rule, "SHAPES_INVALID", "{name}");
    assert_eq!(add.exit_code(), 3);
    assert_eq!(
        snapshot(&store),
        before,
        "a refused add leaves the store unchanged"
    );
}

#[test]
fn uncompilable_extension_shapes_are_shapes_invalid() {
    assert_shapes_invalid("shacl/extension-invalid");
}

#[test]
fn sparql_in_extension_shapes_is_shapes_invalid() {
    assert_shapes_invalid("shacl/extension-sparql");
}

// ---- criterion 4: every Concept in hali:topics -----------------------------------------

#[test]
fn concept_not_in_topics_violates_in_scheme_shape() {
    let r = report("shacl/not-in-scheme");
    let v = only(&r, &format!("{HALI}ConceptInSchemeShape"), "HasValue");
    assert_eq!(v.focus_node, nt("orphan"));
    assert_eq!(v.path.as_deref(), Some(format!("{SKOS}inScheme").as_str()));
    assert_eq!(v.value, None);
    assert_eq!(v.message, "A Concept must be in the hali:topics scheme");
}

#[test]
fn concept_in_other_scheme_violates_in_scheme_shape() {
    let r = report("shacl/other-scheme");
    let v = only(&r, &format!("{HALI}ConceptInSchemeShape"), "HasValue");
    assert_eq!(v.focus_node, nt("stray"));
    assert_eq!(v.message, "A Concept must be in the hali:topics scheme");
}

// ---- criterion 5: exactly one prefLabel per language ------------------------------------

#[test]
fn concept_without_pref_label_violates_pref_label_shape() {
    let r = report("shacl/no-pref-label");
    let v = only(&r, &format!("{HALI}ConceptPrefLabelShape"), "MinCount");
    assert_eq!(v.focus_node, nt("nolabel"));
    assert_eq!(v.path.as_deref(), Some(format!("{SKOS}prefLabel").as_str()));
    assert_eq!(
        v.message,
        "A Concept needs exactly one skos:prefLabel per language"
    );
}

#[test]
fn two_pref_labels_in_one_language_violate_pref_label_shape() {
    let r = report("shacl/two-pref-labels");
    let v = only(&r, &format!("{HALI}ConceptPrefLabelShape"), "UniqueLang");
    assert_eq!(v.focus_node, nt("twolabels"));
    assert_eq!(v.path.as_deref(), Some(format!("{SKOS}prefLabel").as_str()));
    assert_eq!(
        v.message,
        "A Concept needs exactly one skos:prefLabel per language"
    );
}

#[test]
fn pref_labels_in_two_languages_conform() {
    let r = report("shacl/pref-labels-two-languages");
    assert!(
        r.conforms,
        "{:?}",
        r.results.iter().map(key).collect::<Vec<_>>()
    );
    assert_eq!(r.total, 0);
}

// ---- criterion 6: labels unique across Concepts ----------------------------------------

#[test]
fn duplicate_labels_violate_label_unique_shape() {
    let r = report("shacl/duplicate-label");
    // One result per (Concept, label) pair in a clash: ml, ml2 (case and space folded), and
    // nn's prefLabel against dl's altLabel.
    assert_eq!(
        r.total,
        4,
        "{:?}",
        r.results.iter().map(key).collect::<Vec<_>>()
    );
    let mut got = BTreeSet::new();
    for v in &r.results {
        assert_eq!(
            v.shape.as_deref(),
            Some(format!("{HALI}LabelUniqueShape").as_str())
        );
        assert_eq!(v.focus_node, nt("topics"));
        assert_eq!(v.path, None);
        assert_eq!(v.constraint, format!("{SH}SPARQLConstraintComponent"));
        assert_eq!(v.severity, "Violation");
        got.insert((v.value.clone().unwrap(), v.message.clone()));
    }
    let want: BTreeSet<(String, String)> = [
        ("ml", "machine learning"),
        ("ml2", " Machine  Learning"),
        ("nn", "neural networks"),
        ("dl", "Neural Networks"),
    ]
    .into_iter()
    .map(|(id, label)| {
        (
            nt(id),
            format!("label \"{label}\" is used by more than one Concept"),
        )
    })
    .collect();
    assert_eq!(got, want);
}

#[test]
fn same_label_in_other_language_conforms() {
    let r = report("shacl/same-label-other-language");
    assert!(
        r.conforms,
        "{:?}",
        r.results.iter().map(key).collect::<Vec<_>>()
    );
    assert_eq!(r.total, 0);
}

// ---- criterion 7: every shape has a conforming and a violating fixture -----------------

#[test]
fn conforming_fixtures_have_no_violations() {
    for name in [
        "store",
        "first-add",
        "three-adds",
        "noncanonical",
        "shacl/pref-labels-two-languages",
        "shacl/same-label-other-language",
        "shacl/extension-shape",
    ] {
        let r = report(name);
        assert!(
            r.conforms,
            "{name}: {:?}",
            r.results.iter().map(key).collect::<Vec<_>>()
        );
        assert_eq!(r.total, 0, "{name}");
    }
}

/// The shapes that can appear in a result, with a fixture that violates each.
const SHAPE_FIXTURES: [(&str, &str); 9] = [
    ("ConceptInSchemeShape", "shacl/not-in-scheme"),
    ("ConceptPrefLabelShape", "shacl/no-pref-label"),
    ("LabelUniqueShape", "shacl/duplicate-label"),
    ("ResourceByteSizeShape", "shacl/resource-negative-byte-size"),
    ("ResourceChecksumShape", "shacl/resource-two-checksums"),
    (
        "ResourceDownloadURLShape",
        "shacl/resource-two-download-urls",
    ),
    ("ResourceMediaTypeShape", "shacl/resource-no-media-type"),
    ("ResourceSubjectShape", "shacl/resource-subject-not-concept"),
    ("ResourceTitleShape", "shacl/resource-no-title"),
];

#[test]
fn every_core_shape_has_a_violating_fixture() {
    use oxigraph::model::{NamedOrBlankNode, Term};
    let triples = shape_triples();
    let sh_type = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
    let mut shapes: BTreeSet<String> = BTreeSet::new();
    let mut containers: BTreeSet<String> = BTreeSet::new();
    for t in &triples {
        let NamedOrBlankNode::NamedNode(s) = &t.subject else {
            continue;
        };
        let p = t.predicate.as_str();
        let is_shape_type = p == sh_type
            && matches!(&t.object, Term::NamedNode(o)
                if o.as_str() == format!("{SH}NodeShape") || o.as_str() == format!("{SH}PropertyShape"));
        if is_shape_type || p == format!("{SH}path") {
            shapes.insert(s.as_str().to_string());
        }
        if p == format!("{SH}property") {
            containers.insert(s.as_str().to_string());
        }
    }
    // A node shape that only groups property shapes never appears in a result.
    let shapes: BTreeSet<String> = shapes.difference(&containers).cloned().collect();
    let table: BTreeSet<String> = SHAPE_FIXTURES
        .iter()
        .map(|(shape, _)| format!("{HALI}{shape}"))
        .collect();
    assert_eq!(
        shapes, table,
        "a shape was added or removed: update SHAPE_FIXTURES and add fixtures"
    );
    for (shape, fixture) in SHAPE_FIXTURES {
        let r = report(fixture);
        assert!(
            r.results
                .iter()
                .any(|v| v.shape.as_deref() == Some(format!("{HALI}{shape}").as_str())),
            "{fixture} does not violate {shape}"
        );
    }
}

// ---- criterion 8: deterministic order and paging ---------------------------------------

#[test]
fn report_order_is_deterministic() {
    let store = load("shacl/mixed");
    let first: Vec<Vec<String>> = validate(&store, 0, 50)
        .unwrap()
        .results
        .iter()
        .map(key)
        .collect();
    assert_eq!(first.len(), 5);
    for _ in 0..5 {
        let again: Vec<Vec<String>> = validate(&store, 0, 50)
            .unwrap()
            .results
            .iter()
            .map(key)
            .collect();
        assert_eq!(again, first);
    }
    let report = validate(&store, 0, 50).unwrap();
    let keys: Vec<Vec<String>> = report.results.iter().map(sort_key).collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(
        keys, sorted,
        "results are sorted by (focus, shape, path, constraint, value, message)"
    );
}

#[test]
fn report_pages() {
    let store = load("shacl/mixed");
    let all: Vec<Vec<String>> = validate(&store, 0, 50)
        .unwrap()
        .results
        .iter()
        .map(key)
        .collect();
    assert_eq!(all.len(), 5);

    let page = validate(&store, 1, 2).unwrap();
    assert!(!page.conforms);
    assert_eq!(page.total, 5);
    assert!(page.truncated);
    assert_eq!(page.next_offset, Some(3));
    assert_eq!(page.results.iter().map(key).collect::<Vec<_>>(), all[1..3]);

    let last = validate(&store, 4, 2).unwrap();
    assert_eq!(last.total, 5);
    assert!(!last.truncated);
    assert_eq!(last.next_offset, None);
    assert_eq!(last.results.iter().map(key).collect::<Vec<_>>(), all[4..]);

    let beyond = validate(&store, 9, 2).unwrap();
    assert_eq!(beyond.total, 5);
    assert!(beyond.results.is_empty());
    assert!(!beyond.truncated);
    assert_eq!(beyond.next_offset, None);
}

// ---- criteria 13-15: mutations ----------------------------------------------------------

#[test]
fn concept_add_refused_by_shacl_leaves_store_unchanged() {
    let mut store = load("shacl/extension-shape");
    let before = snapshot(&store);
    let err = concept::add(&ctx(), &mut store, "deep learning", &[])
        .expect_err("the extension shape requires an altLabel");
    assert_eq!(err.rule, "SHACL_VIOLATION");
    assert_eq!(err.exit_code(), 1);
    assert_eq!(
        err.message,
        "the change would leave 1 SHACL violation in the store; nothing was written"
    );
    assert_eq!(err.details["total"], 1);
    assert_eq!(err.details["truncated"], false);
    let violations = err.details["violations"].as_array().unwrap();
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0]["focus_node"], nt("deep-learning"));
    assert_eq!(violations[0]["message"], "Every Concept needs an alias");
    assert_eq!(err.suggested_command.as_deref(), Some("hali validate"));
    assert_eq!(snapshot(&store), before, "the in-memory store is unchanged");
}

#[test]
fn concept_add_refused_on_pre_existing_violation() {
    let mut store = load("shacl/duplicate-label");
    let before = snapshot(&store);
    let err = concept::add(&ctx(), &mut store, "quantum computing", &[])
        .expect_err("the store already violates the label shape");
    assert_eq!(err.rule, "SHACL_VIOLATION");
    assert_eq!(err.exit_code(), 1);
    assert_eq!(err.details["total"], 4);
    assert_eq!(snapshot(&store), before);
}

#[test]
fn label_taken_is_checked_before_shacl() {
    // "ML" is the altLabel of hali:ml, and a new Concept would also lack the required alias.
    let mut store = load("shacl/extension-shape");
    let err = concept::add(&ctx(), &mut store, "ML", &[]).expect_err("the label is taken");
    assert_eq!(err.rule, "LABEL_TAKEN");
    assert_eq!(err.exit_code(), 1);
}

// ---- round 1 review: reachable SPARQL, core-shape overrides, rudof panics ---------------

#[test]
fn extension_reusing_core_sparql_constraint_is_shapes_invalid() {
    assert_shapes_invalid("shacl/extension-reuses-core-sparql");
}

#[test]
fn extension_attaching_prefixes_to_core_constraint_is_shapes_invalid() {
    assert_shapes_invalid("shacl/extension-core-prefixes");
}

#[test]
fn extension_sh_declare_alone_is_shapes_invalid() {
    assert_shapes_invalid("shacl/extension-declare");
}

#[test]
fn extension_sh_prefix_alone_is_shapes_invalid() {
    assert_shapes_invalid("shacl/extension-prefix");
}

#[test]
fn extension_sh_namespace_alone_is_shapes_invalid() {
    assert_shapes_invalid("shacl/extension-namespace");
}

#[test]
fn extension_deactivating_a_core_shape_is_shapes_invalid() {
    assert_shapes_invalid("shacl/extension-deactivates-core-shape");
}

#[test]
fn rudof_panic_on_valid_extension_shape_is_shapes_invalid() {
    // A valid SHACL Core sh:disjoint shape over an inverse path makes shacl-0.3.35 panic.
    assert_shapes_invalid("shacl/extension-panics");
}

#[test]
fn triple_term_in_extension_is_shapes_invalid() {
    assert_shapes_invalid("shacl/extension-triple-term");
    assert_shapes_invalid("shacl/extension-triple-term-hasvalue");
}

#[test]
fn extension_alignment_with_core_terms_still_conforms() {
    let mut store = load("shacl/extension-alignment");
    let r = validate(&store, 0, 50).unwrap_or_else(|e| panic!("{} {}", e.rule, e.message));
    assert!(
        r.conforms,
        "{:?}",
        r.results.iter().map(key).collect::<Vec<_>>()
    );
    concept::add(&ctx(), &mut store, "brand new", &[]).unwrap();
}

// ---- round 2 security: extensions may not reference core shapes as objects ----------------

#[test]
fn extension_referencing_a_core_shape_as_object_is_shapes_invalid() {
    for name in [
        "shacl/extension-node-core-shape",
        "shacl/extension-and-core-shape",
        "shacl/extension-property-core-shape",
        "shacl/extension-qualified-core-shape",
    ] {
        assert_shapes_invalid(name);
    }
}

#[test]
fn extension_objects_naming_core_ttl_and_vocabulary_terms_still_conform() {
    let store = load("shacl/extension-core-term-objects");
    let r = validate(&store, 0, 50).unwrap_or_else(|e| panic!("{} {}", e.rule, e.message));
    assert!(
        r.conforms,
        "{:?}",
        r.results.iter().map(key).collect::<Vec<_>>()
    );
}
