//! Concepts in the `hali:topics` scheme (spec §2.3): `concept add` and `concept list`.

use super::{Context, Error, Mutation, insert_all, normalise_label, slug, validate};
use crate::ontology::{HALI, TOPICS, skos};
use crate::storage::{File, Store};
use oxigraph::model::vocab::rdf;
use oxigraph::model::{Literal, NamedNode, NamedNodeRef, NamedOrBlankNodeRef, Quad, Term, TermRef};
use serde::Serialize;
use std::collections::BTreeSet;

/// A Concept record, as `--json` prints it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Concept {
    pub id: String,
    pub label: String,
    pub alt_labels: Vec<String>,
    pub broader: Vec<String>,
}

/// One page of a list.
#[derive(Debug, Serialize)]
pub struct Page {
    pub results: Vec<Concept>,
    pub total: usize,
    pub truncated: bool,
    pub next_offset: Option<usize>,
}

/// Adds a Concept with `label` as its prefLabel, minting its IRI from the label's slug.
/// Writes the `hali:topics` scheme too, the first time. Refused, with the store unchanged, if
/// the store would not conform to the SHACL shapes.
pub fn add(ctx: &Context, store: &mut Store, label: &str) -> Result<Mutation<Concept>, Error> {
    let label = normalise_label(label)?;
    let key = fold(&label);
    for concept in concepts(store) {
        let taken = labels(store, concept.as_ref(), skos::PREF_LABEL, ctx.language())
            .chain(labels(
                store,
                concept.as_ref(),
                skos::ALT_LABEL,
                ctx.language(),
            ))
            .any(|l| fold(&l) == key);
        if taken {
            return Err(Error::label_taken(&label, ctx.language(), &id_of(&concept)));
        }
    }

    let base = slug(&label);
    let id = (1..)
        .map(|n| match n {
            1 => base.clone(),
            n => format!("{base}-{n}"),
        })
        .find(|id| {
            let candidate = iri(id);
            candidate.as_str() != TOPICS && !is_used(store, &candidate)
        })
        .expect("some suffix is free");

    let concept = iri(&id);
    let topics = NamedNode::new_unchecked(TOPICS);
    let literal = Literal::new_language_tagged_literal(&label, ctx.language())
        .map_err(|_| Error::usage("the configured language is not a valid language tag"))?;
    let graph = File::Vocabulary.graph();
    let quads: Vec<Quad> = [
        (&topics, rdf::TYPE, term(skos::CONCEPT_SCHEME)),
        (&concept, rdf::TYPE, term(skos::CONCEPT)),
        (&concept, named(skos::IN_SCHEME), Term::from(topics.clone())),
        (&concept, named(skos::PREF_LABEL), Term::from(literal)),
    ]
    .into_iter()
    .map(|(s, p, o)| Quad::new(s.clone(), p, o, graph.clone()))
    .collect();
    validate::guard(store, &quads, &[])?;
    let changes = insert_all(store, &quads);

    Ok(Mutation {
        value: Concept {
            id,
            label,
            alt_labels: Vec::new(),
            broader: Vec::new(),
        },
        changes,
    })
}

/// Lists Concepts sorted by label (code point order), then id.
pub fn list(ctx: &Context, store: &Store, offset: usize, limit: usize) -> Page {
    let mut all: Vec<Concept> = concepts(store)
        .into_iter()
        .map(|c| {
            let lang = ctx.language();
            let pref: BTreeSet<String> =
                labels(store, c.as_ref(), skos::PREF_LABEL, lang).collect();
            let alt: BTreeSet<String> = labels(store, c.as_ref(), skos::ALT_LABEL, lang).collect();
            let broader: BTreeSet<String> = objects(store, c.as_ref(), skos::BROADER)
                .filter_map(|o| match o {
                    Term::NamedNode(n) => Some(id_of(&n)),
                    _ => None,
                })
                .collect();
            Concept {
                id: id_of(&c),
                label: pref.into_iter().next().unwrap_or_default(),
                alt_labels: alt.into_iter().collect(),
                broader: broader.into_iter().collect(),
            }
        })
        .collect();
    all.sort_by(|a, b| (&a.label, &a.id).cmp(&(&b.label, &b.id)));

    let total = all.len();
    let results: Vec<Concept> = all.into_iter().skip(offset).take(limit).collect();
    let end = offset.saturating_add(results.len());
    let truncated = end < total;
    Page {
        results,
        total,
        truncated,
        next_offset: truncated.then_some(end),
    }
}

/// Every Concept in `hali:topics`, in any graph.
fn concepts(store: &Store) -> BTreeSet<NamedNode> {
    let topics = term(TOPICS);
    store
        .quads_for_pattern(
            None,
            Some(rdf::TYPE),
            Some(named(skos::CONCEPT).into()),
            None,
        )
        .map(|q| q.expect("reading the in-memory store").subject)
        .filter_map(|s| match s {
            oxigraph::model::NamedOrBlankNode::NamedNode(n) => Some(n),
            _ => None,
        })
        .filter(|n| objects(store, n.as_ref(), skos::IN_SCHEME).any(|o| o == topics))
        .collect()
}

fn objects<'a>(
    store: &'a Store,
    subject: NamedNodeRef<'a>,
    predicate: &'a str,
) -> impl Iterator<Item = Term> + 'a {
    store
        .quads_for_pattern(
            Some(NamedOrBlankNodeRef::from(subject)),
            Some(named(predicate)),
            None,
            None,
        )
        .map(|q| q.expect("reading the in-memory store").object)
}

/// The values of a Concept's labels in `language`.
fn labels<'a>(
    store: &'a Store,
    subject: NamedNodeRef<'a>,
    predicate: &'a str,
    language: &'a str,
) -> impl Iterator<Item = String> + 'a {
    objects(store, subject, predicate).filter_map(move |o| match o {
        Term::Literal(l) if l.language() == Some(language) => Some(l.value().to_string()),
        _ => None,
    })
}

/// Whether an IRI already names something, in any graph.
fn is_used(store: &Store, iri: &NamedNode) -> bool {
    store
        .quads_for_pattern(Some(iri.as_ref().into()), None, None, None)
        .next()
        .is_some()
        || store
            .quads_for_pattern(None, None, Some(TermRef::from(iri.as_ref())), None)
            .next()
            .is_some()
}

/// How labels compare for uniqueness: whitespace-normalised and case-insensitive.
fn fold(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn iri(id: &str) -> NamedNode {
    NamedNode::new_unchecked(format!("{HALI}{id}"))
}

/// A Concept's id: its local name in `hali:`, else its whole IRI.
fn id_of(concept: &NamedNode) -> String {
    let iri = concept.as_str();
    iri.strip_prefix(HALI).unwrap_or(iri).to_string()
}

fn named(iri: &str) -> NamedNodeRef<'_> {
    NamedNodeRef::new_unchecked(iri)
}

fn term(iri: &str) -> Term {
    NamedNode::new_unchecked(iri).into()
}
