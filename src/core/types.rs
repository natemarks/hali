//! Types (spec §2.2): the `owl:Class`es under `hali:Resource`. `type add`, `type list`,
//! `type remove`, and the resolution of Type references (§9.2).

use super::{Candidate, Context, Error, Mutation, apply, fold, insert_all, validate};
use crate::ontology::{HALI, RESOURCE, owl, rdfs};
use crate::storage::{File, Store};
use oxigraph::model::vocab::rdf;
use oxigraph::model::{
    Literal, NamedNode, NamedNodeRef, NamedOrBlankNode, NamedOrBlankNodeRef, Quad, Term, TermRef,
};
use oxigraph::sparql::{QueryResults, SparqlEvaluator};
use serde::Serialize;
use std::collections::{BTreeSet, HashSet};

/// A Type record, as `--json` prints it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Type {
    pub id: String,
    pub label: String,
    pub parents: Vec<String>,
}

/// One page of `type list`.
#[derive(Debug, Serialize)]
pub struct Page {
    pub results: Vec<Type>,
    pub total: usize,
    pub truncated: bool,
    pub next_offset: Option<usize>,
}

/// The longest Type name.
const MAX_NAME: usize = 64;

/// Why `Resource` is never a Type name or reference.
const ROOT: &str = "Resource is the root class hali:Resource, not a Type";

/// Whether `name` is a valid Type name: an ASCII letter, then ASCII letters, digits, `_` and
/// `-`, at most 64 characters.
pub fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    name.len() <= MAX_NAME
        && chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
}

/// A usage error unless `name` can name a new Type. The CLI runs it at parse time; `add`
/// runs it again.
pub fn check_name(name: &str) -> Result<(), Error> {
    if !valid_name(name) {
        return Err(Error::usage(
            "a Type name starts with an ASCII letter, has only ASCII letters, digits, `_` and `-`, and is at most 64 characters",
        ));
    }
    if name.eq_ignore_ascii_case("resource") {
        return Err(Error::usage(ROOT));
    }
    Ok(())
}

/// The id of the Type a reference names (spec §9.2): the Type whose id is exactly the
/// reference, then one whose id equals it ASCII case-insensitively, then one with a label in the
/// configured language that equals it folded; among several, the first in IRI order. Empty, or
/// `hali:Resource`, is a usage error; anything else is `UNKNOWN_TYPE`.
pub fn resolve(ctx: &Context, store: &Store, reference: &str) -> Result<String, Error> {
    resolve_iri(ctx, store, reference).map(|t| id_of(&t))
}

/// As `resolve`, the Type's IRI.
pub(crate) fn resolve_iri(
    ctx: &Context,
    store: &Store,
    reference: &str,
) -> Result<NamedNode, Error> {
    if reference.trim().is_empty() {
        return Err(Error::usage("a Type reference is empty"));
    }
    let all = types(store);
    if let Some(t) = all.iter().find(|t| id_of(t) == reference) {
        return Ok(t.clone());
    }
    if reference.eq_ignore_ascii_case("resource") || reference == RESOURCE {
        return Err(Error::usage(ROOT));
    }
    if let Some(t) = all
        .iter()
        .find(|t| id_of(t).eq_ignore_ascii_case(reference))
    {
        return Ok(t.clone());
    }
    let key = fold(reference);
    let by_label = all.iter().find(|t| {
        labels(store, t, ctx.language())
            .iter()
            .any(|l| fold(l) == key)
    });
    match by_label {
        Some(t) => Ok(t.clone()),
        None => {
            let candidates = all.iter().map(|t| {
                let id = id_of(t);
                let mut names = labels(store, t, ctx.language());
                names.push(id.clone());
                Candidate {
                    label: label_of(ctx, store, t),
                    id,
                    names,
                }
            });
            Err(Error::unknown_type(
                reference,
                &super::suggest(candidates, &key),
            ))
        }
    }
}

/// Adds a Type named `name` to `vocabulary.ttl`: `owl:Class`, its name as the label in the
/// configured language, and `rdfs:subClassOf` the Type `parent` references, else
/// `hali:Resource`. The checks run in the order of spec §9.1.
pub fn add(
    ctx: &Context,
    store: &mut Store,
    name: &str,
    parent: Option<&str>,
) -> Result<Mutation<Type>, Error> {
    check_name(name)?;
    let parent = parent.map(|p| resolve_iri(ctx, store, p)).transpose()?;
    let key = fold(name);
    for t in types(store) {
        let taken = fold(&id_of(&t)) == key
            || labels(store, &t, ctx.language())
                .iter()
                .any(|l| fold(l) == key);
        if taken {
            return Err(Error::type_label_taken(name, ctx.language(), &id_of(&t)));
        }
    }
    let class = NamedNode::new_unchecked(format!("{HALI}{name}"));
    if is_used(store, &class) {
        return Err(Error::usage(&format!(
            "hali:{name} already names something in the store; choose another name"
        )));
    }

    let literal = Literal::new_language_tagged_literal(name, ctx.language())
        .map_err(|_| Error::usage("the configured language is not a valid language tag"))?;
    let above = parent
        .clone()
        .unwrap_or_else(|| NamedNode::new_unchecked(RESOURCE));
    let graph = File::Vocabulary.graph();
    let quads = vec![
        Quad::new(class.clone(), rdf::TYPE, named(owl::CLASS), graph.clone()),
        Quad::new(class.clone(), named(rdfs::LABEL), literal, graph.clone()),
        Quad::new(class, named(rdfs::SUB_CLASS_OF), above, graph),
    ];
    validate::guard(store, &quads, &[])?;
    let changes = insert_all(store, &quads);
    Ok(Mutation {
        value: Type {
            id: name.to_string(),
            label: name.to_string(),
            parents: parent.iter().map(id_of).collect(),
        },
        changes,
    })
}

/// Lists Types sorted by label (code point order), then id.
pub fn list(ctx: &Context, store: &Store, offset: usize, limit: usize) -> Page {
    let all = types(store);
    let mut records: Vec<Type> = all.iter().map(|t| record(ctx, store, &all, t)).collect();
    records.sort_by(|a, b| (&a.label, &a.id).cmp(&(&b.label, &b.id)));

    let total = records.len();
    let results: Vec<Type> = records.into_iter().skip(offset).take(limit).collect();
    let end = offset.saturating_add(results.len());
    let truncated = end < total;
    Page {
        results,
        total,
        truncated,
        next_offset: truncated.then_some(end),
    }
}

/// Removes a Type from `vocabulary.ttl`: every triple with it as subject and every triple with
/// it as object. Refused with `IN_USE` while any subject has it as `rdf:type`, or another
/// subject is `rdfs:subClassOf` it, in any graph (a blank node counts). The value is the Type as
/// it was.
pub fn remove(ctx: &Context, store: &mut Store, reference: &str) -> Result<Mutation<Type>, Error> {
    let class = resolve_iri(ctx, store, reference)?;
    let all = types(store);
    let was = record(ctx, store, &all, &class);
    let typed = subjects(store, rdf::TYPE, &class).len();
    // Nodes, not ids: two IRIs can share an id, and a blank node is a subclass too.
    let subtypes = subjects(store, named(rdfs::SUB_CLASS_OF), &class)
        .into_iter()
        .filter(|s| !matches!(s, NamedOrBlankNode::NamedNode(n) if *n == class))
        .count();
    if typed > 0 || subtypes > 0 {
        return Err(Error::type_in_use(&was.id, typed, subtypes));
    }

    let graph = File::Vocabulary.graph();
    let mut removed: Vec<Quad> = store
        .quads_for_pattern(
            Some(class.as_ref().into()),
            None,
            None,
            Some(graph.as_ref().into()),
        )
        .chain(store.quads_for_pattern(
            None,
            None,
            Some(TermRef::from(class.as_ref())),
            Some(graph.as_ref().into()),
        ))
        .map(|q| q.expect("reading the in-memory store"))
        .collect();
    removed.sort_by_cached_key(ToString::to_string);
    removed.dedup();

    if !removed.is_empty() {
        validate::guard(store, &[], &removed)?;
    }
    let changes = apply(store, &[], &removed);
    Ok(Mutation {
        value: was,
        changes,
    })
}

/// Every Type: a named `owl:Class` other than `hali:Resource` that is a (transitive)
/// `rdfs:subClassOf` of it, both read from every graph. The query text is fixed.
const TYPES: &str = "SELECT DISTINCT ?t WHERE { \
    ?t a <http://www.w3.org/2002/07/owl#Class> ; \
       <http://www.w3.org/2000/01/rdf-schema#subClassOf>+ <https://w3id.org/hali#Resource> \
    FILTER(isIRI(?t) && ?t != <https://w3id.org/hali#Resource>) }";

/// Every Type, in IRI order.
pub(crate) fn types(store: &Store) -> BTreeSet<NamedNode> {
    let mut query = SparqlEvaluator::new()
        .without_default_http_service_handler()
        .parse_query(TYPES)
        .expect("the fixed query parses");
    query.dataset_mut().set_default_graph_as_union();
    let QueryResults::Solutions(solutions) = query
        .on_store(store)
        .execute()
        .expect("evaluating a fixed query on the in-memory store")
    else {
        unreachable!("a SELECT query gives solutions");
    };
    solutions
        .filter_map(|s| {
            match s
                .expect("evaluating a fixed query on the in-memory store")
                .get("t")
            {
                Some(Term::NamedNode(t)) => Some(t.clone()),
                _ => None,
            }
        })
        .collect()
}

/// A Type's record, read from every graph. `all` is every Type.
fn record(ctx: &Context, store: &Store, all: &BTreeSet<NamedNode>, class: &NamedNode) -> Type {
    let parents: BTreeSet<String> = store
        .quads_for_pattern(
            Some(NamedOrBlankNodeRef::from(class.as_ref())),
            Some(named(rdfs::SUB_CLASS_OF)),
            None,
            None,
        )
        .filter_map(|q| match q.expect("reading the in-memory store").object {
            Term::NamedNode(n) if all.contains(&n) => Some(id_of(&n)),
            _ => None,
        })
        .collect();
    Type {
        id: id_of(class),
        label: label_of(ctx, store, class),
        parents: parents.into_iter().collect(),
    }
}

/// A Type's label: the smallest `rdfs:label` in the configured language, else its id.
fn label_of(ctx: &Context, store: &Store, class: &NamedNode) -> String {
    labels(store, class, ctx.language())
        .into_iter()
        .min()
        .unwrap_or_else(|| id_of(class))
}

/// The values of a Type's `rdfs:label`s in `language`, from every graph.
fn labels(store: &Store, class: &NamedNode, language: &str) -> Vec<String> {
    store
        .quads_for_pattern(
            Some(NamedOrBlankNodeRef::from(class.as_ref())),
            Some(named(rdfs::LABEL)),
            None,
            None,
        )
        .filter_map(|q| match q.expect("reading the in-memory store").object {
            Term::Literal(l) if l.language() == Some(language) => Some(l.value().to_string()),
            _ => None,
        })
        .collect()
}

/// The distinct subjects with `<predicate> <object>`, in any graph.
fn subjects(
    store: &Store,
    predicate: NamedNodeRef<'_>,
    object: &NamedNode,
) -> HashSet<NamedOrBlankNode> {
    store
        .quads_for_pattern(
            None,
            Some(predicate),
            Some(TermRef::from(object.as_ref())),
            None,
        )
        .map(|q| q.expect("reading the in-memory store").subject)
        .collect()
}

/// Whether an IRI already appears in any position of any graph.
fn is_used(store: &Store, iri: &NamedNode) -> bool {
    store
        .quads_for_pattern(Some(iri.as_ref().into()), None, None, None)
        .next()
        .is_some()
        || store
            .quads_for_pattern(None, Some(iri.as_ref()), None, None)
            .next()
            .is_some()
        || store
            .quads_for_pattern(None, None, Some(TermRef::from(iri.as_ref())), None)
            .next()
            .is_some()
}

/// A Type's id: its local name in `hali:`, else its whole IRI.
pub(crate) fn id_of(class: &NamedNode) -> String {
    let iri = class.as_str();
    iri.strip_prefix(HALI).unwrap_or(iri).to_string()
}

fn named(iri: &str) -> NamedNodeRef<'_> {
    NamedNodeRef::new_unchecked(iri)
}
