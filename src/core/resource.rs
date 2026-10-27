//! Resources (spec §2.1), their catalog records and the user Person (§2.5): `add`, `show` and
//! `edit --title`. What hali knows of a file comes in as `FileFacts`; `core` never reads it.

use super::{Context, Error, Mutation, apply, rfc3339, validate};
use crate::ontology::{
    HALI, HALI_MEDIA_TYPES, IANA_MEDIA_TYPES, RESOURCE, dcat, dcterms, foaf, prov, skos, spdx,
};
use crate::paths::{file_url, path_from_file_url};
use crate::storage::{File, Store};
use oxigraph::model::vocab::{rdf, xsd};
use oxigraph::model::{Literal, NamedNode, NamedNodeRef, NamedOrBlankNode, Quad, Term, TermRef};
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

/// Catalogs several files, all or nothing: on an error the store may hold some of them, and
/// must not be saved. A path given twice counts once. The changes are sorted, without
/// duplicates.
pub fn add_all(
    ctx: &Context,
    store: &mut Store,
    files: &[FileFacts],
    title: Option<&str>,
    user_name: &str,
) -> Result<Mutation<AddedAll>, Error> {
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
        let one = add(ctx, store, facts, title, user_name)?;
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

/// Replaces a Resource's title and sets its catalog record's `dcterms:modified` to now. The
/// same title is a no-op with no changes. Refused, with the store unchanged, if the store would
/// not conform to the SHACL shapes.
pub fn edit_title(
    ctx: &Context,
    store: &mut Store,
    id: &str,
    title: &str,
) -> Result<Mutation<Record>, Error> {
    let resource = known(store, id)?;
    let title = normalise_title(title)?;
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

    let titles = quads_of(&resource, dcterms::TITLE);
    let unchanged = matches!(titles.as_slice(),
        [q] if matches!(&q.object, Term::Literal(l) if l.value() == title && l.datatype() == xsd::STRING));
    if unchanged {
        return Ok(Mutation {
            value: record(ctx, store, &resource),
            changes: Vec::new(),
        });
    }

    let mut removed = titles;
    let mut added = vec![Quad::new(
        resource.clone(),
        named(dcterms::TITLE),
        Literal::new_simple_literal(title),
        graph.clone(),
    )];
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
