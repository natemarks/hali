//! Resources (spec §2.1), their catalog records and the user Person (§2.5): `add`, `show`,
//! `edit` and `find`, with tags (§2.3). What hali knows of a file comes in as `FileFacts`;
//! `core` never reads it.

use super::{Context, Error, Mutation, apply, concept, rfc3339, validate};
use crate::ontology::{
    HALI, HALI_MEDIA_TYPES, IANA_MEDIA_TYPES, RESOURCE, dcat, dcterms, foaf, prov, skos, spdx,
};
use crate::paths::{file_url, path_from_file_url};
use crate::storage::{File, Store};
use oxigraph::model::vocab::{rdf, xsd};
use oxigraph::model::{
    Literal, NamedNode, NamedNodeRef, NamedOrBlankNode, Quad, Term, TermRef, Variable,
};
use oxigraph::sparql::{QueryResults, SparqlEvaluator};
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::Path;

/// What was observed of a file to catalog.
#[derive(Clone, Debug, PartialEq)]
pub struct FileFacts {
    /// The real path, UTF-8, not percent-encoded.
    pub path: String,
    pub byte_size: u64,
    /// The mtime, `YYYY-MM-DDTHH:MM:SSZ`.
    pub modified: String,
    /// Lowercase hex.
    pub sha256: String,
    /// Lowercase `type/subtype`.
    pub media_type: String,
}

/// A Resource record (spec §9.3), as `--json` prints it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Record {
    /// The bare UUID.
    pub id: String,
    pub path: String,
    pub title: String,
    pub media_type: String,
    pub editable: bool,
    pub missing: bool,
    pub stale: bool,
    pub types: Vec<String>,
    pub tags: Vec<Tag>,
    pub byte_size: u64,
    pub modified: String,
}

impl Record {
    /// Sets `missing` and `stale` from what `stat` sees at the record's path now: the size and
    /// mtime (seconds since the Unix epoch) of the regular file there, or `None`.
    pub fn observe(&mut self, seen: Option<(u64, i64)>) {
        match seen {
            None => {
                self.missing = true;
                self.stale = false;
            }
            Some((size, mtime)) => {
                self.missing = false;
                self.stale = size != self.byte_size || rfc3339(mtime) != self.modified;
            }
        }
    }
}

/// A Concept a Resource is tagged with.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Tag {
    pub id: String,
    pub label: String,
}

/// The result of adding one file: its record, and whether it was cataloged already.
#[derive(Clone, Debug, PartialEq)]
pub struct Added {
    pub record: Record,
    pub existing: bool,
}

/// The result of adding several files, each list in argument order.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AddedAll {
    pub added: Vec<Record>,
    pub existing: Vec<Record>,
}

/// A Resource as `show` prints it: the record, its relations and its catalog record.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Shown {
    #[serde(flatten)]
    pub record: Record,
    /// Relations land with #18; always empty until then.
    pub relations: Vec<serde_json::Value>,
    pub catalog_record: Option<CatalogRecord>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CatalogRecord {
    pub id: String,
    pub issued: Option<String>,
    pub modified: Option<String>,
    pub attributed_to: Option<Agent>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Agent {
    pub id: String,
    pub name: Option<String>,
}

/// Trims a title, which must be non-empty and free of control characters.
pub fn normalise_title(title: &str) -> Result<String, Error> {
    if title.chars().any(char::is_control) {
        return Err(Error::usage("a title must not contain control characters"));
    }
    let title = title.trim();
    if title.is_empty() {
        return Err(Error::usage("a title must not be empty"));
    }
    Ok(title.to_string())
}

/// Catalogs a file. If its real path is cataloged already, returns that Resource with no
/// changes (no title change, no refresh). Otherwise mints the Resource, its catalog record and,
/// the first time, the user Person named `user_name`, in that order. Refused, with the store
/// unchanged, if the store would not conform to the SHACL shapes.
pub fn add(
    ctx: &Context,
    store: &mut Store,
    facts: &FileFacts,
    title: Option<&str>,
    user_name: &str,
) -> Result<Mutation<Added>, Error> {
    catalog(ctx, store, facts, title, &[], user_name)
}

/// As `add`, tagging a newly cataloged Resource with the Concepts `tags`.
fn catalog(
    ctx: &Context,
    store: &mut Store,
    facts: &FileFacts,
    title: Option<&str>,
    tags: &[NamedNode],
    user_name: &str,
) -> Result<Mutation<Added>, Error> {
    let title = title.map(normalise_title).transpose()?;
    if let Some(existing) = by_path(store, &facts.path) {
        return Ok(Mutation {
            value: Added {
                record: record(ctx, store, &existing),
                existing: true,
            },
            changes: Vec::new(),
        });
    }

    let resource = urn(&ctx.next_id());
    let catalog_record = urn(&ctx.next_id());
    let (person, new_person) = match user(store) {
        Some(person) => (person, false),
        None => (urn(&ctx.next_id()), true),
    };
    let checksum = NamedNode::new_unchecked(format!("{}#sha256", resource.as_str()));
    let title = title.unwrap_or_else(|| default_title(&facts.path));
    let url = NamedNode::new(file_url(&facts.path))
        .map_err(|_| Error::usage("the path does not make a valid file URL"))?;
    let now = Literal::new_typed_literal(ctx.now(), xsd::DATE_TIME);

    let mut triples: Vec<(&NamedNode, NamedNodeRef<'_>, Term)> = vec![
        (&resource, named(rdf::TYPE.as_str()), iri(RESOURCE)),
        (
            &resource,
            named(dcterms::TITLE),
            Literal::new_simple_literal(title).into(),
        ),
        (&resource, named(dcat::DOWNLOAD_URL), url.into()),
        (
            &resource,
            named(dcat::BYTE_SIZE),
            Literal::new_typed_literal(facts.byte_size.to_string(), xsd::INTEGER).into(),
        ),
        (
            &resource,
            named(dcterms::MODIFIED),
            Literal::new_typed_literal(facts.modified.clone(), xsd::DATE_TIME).into(),
        ),
        (&resource, named(spdx::CHECKSUM), checksum.clone().into()),
        (&checksum, named(spdx::ALGORITHM), iri(spdx::SHA256)),
        (
            &checksum,
            named(spdx::CHECKSUM_VALUE),
            Literal::new_typed_literal(facts.sha256.clone(), xsd::HEX_BINARY).into(),
        ),
        (
            &catalog_record,
            named(rdf::TYPE.as_str()),
            iri(dcat::CATALOG_RECORD),
        ),
        (
            &catalog_record,
            named(foaf::PRIMARY_TOPIC),
            resource.clone().into(),
        ),
        (&catalog_record, named(dcterms::ISSUED), now.clone().into()),
        (&catalog_record, named(dcterms::MODIFIED), now.into()),
        (
            &catalog_record,
            named(prov::WAS_ATTRIBUTED_TO),
            person.clone().into(),
        ),
    ];
    for tag in tags {
        triples.push((&resource, named(dcterms::SUBJECT), tag.clone().into()));
    }
    if new_person {
        triples.push((&person, named(rdf::TYPE.as_str()), iri(prov::PERSON)));
        triples.push((
            &person,
            named(foaf::NAME),
            Literal::new_simple_literal(user_name).into(),
        ));
    }
    let format;
    if is_iana(&facts.media_type) {
        let media = media_iri(IANA_MEDIA_TYPES, &facts.media_type)?;
        triples.push((&resource, named(dcat::MEDIA_TYPE), media.into()));
    } else {
        format = media_iri(HALI_MEDIA_TYPES, &facts.media_type)?;
        triples.push((&resource, named(dcterms::FORMAT), format.clone().into()));
        triples.push((&format, named(rdf::TYPE.as_str()), iri(dcterms::MEDIA_TYPE)));
    }

    let graph = File::Resources.graph();
    let quads: Vec<Quad> = triples
        .into_iter()
        .map(|(s, p, o)| Quad::new(s.clone(), p, o, graph.clone()))
        .collect();
    validate::guard(store, &quads, &[])?;
    let changes = apply(store, &quads, &[]);
    Ok(Mutation {
        value: Added {
            record: record(ctx, store, &resource),
            existing: false,
        },
        changes,
    })
}

/// Catalogs several files, tagging the newly cataloged ones with the Concepts `tags` references,
/// all or nothing: every reference is resolved first, and on a later error the store may hold
/// some of the files, and must not be saved. A path given twice counts once. The changes are
/// sorted, without duplicates.
pub fn add_all(
    ctx: &Context,
    store: &mut Store,
    files: &[FileFacts],
    title: Option<&str>,
    tags: &[String],
    user_name: &str,
) -> Result<Mutation<AddedAll>, Error> {
    let tags = resolve_tags(ctx, store, tags)?;
    let mut seen = BTreeSet::new();
    let mut out = AddedAll {
        added: Vec::new(),
        existing: Vec::new(),
    };
    let mut changes = Vec::new();
    for facts in files {
        if !seen.insert(facts.path.as_str()) {
            continue;
        }
        let one = catalog(ctx, store, facts, title, &tags, user_name)?;
        changes.extend(one.changes);
        if one.value.existing {
            out.existing.push(one.value.record);
        } else {
            out.added.push(one.value.record);
        }
    }
    changes.sort();
    changes.dedup();
    Ok(Mutation {
        value: out,
        changes,
    })
}

/// The id of the Resource a reference names (spec §9.2): a full hyphenated UUID, bare or as a
/// `urn:uuid:` and in any case, else a path, which `path_of` turns into the real path (or the
/// absolute path, if nothing exists there). Prefixes are not accepted.
pub fn resolve_ref(
    store: &Store,
    reference: &str,
    path_of: impl FnOnce(&str) -> Option<String>,
) -> Result<String, Error> {
    let unknown = || Error::unknown_resource(reference);
    let found = match parse_uuid(reference) {
        Some(id) => Some(urn(&id)).filter(|r| is_resource(store, r)),
        None => path_of(reference).and_then(|path| by_path(store, &path)),
    };
    found.map(|r| id_of(&r)).ok_or_else(unknown)
}

/// One Resource with its catalog record.
pub fn show(ctx: &Context, store: &Store, id: &str) -> Result<Shown, Error> {
    let resource = known(store, id)?;
    let catalog_record = catalog_record_of(store, &resource).map(|c| {
        let attributed_to = first_iri(store, &c, prov::WAS_ATTRIBUTED_TO).map(|p| Agent {
            id: id_of(&p),
            name: first_literal(store, &p, foaf::NAME),
        });
        CatalogRecord {
            id: id_of(&c),
            issued: first_literal(store, &c, dcterms::ISSUED),
            modified: first_literal(store, &c, dcterms::MODIFIED),
            attributed_to,
        }
    });
    Ok(Shown {
        record: record(ctx, store, &resource),
        relations: Vec::new(),
        catalog_record,
    })
}

/// What `edit` changes: a new title, and Concept references to tag with or to untag.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Edit {
    pub title: Option<String>,
    pub add_tags: Vec<String>,
    pub remove_tags: Vec<String>,
}

/// Edits a Resource in one mutation: replaces its title, adds and removes tags, and, if that
/// changes anything, sets its catalog record's `dcterms:modified` to now. The same title, a
/// present tag added or an absent one removed changes nothing. A usage error if nothing is
/// asked for, or one Concept is both added and removed. Refused, with the store unchanged, on an
/// unknown Concept reference or if the store would not conform to the SHACL shapes.
pub fn edit(
    ctx: &Context,
    store: &mut Store,
    id: &str,
    edit: &Edit,
) -> Result<Mutation<Record>, Error> {
    if edit.title.is_none() && edit.add_tags.is_empty() && edit.remove_tags.is_empty() {
        return Err(Error::usage(
            "edit needs --title, --add-tag or --remove-tag",
        ));
    }
    let resource = known(store, id)?;
    let title = edit.title.as_deref().map(normalise_title).transpose()?;
    let add_tags = resolve_tags(ctx, store, &edit.add_tags)?;
    let remove_tags = resolve_tags(ctx, store, &edit.remove_tags)?;
    if add_tags.iter().any(|t| remove_tags.contains(t)) {
        return Err(Error::usage("a Concept cannot be both added and removed"));
    }
    let graph = File::Resources.graph();
    let quads_of = |s: &NamedNode, p: &str| -> Vec<Quad> {
        store
            .quads_for_pattern(
                Some(s.as_ref().into()),
                Some(named(p)),
                None,
                Some(graph.as_ref().into()),
            )
            .map(|q| q.expect("reading the in-memory store"))
            .collect()
    };

    let held = |quad: &Quad| store.contains(quad).expect("reading the in-memory store");
    let subject = |tag: &NamedNode| {
        Quad::new(
            resource.clone(),
            named(dcterms::SUBJECT),
            tag.clone(),
            graph.clone(),
        )
    };

    let mut removed: Vec<Quad> = remove_tags.iter().map(subject).filter(held).collect();
    let mut added: Vec<Quad> = add_tags.iter().map(subject).filter(|q| !held(q)).collect();
    if let Some(title) = title {
        let titles = quads_of(&resource, dcterms::TITLE);
        let same = matches!(titles.as_slice(),
            [q] if matches!(&q.object, Term::Literal(l) if l.value() == title && l.datatype() == xsd::STRING));
        if !same {
            removed.extend(titles);
            added.push(Quad::new(
                resource.clone(),
                named(dcterms::TITLE),
                Literal::new_simple_literal(title),
                graph.clone(),
            ));
        }
    }
    if added.is_empty() && removed.is_empty() {
        return Ok(Mutation {
            value: record(ctx, store, &resource),
            changes: Vec::new(),
        });
    }

    if let Some(c) = catalog_record_of(store, &resource) {
        removed.extend(quads_of(&c, dcterms::MODIFIED));
        added.push(Quad::new(
            c,
            named(dcterms::MODIFIED),
            Literal::new_typed_literal(ctx.now(), xsd::DATE_TIME),
            graph.clone(),
        ));
    }
    validate::guard(store, &added, &removed)?;
    let changes = apply(store, &added, &removed);
    Ok(Mutation {
        value: record(ctx, store, &resource),
        changes,
    })
}

/// The order `find` lists Resources in; ties always break on ascending id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sort {
    /// The file's recorded mtime, newest first.
    Modified,
    /// The catalog record's `issued`, newest first; a Resource without one goes last.
    Added,
    /// The title, case-insensitive, ascending.
    Title,
}

/// A media type to match: `type/subtype` or `type/*`, lowercase.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaPattern {
    type_: String,
    /// `None` matches any subtype.
    subtype: Option<String>,
}

/// Parses `type/subtype` or `type/*`, case-insensitively.
pub fn parse_media_pattern(pattern: &str) -> Result<MediaPattern, Error> {
    let bad = || {
        Error::usage(&format!(
            "invalid media type pattern {pattern:?}: use type/subtype or type/*"
        ))
    };
    let token = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || "!#$&^_.+-".contains(c))
    };
    let (type_, subtype) = pattern.split_once('/').ok_or_else(bad)?;
    if !token(type_) {
        return Err(bad());
    }
    let subtype = if subtype == "*" {
        None
    } else if token(subtype) {
        Some(subtype.to_ascii_lowercase())
    } else {
        return Err(bad());
    };
    Ok(MediaPattern {
        type_: type_.to_ascii_lowercase(),
        subtype,
    })
}

impl MediaPattern {
    fn matches(&self, media_type: &str) -> bool {
        match media_type.split_once('/') {
            Some((t, s)) => t == self.type_ && self.subtype.as_deref().is_none_or(|w| w == s),
            None => false,
        }
    }
}

/// What `find` lists: filters (ANDed), sort and page.
#[derive(Clone, Debug, PartialEq)]
pub struct Query {
    /// A case-insensitive substring of the title.
    pub title: Option<String>,
    pub media_type: Option<MediaPattern>,
    /// Only Resources whose file is gone.
    pub missing: bool,
    /// Only Resources whose file changed.
    pub stale: bool,
    /// Concept references: only Resources tagged with each, or with a narrower Concept.
    pub tags: Vec<String>,
    /// Only the asserted tags count, not narrower Concepts.
    pub exact: bool,
    pub sort: Sort,
    pub offset: usize,
    pub limit: usize,
}

impl Default for Query {
    fn default() -> Query {
        Query {
            title: None,
            media_type: None,
            missing: false,
            stale: false,
            tags: Vec::new(),
            exact: false,
            sort: Sort::Modified,
            offset: 0,
            limit: 50,
        }
    }
}

/// One page of `find`; `total` counts the Resources matching the filters.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub results: Vec<Record>,
    pub total: usize,
    pub truncated: bool,
    pub next_offset: Option<usize>,
}

/// The Resources whose `dcterms:subject` reaches `?c` through `skos:broader*`. `?c` is bound
/// with `substitute_variable`; the query text is fixed.
const TAGGED_NARROWER: &str = "SELECT DISTINCT ?r ?c WHERE { \
    ?r <http://purl.org/dc/terms/subject>/<http://www.w3.org/2004/02/skos/core#broader>* ?c }";

/// The Resources whose `dcterms:subject` is `?c`.
const TAGGED_EXACT: &str =
    "SELECT DISTINCT ?r ?c WHERE { ?r <http://purl.org/dc/terms/subject> ?c }";

/// The Resources tagged with `concept` (or, unless `exact`, a narrower Concept), with the
/// `skos:broader` triples of every graph.
fn tagged(store: &Store, concept: &NamedNode, exact: bool) -> BTreeSet<NamedNode> {
    let text = if exact { TAGGED_EXACT } else { TAGGED_NARROWER };
    let mut query = SparqlEvaluator::new()
        .without_default_http_service_handler()
        .parse_query(text)
        .expect("the fixed query parses")
        .substitute_variable(Variable::new_unchecked("c"), concept.clone());
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
                Some(Term::NamedNode(r)) => Some(r.clone()),
                _ => None,
            }
        })
        .collect()
}

/// Lists Resources (spec §6). `stat` gives the size and mtime of the regular file at a path, or
/// `None`; it is called for every candidate when `missing` or `stale` filters, else only for the
/// returned page. Nothing is hashed. An unknown Concept reference in `tags` is
/// `UNKNOWN_CONCEPT`.
pub fn find(
    ctx: &Context,
    store: &Store,
    query: &Query,
    stat: impl Fn(&str) -> Option<(u64, i64)>,
) -> Result<Found, Error> {
    let tags = resolve_tags(ctx, store, &query.tags)?;
    let mut tagged_all: Option<BTreeSet<NamedNode>> = None;
    for tag in &tags {
        let these = tagged(store, tag, query.exact);
        tagged_all = Some(match tagged_all {
            None => these,
            Some(before) => before.intersection(&these).cloned().collect(),
        });
    }
    let title = query.title.as_ref().map(|t| t.to_lowercase());
    let filtered_by_state = query.missing || query.stale;
    // (record, issued)
    let mut candidates: Vec<(Record, Option<String>)> =
        subjects(store, rdf::TYPE.as_str(), iri(RESOURCE).as_ref())
            .iter()
            .filter(|r| tagged_all.as_ref().is_none_or(|t| t.contains(*r)))
            .map(|r| (record(ctx, store, r), r))
            .filter(|(rec, _)| {
                title
                    .as_ref()
                    .is_none_or(|t| rec.title.to_lowercase().contains(t))
                    && query
                        .media_type
                        .as_ref()
                        .is_none_or(|m| m.matches(&rec.media_type))
            })
            .map(|(rec, r)| {
                let issued = (query.sort == Sort::Added)
                    .then(|| catalog_record_of(store, r))
                    .flatten()
                    .and_then(|c| first_literal(store, &c, dcterms::ISSUED));
                (rec, issued)
            })
            .collect();
    if filtered_by_state {
        for (rec, _) in &mut candidates {
            rec.observe(stat(&rec.path));
        }
        candidates
            .retain(|(rec, _)| (!query.missing || rec.missing) && (!query.stale || rec.stale));
    }

    match query.sort {
        Sort::Modified => candidates.sort_by(|a, b| {
            b.0.modified
                .cmp(&a.0.modified)
                .then_with(|| a.0.id.cmp(&b.0.id))
        }),
        Sort::Added => candidates.sort_by(|a, b| {
            // `None` sorts before `Some`, so reversing puts it last.
            b.1.cmp(&a.1).then_with(|| a.0.id.cmp(&b.0.id))
        }),
        Sort::Title => candidates.sort_by(|a, b| {
            a.0.title
                .to_lowercase()
                .cmp(&b.0.title.to_lowercase())
                .then_with(|| a.0.id.cmp(&b.0.id))
        }),
    }

    let total = candidates.len();
    let mut results: Vec<Record> = candidates
        .into_iter()
        .skip(query.offset)
        .take(query.limit)
        .map(|(rec, _)| rec)
        .collect();
    if !filtered_by_state {
        for rec in &mut results {
            rec.observe(stat(&rec.path));
        }
    }
    let end = query.offset.saturating_add(results.len());
    let truncated = end < total;
    Ok(Found {
        results,
        total,
        truncated,
        next_offset: truncated.then_some(end),
    })
}

/// The Concepts that `references` name, in order and without repeats; the first unknown one is
/// `UNKNOWN_CONCEPT`.
fn resolve_tags(
    ctx: &Context,
    store: &Store,
    references: &[String],
) -> Result<Vec<NamedNode>, Error> {
    let mut out: Vec<NamedNode> = Vec::new();
    for reference in references {
        let concept = concept::resolve_iri(ctx, store, reference)?;
        if !out.contains(&concept) {
            out.push(concept);
        }
    }
    Ok(out)
}

/// The record's field names, in record order.
const FIELDS: [&str; 11] = [
    "id",
    "path",
    "title",
    "media_type",
    "editable",
    "missing",
    "stale",
    "types",
    "tags",
    "byte_size",
    "modified",
];

/// Parses a comma-separated `--fields` list into names in record order, without duplicates.
pub fn parse_fields(list: &str) -> Result<Vec<String>, Error> {
    let mut wanted = BTreeSet::new();
    for name in list.split(',') {
        match FIELDS.iter().position(|f| *f == name) {
            Some(i) => {
                wanted.insert(i);
            }
            None => {
                return Err(Error::usage(&format!(
                    "unknown field {name:?}; valid fields: {}",
                    FIELDS.join(", ")
                )));
            }
        }
    }
    Ok(wanted.into_iter().map(|i| FIELDS[i].to_string()).collect())
}

/// The record as a JSON object holding only `fields`, in record order.
pub fn narrow(record: &Record, fields: &[String]) -> serde_json::Value {
    let serde_json::Value::Object(full) = serde_json::to_value(record).expect("records serialize")
    else {
        unreachable!("a record is an object");
    };
    let mut out = serde_json::Map::new();
    for name in FIELDS {
        if fields.iter().any(|f| f == name) {
            out.insert(name.to_string(), full[name].clone());
        }
    }
    serde_json::Value::Object(out)
}

/// The default title: the file name without its (last) extension, control characters replaced
/// by U+FFFD; the whole file name if that leaves nothing.
fn default_title(path: &str) -> String {
    let path = Path::new(path);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let name = if stem.is_empty() {
        path.file_name().and_then(|s| s.to_str()).unwrap_or("")
    } else {
        stem
    };
    name.chars()
        .map(|c| if c.is_control() { '\u{fffd}' } else { c })
        .collect()
}

/// Whether a media type is an IANA one: neither its type nor its subtype starts with `x-`.
fn is_iana(media_type: &str) -> bool {
    !media_type.split('/').any(|part| part.starts_with("x-"))
}

fn media_iri(base: &str, media_type: &str) -> Result<NamedNode, Error> {
    NamedNode::new(format!("{base}{media_type}"))
        .map_err(|_| Error::usage("the media type does not make a valid IRI"))
}

/// A full hyphenated UUID, bare or after `urn:uuid:`, in any case, as a lowercase bare UUID.
fn parse_uuid(reference: &str) -> Option<String> {
    let bare = match reference.get(..9) {
        Some(prefix) if prefix.eq_ignore_ascii_case("urn:uuid:") => &reference[9..],
        _ => reference,
    };
    let well_formed = bare.len() == 36
        && bare.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_hexdigit(),
        });
    well_formed.then(|| bare.to_ascii_lowercase())
}

/// The Resource with this bare id, or `UNKNOWN_RESOURCE`.
fn known(store: &Store, id: &str) -> Result<NamedNode, Error> {
    Some(urn(id))
        .filter(|r| is_resource(store, r))
        .ok_or_else(|| Error::unknown_resource(id))
}

fn is_resource(store: &Store, resource: &NamedNode) -> bool {
    has(store, resource, rdf::TYPE.as_str(), iri(RESOURCE).as_ref())
}

/// The Resource whose `dcat:downloadURL` is the URL of `path`.
fn by_path(store: &Store, path: &str) -> Option<NamedNode> {
    let url = NamedNode::new(file_url(path)).ok()?;
    subjects(store, dcat::DOWNLOAD_URL, url.as_ref().into())
        .into_iter()
        .find(|r| is_resource(store, r))
}

/// The user Person, if one is minted: the first `prov:Person`, by IRI.
fn user(store: &Store) -> Option<NamedNode> {
    subjects(store, rdf::TYPE.as_str(), iri(prov::PERSON).as_ref())
        .into_iter()
        .next()
}

fn catalog_record_of(store: &Store, resource: &NamedNode) -> Option<NamedNode> {
    subjects(store, foaf::PRIMARY_TOPIC, TermRef::from(resource.as_ref()))
        .into_iter()
        .find(|c| {
            has(
                store,
                c,
                rdf::TYPE.as_str(),
                iri(dcat::CATALOG_RECORD).as_ref(),
            )
        })
}

/// The record of a Resource, as the store has it; `missing` and `stale` are left false.
fn record(ctx: &Context, store: &Store, resource: &NamedNode) -> Record {
    let url = first_iri(store, resource, dcat::DOWNLOAD_URL);
    let path = url
        .as_ref()
        .map(|u| path_from_file_url(u.as_str()).unwrap_or_else(|| u.as_str().to_string()))
        .unwrap_or_default();
    let media_type = first_iri(store, resource, dcat::MEDIA_TYPE)
        .and_then(|m| {
            m.as_str()
                .strip_prefix(IANA_MEDIA_TYPES)
                .map(str::to_string)
        })
        .or_else(|| {
            first_iri(store, resource, dcterms::FORMAT).and_then(|m| {
                m.as_str()
                    .strip_prefix(HALI_MEDIA_TYPES)
                    .map(str::to_string)
            })
        })
        .unwrap_or_default();
    let types: BTreeSet<String> = objects(store, resource, rdf::TYPE.as_str())
        .into_iter()
        .filter_map(|t| match t {
            Term::NamedNode(n) if n.as_str() != RESOURCE => Some(id_of(&n)),
            _ => None,
        })
        .collect();
    let mut tags: Vec<Tag> = objects(store, resource, dcterms::SUBJECT)
        .into_iter()
        .filter_map(|t| match t {
            Term::NamedNode(c) => Some(Tag {
                id: id_of(&c),
                label: pref_label(store, &c, ctx.language()).unwrap_or_default(),
            }),
            _ => None,
        })
        .collect();
    tags.sort_by(|a, b| (&a.label, &a.id).cmp(&(&b.label, &b.id)));
    Record {
        id: id_of(resource),
        path,
        title: first_literal(store, resource, dcterms::TITLE).unwrap_or_default(),
        editable: media_type.starts_with("text/"),
        media_type,
        missing: false,
        stale: false,
        types: types.into_iter().collect(),
        tags,
        byte_size: first_literal(store, resource, dcat::BYTE_SIZE)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0),
        modified: first_literal(store, resource, dcterms::MODIFIED).unwrap_or_default(),
    }
}

/// A Concept's prefLabel in `language`, from any graph.
fn pref_label(store: &Store, concept: &NamedNode, language: &str) -> Option<String> {
    store
        .quads_for_pattern(
            Some(concept.as_ref().into()),
            Some(named(skos::PREF_LABEL)),
            None,
            None,
        )
        .filter_map(|q| match q.expect("reading the in-memory store").object {
            Term::Literal(l) if l.language() == Some(language) => Some(l.value().to_string()),
            _ => None,
        })
        .min()
}

/// The objects of a subject's predicate in resources.ttl, sorted.
fn objects(store: &Store, subject: &NamedNode, predicate: &str) -> Vec<Term> {
    let graph = File::Resources.graph();
    let mut out: Vec<Term> = store
        .quads_for_pattern(
            Some(subject.as_ref().into()),
            Some(named(predicate)),
            None,
            Some(graph.as_ref().into()),
        )
        .map(|q| q.expect("reading the in-memory store").object)
        .collect();
    out.sort_by_cached_key(ToString::to_string);
    out
}

fn first_iri(store: &Store, subject: &NamedNode, predicate: &str) -> Option<NamedNode> {
    objects(store, subject, predicate)
        .into_iter()
        .find_map(|t| match t {
            Term::NamedNode(n) => Some(n),
            _ => None,
        })
}

fn first_literal(store: &Store, subject: &NamedNode, predicate: &str) -> Option<String> {
    objects(store, subject, predicate)
        .into_iter()
        .find_map(|t| match t {
            Term::Literal(l) => Some(l.value().to_string()),
            _ => None,
        })
}

/// The named subjects with `predicate object` in resources.ttl, sorted by IRI.
fn subjects(store: &Store, predicate: &str, object: TermRef<'_>) -> Vec<NamedNode> {
    let graph = File::Resources.graph();
    let found: BTreeSet<String> = store
        .quads_for_pattern(
            None,
            Some(named(predicate)),
            Some(object),
            Some(graph.as_ref().into()),
        )
        .filter_map(|q| match q.expect("reading the in-memory store").subject {
            NamedOrBlankNode::NamedNode(n) => Some(n.into_string()),
            _ => None,
        })
        .collect();
    found.into_iter().map(NamedNode::new_unchecked).collect()
}

fn has(store: &Store, subject: &NamedNode, predicate: &str, object: TermRef<'_>) -> bool {
    let graph = File::Resources.graph();
    store
        .quads_for_pattern(
            Some(subject.as_ref().into()),
            Some(named(predicate)),
            Some(object),
            Some(graph.as_ref().into()),
        )
        .next()
        .is_some()
}

fn urn(id: &str) -> NamedNode {
    NamedNode::new_unchecked(format!("urn:uuid:{id}"))
}

/// An id as `--json` prints it: a `urn:uuid:` without the prefix, a `hali:` term by its local
/// name, anything else as the whole IRI.
fn id_of(node: &NamedNode) -> String {
    let iri = node.as_str();
    iri.strip_prefix("urn:uuid:")
        .or_else(|| iri.strip_prefix(HALI))
        .unwrap_or(iri)
        .to_string()
}

fn named(iri: &str) -> NamedNodeRef<'_> {
    NamedNodeRef::new_unchecked(iri)
}

fn iri(iri: &str) -> Term {
    NamedNode::new_unchecked(iri).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuids_parse_in_full_only() {
        let id = "018bcfe5-6801-710a-adec-89025cc1beeb";
        assert_eq!(parse_uuid(id).as_deref(), Some(id));
        assert_eq!(
            parse_uuid(&format!("URN:UUID:{}", id.to_uppercase())).as_deref(),
            Some(id)
        );
        for bad in [
            "018bcfe5",
            "018bcfe5-6801",
            &id[..35],
            "018bcfe56801710aadec89025cc1beeb",
        ] {
            assert_eq!(parse_uuid(bad), None, "{bad}");
        }
    }

    #[test]
    fn default_titles() {
        assert_eq!(default_title("/a/archive.tar.gz"), "archive.tar");
        assert_eq!(default_title("/a/.bashrc"), ".bashrc");
        assert_eq!(default_title("/a/a\u{1}b.txt"), "a\u{fffd}b");
    }

    #[test]
    fn titles_are_trimmed_and_checked() {
        assert_eq!(normalise_title("  Field Notes ").unwrap(), "Field Notes");
        for bad in ["", "   ", "a\u{7}b", "tab\there"] {
            assert_eq!(normalise_title(bad).unwrap_err().exit_code(), 2, "{bad:?}");
        }
    }

    #[test]
    fn x_types_are_not_iana() {
        assert!(is_iana("text/plain"));
        assert!(!is_iana("application/x-shellscript"));
        assert!(!is_iana("x-epoc/x-sisx-app"));
    }

    #[test]
    fn observe_sets_missing_and_stale() {
        let mut r = Record {
            id: String::new(),
            path: String::new(),
            title: String::new(),
            media_type: String::new(),
            editable: false,
            missing: false,
            stale: false,
            types: Vec::new(),
            tags: Vec::new(),
            byte_size: 6,
            modified: "2026-10-20T14:02:11Z".to_string(),
        };
        r.observe(Some((6, 1_792_504_931)));
        assert!(!r.missing && !r.stale);
        r.observe(Some((7, 1_792_504_931)));
        assert!(r.stale);
        r.observe(Some((6, 1_792_504_932)));
        assert!(r.stale);
        r.observe(None);
        assert!(r.missing && !r.stale);
    }
}
