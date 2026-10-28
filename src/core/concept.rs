//! Concepts in the `hali:topics` scheme (spec §2.3): `concept add`, `concept list`, and the
//! resolution of Concept references (§9.2).

use super::resource::Tag;
use super::{Context, Error, Mutation, apply, insert_all, normalise_label, slug, validate};
use crate::ontology::{HALI, TOPICS, dcterms, skos};
use crate::storage::{File, Store};
use oxigraph::model::vocab::rdf;
use oxigraph::model::{
    Literal, NamedNode, NamedNodeRef, NamedOrBlankNode, NamedOrBlankNodeRef, Quad, Term, TermRef,
};
use serde::Serialize;
use std::collections::{BTreeSet, HashSet};

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

/// The id of the Concept a reference names (spec §9.2): first the Concept whose id (slug) is
/// exactly the reference, then the one with a prefLabel or altLabel in the configured language
/// that equals it, whitespace-normalised and case-insensitive. Otherwise `UNKNOWN_CONCEPT`,
/// with the closest Concepts as suggestions.
pub fn resolve(ctx: &Context, store: &Store, reference: &str) -> Result<String, Error> {
    resolve_iri(ctx, store, reference).map(|c| id_of(&c))
}

/// As `resolve`, the Concept's IRI.
pub(crate) fn resolve_iri(
    ctx: &Context,
    store: &Store,
    reference: &str,
) -> Result<NamedNode, Error> {
    if reference.trim().is_empty() {
        return Err(Error::usage("a Concept reference is empty"));
    }
    let all = concepts(store);
    if let Some(c) = all.iter().find(|c| id_of(c) == reference) {
        return Ok(c.clone());
    }
    let key = fold(reference);
    let by_label = all.iter().find(|c| {
        all_labels(store, c.as_ref(), ctx.language())
            .iter()
            .any(|l| fold(l) == key)
    });
    match by_label {
        Some(c) => Ok(c.clone()),
        None => Err(Error::unknown_concept(
            reference,
            &suggest(ctx, store, &all, &key),
        )),
    }
}

/// At most this many Concepts are suggested for an unknown reference.
const MAX_SUGGESTIONS: usize = 3;

/// The Concepts closest to the folded reference `key`: those whose folded id or any label
/// contains it, or is within edit distance max(2, len/3) of it, by distance, label, then id.
fn suggest(ctx: &Context, store: &Store, all: &BTreeSet<NamedNode>, key: &str) -> Vec<Tag> {
    let limit = (key.chars().count() / 3).max(2);
    let mut found: Vec<(usize, Tag)> = all
        .iter()
        .filter_map(|c| {
            let id = id_of(c);
            let mut names = all_labels(store, c.as_ref(), ctx.language());
            names.push(id.clone());
            let names: Vec<String> = names.iter().map(|n| fold(n)).collect();
            let distance = names.iter().map(|n| levenshtein(n, key)).min()?;
            let close = distance <= limit || names.iter().any(|n| n.contains(key));
            close.then(|| {
                let label = labels(store, c.as_ref(), skos::PREF_LABEL, ctx.language())
                    .min()
                    .unwrap_or_default();
                (distance, Tag { id, label })
            })
        })
        .collect();
    found.sort_by(|(da, a), (db, b)| (da, &a.label, &a.id).cmp(&(db, &b.label, &b.id)));
    found
        .into_iter()
        .take(MAX_SUGGESTIONS)
        .map(|(_, t)| t)
        .collect()
}

/// The edit distance between two strings, in characters.
fn levenshtein(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = if ca == *cb {
                diagonal
            } else {
                1 + diagonal.min(above).min(row[j])
            };
            diagonal = above;
        }
    }
    row[b.len()]
}

/// Adds a Concept with `label` as its prefLabel, minting its IRI from the label's slug, and a
/// `skos:broader` link to each Concept that `broader` references. Writes the `hali:topics`
/// scheme too, the first time. Refused, with the store unchanged, on an unknown reference or
/// if the store would not conform to the SHACL shapes.
pub fn add(
    ctx: &Context,
    store: &mut Store,
    label: &str,
    broader: &[String],
) -> Result<Mutation<Concept>, Error> {
    let label = normalise_label(label)?;
    let parents: BTreeSet<NamedNode> = broader
        .iter()
        .map(|r| resolve_iri(ctx, store, r))
        .collect::<Result<_, _>>()?;
    let key = fold(&label);
    for concept in concepts(store) {
        let taken = all_labels(store, concept.as_ref(), ctx.language())
            .iter()
            .any(|l| fold(l) == key);
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
    let mut triples = vec![
        (&topics, rdf::TYPE, term(skos::CONCEPT_SCHEME)),
        (&concept, rdf::TYPE, term(skos::CONCEPT)),
        (&concept, named(skos::IN_SCHEME), Term::from(topics.clone())),
        (&concept, named(skos::PREF_LABEL), Term::from(literal)),
    ];
    for parent in &parents {
        triples.push((&concept, named(skos::BROADER), Term::from(parent.clone())));
    }
    let quads: Vec<Quad> = triples
        .into_iter()
        .map(|(s, p, o)| Quad::new(s.clone(), p, o, graph.clone()))
        .collect();
    validate::guard(store, &quads, &[])?;
    let changes = insert_all(store, &quads);

    let broader: BTreeSet<String> = parents.iter().map(id_of).collect();
    Ok(Mutation {
        value: Concept {
            id,
            label,
            alt_labels: Vec::new(),
            broader: broader.into_iter().collect(),
        },
        changes,
    })
}

/// Lists Concepts sorted by label (code point order), then id.
pub fn list(ctx: &Context, store: &Store, offset: usize, limit: usize) -> Page {
    let mut all: Vec<Concept> = concepts(store)
        .into_iter()
        .map(|c| record(ctx, store, &c))
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

/// A Concept's record, in the configured language, read from every graph.
fn record(ctx: &Context, store: &Store, concept: &NamedNode) -> Concept {
    let lang = ctx.language();
    let pref: BTreeSet<String> = labels(store, concept.as_ref(), skos::PREF_LABEL, lang).collect();
    let alt: BTreeSet<String> = labels(store, concept.as_ref(), skos::ALT_LABEL, lang).collect();
    let broader: BTreeSet<String> = objects(store, concept.as_ref(), skos::BROADER)
        .filter_map(|o| match o {
            Term::NamedNode(n) => Some(id_of(&n)),
            _ => None,
        })
        .collect();
    Concept {
        id: id_of(concept),
        label: pref.into_iter().next().unwrap_or_default(),
        alt_labels: alt.into_iter().collect(),
        broader: broader.into_iter().collect(),
    }
}

/// The ids of the Concepts directly under `concept` (`skos:broader <concept>`), in any graph,
/// sorted. Derived by query; `skos:narrower` is never stored.
fn narrower_of(store: &Store, concept: &NamedNode) -> BTreeSet<String> {
    subjects(store, skos::BROADER, concept)
        .into_iter()
        .filter_map(|s| match s {
            NamedOrBlankNode::NamedNode(n) => Some(id_of(&n)),
            NamedOrBlankNode::BlankNode(_) => None,
        })
        .collect()
}

/// The distinct subjects with `<predicate> <object>`, in any graph.
fn subjects(store: &Store, predicate: &str, object: &NamedNode) -> HashSet<NamedOrBlankNode> {
    store
        .quads_for_pattern(
            None,
            Some(named(predicate)),
            Some(TermRef::from(object.as_ref())),
            None,
        )
        .map(|q| q.expect("reading the in-memory store").subject)
        .collect()
}

/// A Concept as `concept show` prints it.
#[derive(Debug, PartialEq, Serialize)]
pub struct Shown {
    pub id: String,
    pub label: String,
    pub alt_labels: Vec<String>,
    pub broader: Vec<String>,
    pub narrower: Vec<String>,
    pub tagged: usize,
}

/// One Concept with its derived `narrower` and the number of subjects tagged with it. Read only.
pub fn show(ctx: &Context, store: &Store, reference: &str) -> Result<Shown, Error> {
    let concept = resolve_iri(ctx, store, reference)?;
    let Concept {
        id,
        label,
        alt_labels,
        broader,
    } = record(ctx, store, &concept);
    Ok(Shown {
        id,
        label,
        alt_labels,
        broader,
        narrower: narrower_of(store, &concept).into_iter().collect(),
        tagged: subjects(store, dcterms::SUBJECT, &concept).len(),
    })
}

/// What `concept edit` changes; labels are in the configured language.
#[derive(Clone, Debug, Default)]
pub struct ConceptEdit {
    pub add_alt: Vec<String>,
    pub remove_alt: Vec<String>,
    pub add_broader: Vec<String>,
    pub remove_broader: Vec<String>,
}

/// A Concept other than `own` that holds the label folded to `key` in the configured language.
fn label_holder(ctx: &Context, store: &Store, own: &NamedNode, key: &str) -> Option<NamedNode> {
    concepts(store).into_iter().find(|c| {
        c != own
            && all_labels(store, c.as_ref(), ctx.language())
                .iter()
                .any(|l| fold(l) == key)
    })
}

/// The vocabulary-graph quads of `subject`'s `predicate` whose literal is in `language`, with
/// the literal's text.
fn vocabulary_labels(
    store: &Store,
    subject: &NamedNode,
    predicate: &str,
    language: &str,
) -> Vec<(String, Quad)> {
    let graph = File::Vocabulary.graph();
    store
        .quads_for_pattern(
            Some(subject.as_ref().into()),
            Some(named(predicate)),
            None,
            Some(graph.as_ref().into()),
        )
        .map(|q| q.expect("reading the in-memory store"))
        .filter_map(|q| match &q.object {
            Term::Literal(l) if l.language() == Some(language) => Some((l.value().to_string(), q)),
            _ => None,
        })
        .collect()
}

fn label_quad(
    ctx: &Context,
    concept: &NamedNode,
    predicate: &str,
    label: &str,
) -> Result<Quad, Error> {
    let literal = Literal::new_language_tagged_literal(label, ctx.language())
        .map_err(|_| Error::usage("the configured language is not a valid language tag"))?;
    Ok(Quad::new(
        concept.clone(),
        named(predicate),
        literal,
        File::Vocabulary.graph(),
    ))
}

fn broader_quad(concept: &NamedNode, parent: &NamedNode) -> Quad {
    Quad::new(
        concept.clone(),
        named(skos::BROADER),
        parent.clone(),
        File::Vocabulary.graph(),
    )
}

/// Replaces the Concept's prefLabel in the configured language with `new_label` and keeps the
/// old one as an altLabel; the IRI never changes. A label another Concept holds is refused
/// (`LABEL_TAKEN`); one of the Concept's own altLabels swaps places with the prefLabel.
pub fn rename(
    ctx: &Context,
    store: &mut Store,
    reference: &str,
    new_label: &str,
) -> Result<Mutation<Concept>, Error> {
    let new_label = normalise_label(new_label)?;
    let concept = resolve_iri(ctx, store, reference)?;
    let key = fold(&new_label);
    if let Some(holder) = label_holder(ctx, store, &concept, &key) {
        return Err(Error::label_taken(
            &new_label,
            ctx.language(),
            &id_of(&holder),
        ));
    }

    let lang = ctx.language();
    let old_prefs = vocabulary_labels(store, &concept, skos::PREF_LABEL, lang);
    if old_prefs.iter().any(|(l, _)| *l == new_label) {
        return Ok(Mutation {
            value: record(ctx, store, &concept),
            changes: Vec::new(),
        });
    }
    let mut added = vec![label_quad(ctx, &concept, skos::PREF_LABEL, &new_label)?];
    let mut removed: Vec<Quad> = old_prefs.iter().map(|(_, q)| q.clone()).collect();
    for (old, _) in &old_prefs {
        if fold(old) != key {
            added.push(label_quad(ctx, &concept, skos::ALT_LABEL, old)?);
        }
    }
    for (alt, quad) in vocabulary_labels(store, &concept, skos::ALT_LABEL, lang) {
        if fold(&alt) == key {
            removed.push(quad);
        }
    }

    validate::guard(store, &added, &removed)?;
    let changes = apply(store, &added, &removed);
    Ok(Mutation {
        value: record(ctx, store, &concept),
        changes,
    })
}

/// A usage error when one folded label is in both `--add-alt` and `--remove-alt`. The CLI runs
/// it before it reads the store; `edit` runs it again.
pub fn check_alt_lists(add_alt: &[String], remove_alt: &[String]) -> Result<(), Error> {
    let removing: BTreeSet<String> = remove_alt.iter().map(|l| fold(l)).collect();
    if add_alt.iter().any(|l| removing.contains(&fold(l))) {
        return Err(Error::usage(
            "a label cannot be both added and removed as an altLabel",
        ));
    }
    Ok(())
}

/// Adds and removes altLabels and broader links of one Concept, in one mutation. The checks run
/// in the order of spec §9.1; every no-op is silent.
pub fn edit(
    ctx: &Context,
    store: &mut Store,
    reference: &str,
    edit: &ConceptEdit,
) -> Result<Mutation<Concept>, Error> {
    if edit.add_alt.is_empty()
        && edit.remove_alt.is_empty()
        && edit.add_broader.is_empty()
        && edit.remove_broader.is_empty()
    {
        return Err(Error::usage(
            "nothing to change: give at least one change flag",
        ));
    }
    let add_alt: Vec<String> = edit
        .add_alt
        .iter()
        .map(|l| normalise_label(l))
        .collect::<Result<_, _>>()?;
    let remove_alt: Vec<String> = edit
        .remove_alt
        .iter()
        .map(|l| normalise_label(l))
        .collect::<Result<_, _>>()?;
    check_alt_lists(&add_alt, &remove_alt)?;

    let concept = resolve_iri(ctx, store, reference)?;
    let add_broader: Vec<NamedNode> = edit
        .add_broader
        .iter()
        .map(|r| resolve_iri(ctx, store, r))
        .collect::<Result<_, _>>()?;
    let remove_broader: Vec<NamedNode> = edit
        .remove_broader
        .iter()
        .map(|r| resolve_iri(ctx, store, r))
        .collect::<Result<_, _>>()?;
    if add_broader.contains(&concept) {
        return Err(Error::usage("a Concept cannot be broader than itself"));
    }
    if add_broader.iter().any(|p| remove_broader.contains(p)) {
        return Err(Error::usage(
            "a Concept cannot be both added and removed as broader",
        ));
    }

    let lang = ctx.language();
    let mut held: BTreeSet<String> = all_labels(store, concept.as_ref(), lang)
        .iter()
        .map(|l| fold(l))
        .collect();
    let mut added: Vec<Quad> = Vec::new();
    for label in &add_alt {
        let key = fold(label);
        if let Some(holder) = label_holder(ctx, store, &concept, &key) {
            return Err(Error::label_taken(label, lang, &id_of(&holder)));
        }
        if held.insert(key) {
            added.push(label_quad(ctx, &concept, skos::ALT_LABEL, label)?);
        }
    }
    added.extend(add_broader.iter().map(|p| broader_quad(&concept, p)));

    let removing: BTreeSet<String> = remove_alt.iter().map(|l| fold(l)).collect();
    let mut removed: Vec<Quad> = vocabulary_labels(store, &concept, skos::ALT_LABEL, lang)
        .into_iter()
        .filter(|(alt, _)| removing.contains(&fold(alt)))
        .map(|(_, q)| q)
        .collect();
    removed.extend(remove_broader.iter().map(|p| broader_quad(&concept, p)));

    let holds = |q: &Quad| store.contains(q).expect("reading the in-memory store");
    if !(added.iter().all(holds) && !removed.iter().any(holds)) {
        validate::guard(store, &added, &removed)?;
    }
    let changes = apply(store, &added, &removed);
    Ok(Mutation {
        value: record(ctx, store, &concept),
        changes,
    })
}

/// Removes a Concept from `vocabulary.ttl`: every triple with it as subject and every triple
/// with it as object. Refused with `IN_USE` while anything is tagged with it or another Concept
/// is under it, in any graph. The `hali:topics` scheme node stays. The value is the Concept as
/// it was.
pub fn remove(
    ctx: &Context,
    store: &mut Store,
    reference: &str,
) -> Result<Mutation<Concept>, Error> {
    let concept = resolve_iri(ctx, store, reference)?;
    let was = record(ctx, store, &concept);
    let tagged = subjects(store, dcterms::SUBJECT, &concept).len();
    let mut narrower = narrower_of(store, &concept);
    narrower.remove(&was.id);
    if tagged > 0 || !narrower.is_empty() {
        return Err(Error::in_use(&was.id, tagged, narrower.len()));
    }

    let graph = File::Vocabulary.graph();
    let mut removed: Vec<Quad> = store
        .quads_for_pattern(
            Some(concept.as_ref().into()),
            None,
            None,
            Some(graph.as_ref().into()),
        )
        .chain(store.quads_for_pattern(
            None,
            None,
            Some(TermRef::from(concept.as_ref())),
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

/// A Concept's prefLabels and altLabels in `language`.
fn all_labels(store: &Store, subject: NamedNodeRef<'_>, language: &str) -> Vec<String> {
    labels(store, subject, skos::PREF_LABEL, language)
        .chain(labels(store, subject, skos::ALT_LABEL, language))
        .collect()
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
