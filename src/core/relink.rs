//! `relink` (spec §6, §9.1 **Relink:**): repairs Resources whose file moved, by searching roots
//! for files with their size and content, or by an explicit `<ref> <path>`. What hali knows of
//! a file comes in through injected functions; `core` reads no file.

use super::resource::{
    Hashed, Record, all_resources, by_path, known, record, recorded_path, recorded_sha256,
    refresh_one, touch_at,
};
use super::{Context, Error, Mutation, apply, validate};
use crate::ontology::{dcat, dcterms};
use crate::paths::file_url;
use crate::storage::walk::Walked;
use crate::storage::{File, Store};
use oxigraph::model::vocab::xsd;
use oxigraph::model::{Literal, NamedNode, NamedNodeRef, Quad};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};

/// At most this many entries go into each list of a search.
const MAX_LISTED: usize = 100;

/// At most this many candidates are listed for an ambiguous Resource.
pub(crate) const MAX_CANDIDATES: usize = 10;

/// How many Resources a search relinked, left ambiguous and left unmatched; always exact.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Counts {
    pub relinked: usize,
    pub ambiguous: usize,
    pub unmatched: usize,
}

/// A relinked Resource: its record after the change, and the path it was recorded at.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Relinked {
    pub resource: Record,
    pub from: String,
}

/// A Missing Resource with matches that are not one-to-one: the files (at most 10, in byte
/// order of the path) that have its size and content.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Ambiguous {
    pub resource: Record,
    pub candidates: Vec<String>,
}

/// The outcome of a search. Each list is in id order and holds at most 100 entries;
/// `truncated` is true when one was cut. `missing` and `stale` of the records are left false.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Searched {
    pub counts: Counts,
    pub relinked: Vec<Relinked>,
    pub ambiguous: Vec<Ambiguous>,
    pub unmatched: Vec<Record>,
    pub truncated: bool,
}

/// The outcome of an explicit relink: the record after the change, the path it was recorded at,
/// and whether its size, mtime and hash were refreshed too.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Explicit {
    pub resource: Record,
    pub from: String,
    pub refreshed: bool,
}

/// What became of one Missing Resource in `match_moves`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The one candidate (by index) that matches it and no other Missing Resource.
    Relinked(usize),
    /// The candidates (by index, ascending) that match it, when that is not one-to-one.
    Ambiguous(Vec<usize>),
    Unmatched,
}

/// Matches Missing Resources to candidate files, each given as (byte size, SHA-256): a
/// Resource is relinked only when exactly one candidate has its content and no other Missing
/// Resource has that candidate's content.
pub(crate) fn match_moves(missing: &[(u64, String)], candidates: &[(u64, String)]) -> Vec<Outcome> {
    let mut by_content: HashMap<(u64, &str), Vec<usize>> = HashMap::new();
    for (j, (size, sha)) in candidates.iter().enumerate() {
        by_content.entry((*size, sha.as_str())).or_default().push(j);
    }
    let mut claims: HashMap<(u64, &str), usize> = HashMap::new();
    for (size, sha) in missing {
        *claims.entry((*size, sha.as_str())).or_default() += 1;
    }
    missing
        .iter()
        .map(|(size, sha)| {
            let key = (*size, sha.as_str());
            match by_content.get(&key).map(Vec::as_slice) {
                None | Some([]) => Outcome::Unmatched,
                Some([one]) if claims[&key] == 1 => Outcome::Relinked(*one),
                Some(many) => Outcome::Ambiguous(many.to_vec()),
            }
        })
        .collect()
}

/// Searches `roots` (real, absolute directories) for the files of Missing Resources and relinks
/// the unique matches, in one mutation with one SHACL check (spec §6). `walk` gives the entries
/// of one root; `stat` the size and mtime of the regular file at a path (`None` is Missing);
/// `hash` reads a file of at most `cap` bytes, reading no more than `cap + 1`, and gives `None`
/// when it cannot. A Missing Resource recorded as empty is never matched; a file is a candidate
/// unless it is empty or some Resource records its path, and it is hashed at most once, only
/// when a Missing Resource has its size.
pub fn search(
    ctx: &Context,
    store: &mut Store,
    roots: &[String],
    mut walk: impl FnMut(&str) -> Vec<Walked>,
    stat: impl Fn(&str) -> Option<(u64, i64)>,
    mut hash: impl FnMut(&str, u64) -> Option<Hashed>,
) -> Result<Mutation<Searched>, Error> {
    let resources = all_resources(store);
    let cataloged: HashSet<String> = resources.iter().map(|r| recorded_path(store, r)).collect();
    // (resource, its recorded path, size and SHA-256), in id order.
    let mut missing: Vec<(NamedNode, String, u64, String)> = Vec::new();
    // Missing, but recorded as empty or without a checksum: never matched, so unmatched.
    let mut unmatchable: Vec<NamedNode> = Vec::new();
    for r in &resources {
        let before = record(ctx, store, r);
        if stat(&before.path).is_some() {
            continue;
        }
        match recorded_sha256(store, r) {
            Some(sha) if before.byte_size > 0 => {
                missing.push((r.clone(), before.path, before.byte_size, sha))
            }
            _ => unmatchable.push(r.clone()),
        }
    }

    // Size first: only files with the size of a Missing Resource are hashed.
    let sizes: HashSet<u64> = missing.iter().map(|m| m.2).collect();
    let mut found: BTreeMap<String, u64> = BTreeMap::new();
    if !sizes.is_empty() {
        for root in roots {
            for entry in walk(root) {
                if let Walked::File {
                    path, byte_size, ..
                } = entry
                    && byte_size > 0
                    && sizes.contains(&byte_size)
                    && !cataloged.contains(&path)
                {
                    found.entry(path).or_insert(byte_size);
                }
            }
        }
    }
    let mut paths: Vec<String> = Vec::new();
    let mut read: Vec<Hashed> = Vec::new();
    for (path, size) in found {
        if let Some(hashed) = hash(&path, size) {
            paths.push(path);
            read.push(hashed);
        }
    }

    let keys: Vec<(u64, String)> = missing.iter().map(|m| (m.2, m.3.clone())).collect();
    let contents: Vec<(u64, String)> = read
        .iter()
        .map(|h| (h.byte_size, h.sha256.clone()))
        .collect();
    let outcomes = match_moves(&keys, &contents);

    let now = ctx.now();
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut relinked: Vec<(NamedNode, String)> = Vec::new();
    let mut ambiguous: Vec<(NamedNode, Vec<String>)> = Vec::new();
    let mut unmatched: Vec<NamedNode> = Vec::new();
    for ((r, from, _, _), outcome) in missing.into_iter().zip(outcomes) {
        match outcome {
            Outcome::Relinked(j) => {
                let (add, remove) = moved(store, &r, &paths[j], &read[j].modified)?;
                added.extend(add);
                removed.extend(remove);
                touch_at(store, &r, &now, &mut added, &mut removed);
                relinked.push((r, from));
            }
            Outcome::Ambiguous(js) => {
                let listed = js
                    .iter()
                    .take(MAX_CANDIDATES)
                    .map(|j| paths[*j].clone())
                    .collect();
                ambiguous.push((r, listed));
            }
            Outcome::Unmatched => unmatched.push(r),
        }
    }
    unmatched.extend(unmatchable);
    unmatched.sort();

    let changes = if added.is_empty() && removed.is_empty() {
        Vec::new()
    } else {
        validate::guard(store, &added, &removed)?;
        apply(store, &added, &removed)
    };
    let counts = Counts {
        relinked: relinked.len(),
        ambiguous: ambiguous.len(),
        unmatched: unmatched.len(),
    };
    let truncated = [counts.relinked, counts.ambiguous, counts.unmatched]
        .iter()
        .any(|n| *n > MAX_LISTED);
    let store: &Store = store;
    Ok(Mutation {
        value: Searched {
            counts,
            relinked: relinked
                .into_iter()
                .take(MAX_LISTED)
                .map(|(r, from)| Relinked {
                    resource: record(ctx, store, &r),
                    from,
                })
                .collect(),
            ambiguous: ambiguous
                .into_iter()
                .take(MAX_LISTED)
                .map(|(r, candidates)| Ambiguous {
                    resource: record(ctx, store, &r),
                    candidates,
                })
                .collect(),
            unmatched: unmatched
                .iter()
                .take(MAX_LISTED)
                .map(|r| record(ctx, store, r))
                .collect(),
            truncated,
        },
        changes,
    })
}

/// Records that the Resource `id` (a bare UUID) is now the file at the real path `path`, which
/// was read as `hashed`, in one mutation with one SHACL check. Any Resource can be relinked,
/// Missing or not. Content other than the record's (size or SHA-256) is `HASH_MISMATCH` unless
/// `force`, which also records the new size and hash (`refreshed`). A path another Resource
/// records is a usage error, forced or not.
pub fn explicit(
    ctx: &Context,
    store: &mut Store,
    id: &str,
    path: &str,
    hashed: &Hashed,
    force: bool,
) -> Result<Mutation<Explicit>, Error> {
    let r = known(store, id)?;
    if let Some(other) = by_path(store, path).filter(|other| *other != r) {
        return Err(Error::usage(&format!(
            "{path} is already the path of resource {}; relink or remove that one first",
            record(ctx, store, &other).id
        )));
    }
    let before = record(ctx, store, &r);
    let expected = recorded_sha256(store, &r).unwrap_or_default();
    let differs = hashed.sha256 != expected || hashed.byte_size != before.byte_size;
    if differs && !force {
        return Err(Error::hash_mismatch(
            &before.id,
            path,
            &expected,
            &hashed.sha256,
        ));
    }
    let (mut added, mut removed) = moved(store, &r, path, &hashed.modified)?;
    if differs {
        let (add, remove) = refresh_one(store, &r, hashed);
        added.extend(add);
        removed.extend(remove);
    }
    let held = |q: &Quad| store.contains(q).expect("reading the in-memory store");
    let changes_anything =
        added.iter().any(|q| !held(q)) || removed.iter().any(|q| !added.contains(q));
    let changes = if changes_anything {
        touch_at(store, &r, &ctx.now(), &mut added, &mut removed);
        validate::guard(store, &added, &removed)?;
        apply(store, &added, &removed)
    } else {
        Vec::new()
    };
    Ok(Mutation {
        value: Explicit {
            resource: record(ctx, store, &r),
            from: before.path,
            refreshed: differs,
        },
        changes,
    })
}

/// The quads that record `resource` at `path` with the mtime `modified`: its `dcat:downloadURL`
/// and `dcterms:modified` replaced.
pub(crate) fn moved(
    store: &Store,
    resource: &NamedNode,
    path: &str,
    modified: &str,
) -> Result<(Vec<Quad>, Vec<Quad>), Error> {
    let graph = File::Resources.graph();
    let url = NamedNode::new(file_url(path))
        .map_err(|_| Error::usage("the path does not make a valid file URL"))?;
    let held = |predicate: &str| -> Vec<Quad> {
        store
            .quads_for_pattern(
                Some(resource.as_ref().into()),
                Some(NamedNodeRef::new_unchecked(predicate)),
                None,
                Some(graph.as_ref().into()),
            )
            .map(|q| q.expect("reading the in-memory store"))
            .collect()
    };
    let mut removed = held(dcat::DOWNLOAD_URL);
    removed.extend(held(dcterms::MODIFIED));
    let added = vec![
        Quad::new(
            resource.clone(),
            NamedNodeRef::new_unchecked(dcat::DOWNLOAD_URL),
            url,
            graph.clone(),
        ),
        Quad::new(
            resource.clone(),
            NamedNodeRef::new_unchecked(dcterms::MODIFIED),
            Literal::new_typed_literal(modified, xsd::DATE_TIME),
            graph,
        ),
    ];
    Ok((added, removed))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(size: u64, sha: &str) -> (u64, String) {
        (size, sha.to_string())
    }

    #[test]
    fn matching_is_one_to_one() {
        // One Resource, one file: relinked.
        assert_eq!(
            match_moves(&[c(6, "a")], &[c(6, "a")]),
            [Outcome::Relinked(0)]
        );
        // One Resource, two files: ambiguous.
        assert_eq!(
            match_moves(&[c(6, "a")], &[c(6, "a"), c(6, "a")]),
            [Outcome::Ambiguous(vec![0, 1])]
        );
        // Two Resources, one file: both ambiguous.
        assert_eq!(
            match_moves(&[c(6, "a"), c(6, "a")], &[c(6, "a")]),
            [Outcome::Ambiguous(vec![0]), Outcome::Ambiguous(vec![0])]
        );
        // The size must match too; no match is unmatched.
        assert_eq!(
            match_moves(&[c(6, "a"), c(7, "b")], &[c(7, "a"), c(7, "b")]),
            [Outcome::Unmatched, Outcome::Relinked(1)]
        );
    }
}
