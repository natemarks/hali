//! Relations between Resources (spec §2.4): the relation kinds, `relation add`, `relation list`,
//! `relation remove`, `relate`, `unrelate`, the resolution of relation references (§9.2), and
//! the fixed queries that `find`, `show` and `forget` share.

use super::{
    Candidate, Context, Error, Mutation, apply, fold, insert_all, is_used, resource, validate,
};
use crate::ontology::{HALI, PREFIXES, STANDARD_RELATIONS, owl, rdfs};
use crate::storage::{File, Store};
use oxigraph::model::vocab::rdf;
use oxigraph::model::{
    Literal, NamedNode, NamedNodeRef, NamedOrBlankNode, NamedOrBlankNodeRef, Quad, Term, TermRef,
    Variable,
};
use oxigraph::sparql::{QueryResults, SparqlEvaluator};
use serde::Serialize;
use std::collections::{BTreeSet, HashSet};

/// A relation kind record, as `--json` prints it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RelationKind {
    pub id: String,
    pub label: String,
    pub parents: Vec<String>,
    pub standard: bool,
}

/// One page of `relation list`.
#[derive(Debug, Serialize)]
pub struct Page {
    pub results: Vec<RelationKind>,
    pub total: usize,
    pub truncated: bool,
    pub next_offset: Option<usize>,
}

/// One relation: the Resource ids (bare UUIDs) and the kind id.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Link {
    pub from: String,
    pub relation: String,
    pub to: String,
}

/// The longest relation kind name.
const MAX_NAME: usize = 64;

/// Whether `name` is a valid relation kind name: an ASCII lowercase letter, then ASCII letters,
/// digits, `_` and `-`, at most 64 characters.
pub fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    name.len() <= MAX_NAME
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
}

/// A usage error unless `name` can name a new relation kind. The CLI runs it at parse time;
/// `add` runs it again.
pub fn check_name(name: &str) -> Result<(), Error> {
    if valid_name(name) {
        Ok(())
    } else {
        Err(Error::usage(
            "a relation name starts with an ASCII lowercase letter, has only ASCII letters, digits, `_` and `-`, and is at most 64 characters",
        ))
    }
}

/// The id of the relation kind a reference names (spec §9.2): the kind whose id is exactly the
/// reference, then the one whose IRI is the reference expanded through the fixed prefix table,
/// then one whose id equals it ASCII case-insensitively (the first in IRI order). Empty is a
/// usage error; anything else is `UNKNOWN_RELATION`.
pub fn resolve(ctx: &Context, store: &Store, reference: &str) -> Result<String, Error> {
    resolve_iri(ctx, store, reference).map(|k| id_of(&k))
}

/// As `resolve`, the kind's IRI.
pub(crate) fn resolve_iri(
    ctx: &Context,
    store: &Store,
    reference: &str,
) -> Result<NamedNode, Error> {
    if reference.trim().is_empty() {
        return Err(Error::usage("a relation reference is empty"));
    }
    let all = kinds(store);
    if let Some(k) = all.iter().find(|k| id_of(k) == reference) {
        return Ok(k.clone());
    }
    if let Some(iri) = expand(reference)
        && let Some(k) = all.iter().find(|k| k.as_str() == iri)
    {
        return Ok(k.clone());
    }
    if let Some(k) = all
        .iter()
        .find(|k| id_of(k).eq_ignore_ascii_case(reference))
    {
        return Ok(k.clone());
    }
    let candidates = all.iter().map(|k| {
        let id = id_of(k);
        let mut names = labels(store, k, ctx.language());
        names.push(local(k).to_string());
        names.push(id.clone());
        Candidate {
            label: label_of(ctx, store, k),
            id,
            names,
        }
    });
    Err(Error::unknown_relation(
        reference,
        &super::suggest(candidates, &fold(reference)),
    ))
}

/// Adds a relation kind named `name` to `vocabulary.ttl`: `owl:ObjectProperty`, its name as the
/// label in the configured language, and `rdfs:subPropertyOf` the standard relation `parent`
/// references. The checks run in the order of spec §9.1.
pub fn add(
    ctx: &Context,
    store: &mut Store,
    name: &str,
    parent: &str,
) -> Result<Mutation<RelationKind>, Error> {
    check_name(name)?;
    let parent = resolve_iri(ctx, store, parent)?;
    if !is_standard(&parent) {
        return Err(Error::usage(
            "--parent must be a standard relation: dcterms:relation, dcterms:isPartOf, dcterms:references or prov:wasDerivedFrom",
        ));
    }
    let key = fold(name);
    for k in kinds(store) {
        let taken = fold(local(&k)) == key
            || labels(store, &k, ctx.language())
                .iter()
                .any(|l| fold(l) == key);
        if taken {
            return Err(Error::relation_label_taken(
                name,
                ctx.language(),
                &id_of(&k),
            ));
        }
    }
    let property = NamedNode::new_unchecked(format!("{HALI}{name}"));
    if is_used(store, &property) {
        return Err(Error::usage(&format!(
            "hali:{name} already names something in the store; choose another name"
        )));
    }

    let literal = Literal::new_language_tagged_literal(name, ctx.language())
        .map_err(|_| Error::usage("the configured language is not a valid language tag"))?;
    let graph = File::Vocabulary.graph();
    let quads = vec![
        Quad::new(
            property.clone(),
            rdf::TYPE,
            named(owl::OBJECT_PROPERTY),
            graph.clone(),
        ),
        Quad::new(property.clone(), named(rdfs::LABEL), literal, graph.clone()),
        Quad::new(
            property,
            named(rdfs::SUB_PROPERTY_OF),
            parent.clone(),
            graph,
        ),
    ];
    validate::guard(store, &quads, &[])?;
    let changes = insert_all(store, &quads);
    Ok(Mutation {
        value: RelationKind {
            id: name.to_string(),
            label: name.to_string(),
            parents: vec![id_of(&parent)],
            standard: false,
        },
        changes,
    })
}

/// Lists the relation kinds, standard and custom, sorted by label (code point order), then id.
pub fn list(ctx: &Context, store: &Store, offset: usize, limit: usize) -> Page {
    let all = kinds(store);
    let mut records: Vec<RelationKind> = all.iter().map(|k| record(ctx, store, &all, k)).collect();
    records.sort_by(|a, b| (&a.label, &a.id).cmp(&(&b.label, &b.id)));

    let total = records.len();
    let results: Vec<RelationKind> = records.into_iter().skip(offset).take(limit).collect();
    let end = offset.saturating_add(results.len());
    let truncated = end < total;
    Page {
        results,
        total,
        truncated,
        next_offset: truncated.then_some(end),
    }
}

/// Removes a custom relation kind from `vocabulary.ttl`: every triple with it as subject and
/// every triple with it as object. A standard relation is a usage error. Refused with `IN_USE`
/// while any triple in any graph has it as predicate, or another subject is
/// `rdfs:subPropertyOf` it (a blank node counts). The value is the kind as it was.
pub fn remove(
    ctx: &Context,
    store: &mut Store,
    reference: &str,
) -> Result<Mutation<RelationKind>, Error> {
    let property = resolve_iri(ctx, store, reference)?;
    let all = kinds(store);
    let was = record(ctx, store, &all, &property);
    if was.standard {
        return Err(Error::usage(&format!(
            "{} is a standard relation and cannot be removed",
            was.id
        )));
    }
    // Nodes, not ids: the same triple in two graphs is one use.
    let used: HashSet<(NamedOrBlankNode, Term)> = store
        .quads_for_pattern(None, Some(property.as_ref()), None, None)
        .map(|q| {
            let q = q.expect("reading the in-memory store");
            (q.subject, q.object)
        })
        .collect();
    let subproperties: HashSet<NamedOrBlankNode> = store
        .quads_for_pattern(
            None,
            Some(named(rdfs::SUB_PROPERTY_OF)),
            Some(TermRef::from(property.as_ref())),
            None,
        )
        .map(|q| q.expect("reading the in-memory store").subject)
        .filter(|s| !matches!(s, NamedOrBlankNode::NamedNode(n) if *n == property))
        .collect();
    if !used.is_empty() || !subproperties.is_empty() {
        return Err(Error::relation_in_use(
            &was.id,
            used.len(),
            subproperties.len(),
        ));
    }

    let graph = File::Vocabulary.graph();
    let mut removed: Vec<Quad> = store
        .quads_for_pattern(
            Some(property.as_ref().into()),
            None,
            None,
            Some(graph.as_ref().into()),
        )
        .chain(store.quads_for_pattern(
            None,
            None,
            Some(TermRef::from(property.as_ref())),
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

/// Relates the Resource `from` to the Resource `to` by the relation kind `relation` references:
/// writes the one triple `<from> <relation> <to>` to `resources.ttl` (never the inverse) and
/// bumps `from`'s catalog record `dcterms:modified`. A triple `resources.ttl` holds already is a
/// no-op. The checks run in the order of spec §9.1.
pub fn relate(
    ctx: &Context,
    store: &mut Store,
    from: &str,
    relation: &str,
    to: &str,
) -> Result<Mutation<Link>, Error> {
    let (subject, property, object, value) = link(ctx, store, from, relation, to, false)?;
    let quad = Quad::new(subject.clone(), property, object, File::Resources.graph());
    if store.contains(&quad).expect("reading the in-memory store") {
        return Ok(Mutation {
            value,
            changes: Vec::new(),
        });
    }
    let mut added = vec![quad];
    let mut removed = Vec::new();
    resource::touch(ctx, store, &subject, &mut added, &mut removed);
    validate::guard(store, &added, &removed)?;
    let changes = apply(store, &added, &removed);
    Ok(Mutation { value, changes })
}

/// Removes exactly the triple `<from> <relation> <to>` from `resources.ttl` (not a
/// sub-property's, not the reverse) and bumps `from`'s catalog record `dcterms:modified`. `to`
/// may be the full UUID of a Resource that is no longer cataloged. An absent triple, or one only
/// `extensions.ttl` holds, is a no-op.
pub fn unrelate(
    ctx: &Context,
    store: &mut Store,
    from: &str,
    relation: &str,
    to: &str,
) -> Result<Mutation<Link>, Error> {
    let (subject, property, object, value) = link(ctx, store, from, relation, to, true)?;
    let quad = Quad::new(subject.clone(), property, object, File::Resources.graph());
    if !store.contains(&quad).expect("reading the in-memory store") {
        return Ok(Mutation {
            value,
            changes: Vec::new(),
        });
    }
    let mut added = Vec::new();
    let mut removed = vec![quad];
    resource::touch(ctx, store, &subject, &mut added, &mut removed);
    validate::guard(store, &added, &removed)?;
    let changes = apply(store, &added, &removed);
    Ok(Mutation { value, changes })
}

/// The checks `relate` and `unrelate` share, in order: `from` is cataloged, the relation
/// resolves, `to` is cataloged (or, if `gone_ok`, any full UUID), and `from` is not `to`.
fn link(
    ctx: &Context,
    store: &Store,
    from: &str,
    relation: &str,
    to: &str,
    gone_ok: bool,
) -> Result<(NamedNode, NamedNode, NamedNode, Link), Error> {
    let subject = resource::known(store, from)?;
    let property = resolve_iri(ctx, store, relation)?;
    let object = match resource::known(store, to) {
        Ok(object) => object,
        Err(err) => match resource::parse_uuid(to) {
            Some(id) if gone_ok => resource::urn(&id),
            _ => return Err(err),
        },
    };
    if subject == object {
        return Err(Error::usage("a Resource cannot be related to itself"));
    }
    let value = Link {
        from: resource::id_of(&subject),
        relation: id_of(&property),
        to: resource::id_of(&object),
    };
    Ok((subject, property, object, value))
}

/// Every relation kind: the four standard relations, and every named `owl:ObjectProperty` that
/// is a (transitive) `rdfs:subPropertyOf` one, both read from every graph. The text is fixed.
const KINDS: &str = "SELECT DISTINCT ?p WHERE { \
    ?p a <http://www.w3.org/2002/07/owl#ObjectProperty> ; \
       <http://www.w3.org/2000/01/rdf-schema#subPropertyOf>+ ?std \
    VALUES ?std { <http://purl.org/dc/terms/relation> <http://purl.org/dc/terms/isPartOf> \
      <http://purl.org/dc/terms/references> <http://www.w3.org/ns/prov#wasDerivedFrom> } \
    FILTER(isIRI(?p)) }";

/// Every relation kind, in IRI order.
pub(crate) fn kinds(store: &Store) -> BTreeSet<NamedNode> {
    let mut out: BTreeSet<NamedNode> = STANDARD_RELATIONS
        .iter()
        .map(|iri| NamedNode::new_unchecked(*iri))
        .collect();
    let mut query = SparqlEvaluator::new()
        .without_default_http_service_handler()
        .parse_query(KINDS)
        .expect("the fixed query parses");
    query.dataset_mut().set_default_graph_as_union();
    let QueryResults::Solutions(solutions) = query
        .on_store(store)
        .execute()
        .expect("evaluating a fixed query on the in-memory store")
    else {
        unreachable!("a SELECT query gives solutions");
    };
    for s in solutions {
        if let Some(Term::NamedNode(p)) = s
            .expect("evaluating a fixed query on the in-memory store")
            .get("p")
        {
            out.insert(p.clone());
        }
    }
    out
}

/// The subjects and objects `?r` that are linked to `?c`, in either direction, by a predicate
/// that reaches `?v` through `rdfs:subPropertyOf*`, over every graph. `?c` and `?v` are bound
/// with `substitute_variable`; the text is fixed.
const RELATED: &str = "SELECT DISTINCT ?r ?c ?v WHERE { \
    { ?r ?p ?c } UNION { ?c ?p ?r } \
    ?p <http://www.w3.org/2000/01/rdf-schema#subPropertyOf>* ?v }";

/// The named nodes other than `resource` that a relation of the kind `via` (or one of its
/// sub-properties) links to it, in either direction, in any graph.
pub(crate) fn related(store: &Store, resource: &NamedNode, via: &NamedNode) -> BTreeSet<NamedNode> {
    let mut query = SparqlEvaluator::new()
        .without_default_http_service_handler()
        .parse_query(RELATED)
        .expect("the fixed query parses")
        .substitute_variable(Variable::new_unchecked("c"), resource.clone())
        .substitute_variable(Variable::new_unchecked("v"), via.clone());
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
                .get("r")
            {
                Some(Term::NamedNode(r)) if r != resource => Some(r.clone()),
                _ => None,
            }
        })
        .collect()
}

/// Whether a kind is one of the four standard relations.
fn is_standard(kind: &NamedNode) -> bool {
    STANDARD_RELATIONS.contains(&kind.as_str())
}

/// A kind's record, read from every graph. `all` is every kind.
fn record(
    ctx: &Context,
    store: &Store,
    all: &BTreeSet<NamedNode>,
    kind: &NamedNode,
) -> RelationKind {
    let parents: BTreeSet<String> = store
        .quads_for_pattern(
            Some(NamedOrBlankNodeRef::from(kind.as_ref())),
            Some(named(rdfs::SUB_PROPERTY_OF)),
            None,
            None,
        )
        .filter_map(|q| match q.expect("reading the in-memory store").object {
            Term::NamedNode(n) if all.contains(&n) && n != *kind => Some(id_of(&n)),
            _ => None,
        })
        .collect();
    RelationKind {
        id: id_of(kind),
        label: label_of(ctx, store, kind),
        parents: parents.into_iter().collect(),
        standard: is_standard(kind),
    }
}

/// A kind's label: the smallest `rdfs:label` in the configured language, else its id.
pub(crate) fn label_of(ctx: &Context, store: &Store, kind: &NamedNode) -> String {
    labels(store, kind, ctx.language())
        .into_iter()
        .min()
        .unwrap_or_else(|| id_of(kind))
}

/// The values of a kind's `rdfs:label`s in `language`, from every graph.
fn labels(store: &Store, kind: &NamedNode, language: &str) -> Vec<String> {
    store
        .quads_for_pattern(
            Some(NamedOrBlankNodeRef::from(kind.as_ref())),
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

/// A kind's id: its local name in `hali:`, else `prefix:local` from the fixed prefix table
/// (for a plain local name), else its whole IRI.
pub(crate) fn id_of(kind: &NamedNode) -> String {
    let iri = kind.as_str();
    if let Some(local) = iri.strip_prefix(HALI) {
        return local.to_string();
    }
    for (name, ns) in PREFIXES {
        if let Some(local) = iri.strip_prefix(ns)
            && plain(local)
        {
            return format!("{name}:{local}");
        }
    }
    iri.to_string()
}

/// The local part of a kind's IRI: after its namespace in the prefix table, else after the last
/// `#` or `/`.
fn local(kind: &NamedNode) -> &str {
    let iri = kind.as_str();
    for (_, ns) in PREFIXES {
        if let Some(local) = iri.strip_prefix(ns) {
            return local;
        }
    }
    iri.rsplit(['#', '/']).next().unwrap_or(iri)
}

/// A `prefix:local` reference as an IRI, through the fixed prefix table.
fn expand(reference: &str) -> Option<String> {
    let (prefix, local) = reference.split_once(':')?;
    PREFIXES
        .iter()
        .find(|(name, _)| *name == prefix)
        .map(|(_, ns)| format!("{ns}{local}"))
}

/// Whether a local name can follow `prefix:` in an id: letters, digits, `-` and `_`.
fn plain(local: &str) -> bool {
    !local.is_empty()
        && local
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_'))
}

fn named(iri: &str) -> NamedNodeRef<'_> {
    NamedNodeRef::new_unchecked(iri)
}
