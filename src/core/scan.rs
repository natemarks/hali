//! `hali scan` (spec §7, §9.1 **Scan:**): registers the new files a walk of a Scan root found,
//! refreshes the Stale Resources under it, relinks the ones that provably moved, reports the
//! missing ones and, with `--prune`, forgets them, in one mutation. The walk, `stat` and hashing
//! come in as values and functions; `core` reads no file.

use super::relink::{self, Ambiguous, MAX_CANDIDATES, Outcome, Relinked};
use super::resource::{self, FileFacts, Hashed, MediaPattern, Record};
use super::{Context, Error, Mutation, apply, validate};
use crate::ontology::RESOURCE;
use crate::storage::Store;
use crate::storage::walk::{SkipReason, Walked};
use oxigraph::model::NamedNode;
use oxigraph::model::vocab::rdf;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashSet};

/// At most this many entries go into each list of the output; the counts stay exact.
const MAX_LISTED: usize = 100;

/// What to scan, and which new files to register.
#[derive(Clone, Debug, PartialEq)]
pub struct ScanOptions {
    /// The real, absolute path of the Scan root, without a trailing slash.
    pub root: String,
    /// The largest new file registered, in bytes.
    pub max_size: u64,
    /// New files must match one of these (ORed); empty means no filter.
    pub media_types: Vec<MediaPattern>,
}

/// Whether a scan forgets the Missing Resources under its root (`--prune [--force]`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prune {
    /// Report them only.
    Off,
    /// Forget them, except those another Resource relates to.
    On,
    /// Forget them all, with the relations other Resources hold to them in resources.ttl.
    Forced,
}

/// Why a file could not be hashed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unhashable {
    /// Nothing is there any more.
    Missing,
    /// Something other than a regular file is there now.
    NotRegular,
    /// It cannot be opened or read.
    Unreadable,
}

/// An entry the scan did not register or refresh, and why.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Skipped {
    pub path: String,
    /// `too_large`, `unreadable`, `not_regular`, `non_utf8`, `outside_root` or
    /// `bad_ignore_file`.
    pub reason: &'static str,
}

/// How many Resources a scan added, refreshed, relinked, left ambiguous, left unchanged, found
/// still missing and pruned, and how many entries it skipped. Always exact.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub added: usize,
    pub refreshed: usize,
    pub relinked: usize,
    pub ambiguous: usize,
    pub unchanged: usize,
    pub missing: usize,
    pub pruned: usize,
    pub skipped: usize,
}

/// The outcome of a scan. Each list is in byte order of the path (the new path of a relinked
/// Resource) and holds at most 100 entries; `truncated` is true when one was cut. The records'
/// `missing` and `stale` are left false; the pruned ones are as they were.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Scanned {
    pub counts: Counts,
    /// The added Resources by media type.
    pub media_types: BTreeMap<String, usize>,
    pub added: Vec<Record>,
    pub refreshed: Vec<Record>,
    pub relinked: Vec<Relinked>,
    pub ambiguous: Vec<Ambiguous>,
    /// The Resources under the root still cataloged and Missing after the scan.
    pub missing: Vec<Record>,
    pub pruned: Vec<Record>,
    pub skipped: Vec<Skipped>,
    pub truncated: bool,
}

fn reason(reason: SkipReason) -> &'static str {
    match reason {
        SkipReason::TooLarge => "too_large",
        SkipReason::Unreadable => "unreadable",
        SkipReason::NotRegular => "not_regular",
        SkipReason::NonUtf8 => "non_utf8",
        SkipReason::OutsideRoot => "outside_root",
        SkipReason::BadIgnoreFile => "bad_ignore_file",
    }
}

/// Whether a recorded path is under the Scan root `root`: the root itself, or `<root>/` followed
/// by segments none of which is empty, `.` or `..`.
pub fn under_root(root: &str, path: &str) -> bool {
    if path == root {
        return true;
    }
    let prefix = if root.ends_with('/') {
        root.to_string()
    } else {
        format!("{root}/")
    };
    path.strip_prefix(&prefix).is_some_and(|rest| {
        rest.split('/')
            .all(|segment| !matches!(segment, "" | "." | ".."))
    })
}

/// Scans without pruning: `scan_with(…, Prune::Off, …)`.
#[allow(clippy::too_many_arguments)]
pub fn scan(
    ctx: &Context,
    store: &mut Store,
    options: &ScanOptions,
    entries: impl IntoIterator<Item = Walked>,
    stat: impl Fn(&str) -> Option<(u64, i64)>,
    inspect: impl FnMut(&str) -> Result<FileFacts, Unhashable>,
    glob_type: impl Fn(&str) -> Option<String>,
    progress: impl FnMut(usize, usize),
    user_name: &str,
) -> Result<Mutation<Scanned>, Error> {
    scan_with(
        ctx,
        store,
        options,
        Prune::Off,
        entries,
        stat,
        inspect,
        glob_type,
        progress,
        user_name,
    )
}

/// Scans (spec §9.1 **Scan:**). `entries` is the walk of `options.root`; `stat` gives the size
/// and mtime (seconds) of the regular file at a path; `inspect` hashes the file at a real path;
/// `glob_type` gives the media type a file name alone implies, if any; `progress` is told the
/// entries seen and the files hashed so far, the last call with the totals.
///
/// A new file (one whose path no Resource records) is a candidate unless it is over `max_size`
/// (skipped `too_large`, never opened), its name or content rules it out by the media type
/// patterns (dropped, not reported), it vanished (dropped) or it cannot be hashed (skipped).
/// Every Resource under the root is `stat`ed: no regular file there is missing, a Stale one is
/// hashed and refreshed as `refresh` does, the rest are unchanged; the filters never apply to
/// them. A non-empty candidate with the size and SHA-256 of exactly one Missing Resource
/// (store-wide; one outside the root is `stat`ed only then), and no other Missing Resource
/// having its content, relinks that Resource as `relink` does; when the match is not
/// one-to-one, the Resources are ambiguous and their candidates are not registered. With
/// `prune`, the Missing Resources under the root that are not ambiguous are forgotten as
/// `forget` forgets them, unless another Resource relates to one (`Prune::On`). The other
/// candidates are registered, ids minted in byte order of their paths. All changes are one
/// mutation with one SHACL check; on an error the store is untouched.
#[allow(clippy::too_many_arguments)]
pub fn scan_with(
    ctx: &Context,
    store: &mut Store,
    options: &ScanOptions,
    prune: Prune,
    entries: impl IntoIterator<Item = Walked>,
    stat: impl Fn(&str) -> Option<(u64, i64)>,
    mut inspect: impl FnMut(&str) -> Result<FileFacts, Unhashable>,
    glob_type: impl Fn(&str) -> Option<String>,
    mut progress: impl FnMut(usize, usize),
    user_name: &str,
) -> Result<Mutation<Scanned>, Error> {
    let ruled_out = |media_type: &str| {
        !options.media_types.is_empty()
            && !options.media_types.iter().any(|p| p.matches(media_type))
    };
    let resources = resource::subjects(store, rdf::TYPE.as_str(), resource::iri(RESOURCE).as_ref());
    let mut cataloged: Vec<(String, NamedNode)> = resources
        .into_iter()
        .map(|r| (resource::recorded_path(store, &r), r))
        .collect();
    let known: HashSet<&str> = cataloged.iter().map(|(p, _)| p.as_str()).collect();

    let (mut seen, mut hashed) = (0, 0);
    let mut skipped: Vec<Skipped> = Vec::new();
    let mut new: Vec<FileFacts> = Vec::new();
    for entry in entries {
        seen += 1;
        match entry {
            Walked::Skipped { path, reason: why } => skipped.push(Skipped {
                path,
                reason: reason(why),
            }),
            Walked::File {
                path, byte_size, ..
            } => {
                if known.contains(path.as_str()) {
                    // Cataloged.
                } else if byte_size > options.max_size {
                    skipped.push(Skipped {
                        path,
                        reason: reason(SkipReason::TooLarge),
                    });
                } else if !options.media_types.is_empty()
                    && glob_type(path.rsplit('/').next().unwrap_or(&path))
                        .is_some_and(|t| ruled_out(&t))
                {
                    // The name alone rules it out: not hashed.
                } else {
                    hashed += 1;
                    match inspect(&path) {
                        Ok(facts) => {
                            if !ruled_out(&facts.media_type) {
                                new.push(facts);
                            }
                        }
                        Err(Unhashable::Missing) => {}
                        Err(Unhashable::NotRegular) => skipped.push(Skipped {
                            path,
                            reason: reason(SkipReason::NotRegular),
                        }),
                        Err(Unhashable::Unreadable) => skipped.push(Skipped {
                            path,
                            reason: reason(SkipReason::Unreadable),
                        }),
                    }
                }
            }
        }
        progress(seen, hashed);
    }
    drop(known);

    let now = ctx.now();
    let mut added_quads = Vec::new();
    let mut removed_quads = Vec::new();
    let mut refreshed: Vec<NamedNode> = Vec::new();
    // Under the root, in byte order of the path.
    let mut missing: Vec<NamedNode> = Vec::new();
    let mut unchanged = 0;
    cataloged.retain(|(path, _)| under_root(&options.root, path));
    cataloged.sort();
    for (path, r) in &cataloged {
        let Some(stats) = stat(path) else {
            missing.push(r.clone());
            continue;
        };
        let mut observed = resource::record(ctx, store, r);
        observed.observe(Some(stats));
        if !observed.stale {
            unchanged += 1;
            continue;
        }
        hashed += 1;
        let facts = inspect(path);
        progress(seen, hashed);
        let facts = match facts {
            Ok(facts) => facts,
            Err(Unhashable::Missing) => {
                missing.push(r.clone());
                continue;
            }
            Err(why) => {
                let why = if why == Unhashable::NotRegular {
                    SkipReason::NotRegular
                } else {
                    SkipReason::Unreadable
                };
                skipped.push(Skipped {
                    path: path.clone(),
                    reason: reason(why),
                });
                continue;
            }
        };
        let now_hashed = Hashed {
            byte_size: facts.byte_size,
            modified: facts.modified,
            sha256: facts.sha256,
        };
        let (mut add, mut remove) = resource::refresh_one(store, r, &now_hashed);
        if add.is_empty() && remove.is_empty() {
            unchanged += 1;
        } else {
            resource::touch_at(store, r, &now, &mut add, &mut remove);
            added_quads.extend(add);
            removed_quads.extend(remove);
            refreshed.push(r.clone());
        }
    }
    progress(seen, hashed);

    // Moves: the non-empty new files against the Missing Resources with their content.
    new.sort_by(|a, b| a.path.cmp(&b.path));
    let candidates: Vec<usize> = (0..new.len()).filter(|&i| new[i].byte_size > 0).collect();
    let contents: BTreeSet<(u64, &str)> = candidates
        .iter()
        .map(|&i| (new[i].byte_size, new[i].sha256.as_str()))
        .collect();
    let under_missing: HashSet<&NamedNode> = missing.iter().collect();
    // (path, resource, size, SHA-256) of each Missing Resource that a candidate may match.
    let mut pool: Vec<(String, NamedNode, u64, String)> = Vec::new();
    let mut looked: HashSet<NamedNode> = HashSet::new();
    for (size, sha) in contents {
        for r in resource::with_content(store, size, sha) {
            if !looked.insert(r.clone()) {
                continue;
            }
            let path = resource::recorded_path(store, &r);
            let is_missing = if under_root(&options.root, &path) {
                under_missing.contains(&r)
            } else {
                stat(&path).is_none()
            };
            if is_missing {
                pool.push((path, r, size, sha.to_string()));
            }
        }
    }
    drop(under_missing);
    pool.sort();
    let keys: Vec<(u64, String)> = pool.iter().map(|m| (m.2, m.3.clone())).collect();
    let found: Vec<(u64, String)> = candidates
        .iter()
        .map(|&i| (new[i].byte_size, new[i].sha256.clone()))
        .collect();
    let mut taken: HashSet<usize> = HashSet::new();
    // (new path, resource, old path)
    let mut relinked: Vec<(String, NamedNode, String)> = Vec::new();
    let mut ambiguous: Vec<(NamedNode, Vec<String>)> = Vec::new();
    for ((from, r, _, _), outcome) in pool.into_iter().zip(relink::match_moves(&keys, &found)) {
        match outcome {
            Outcome::Relinked(j) => {
                let facts = &new[candidates[j]];
                let (mut add, mut remove) = relink::moved(store, &r, &facts.path, &facts.modified)?;
                resource::touch_at(store, &r, &now, &mut add, &mut remove);
                added_quads.extend(add);
                removed_quads.extend(remove);
                taken.insert(candidates[j]);
                relinked.push((facts.path.clone(), r, from));
            }
            Outcome::Ambiguous(js) => {
                taken.extend(js.iter().map(|&j| candidates[j]));
                let listed = js
                    .iter()
                    .take(MAX_CANDIDATES)
                    .map(|&j| new[candidates[j]].path.clone())
                    .collect();
                ambiguous.push((r, listed));
            }
            Outcome::Unmatched => {}
        }
    }
    let moved_away: HashSet<&NamedNode> = relinked.iter().map(|(_, r, _)| r).collect();
    missing.retain(|r| !moved_away.contains(r));
    drop(moved_away);

    // Prune: the Missing Resources under the root that are not ambiguous; relations are read
    // from the store as it was, so one from another pruned Resource still keeps a Resource.
    let unsure: HashSet<&NamedNode> = ambiguous.iter().map(|(r, _)| r).collect();
    let pruned: Vec<NamedNode> = missing
        .iter()
        .filter(|r| match prune {
            Prune::Off => false,
            Prune::On => !unsure.contains(r) && resource::inbound(store, r).is_empty(),
            Prune::Forced => !unsure.contains(r),
        })
        .cloned()
        .collect();
    drop(unsure);
    let leaving: HashSet<NamedNode> = pruned.iter().cloned().collect();
    missing.retain(|r| !leaving.contains(r));
    let pruned_records: Vec<Record> = pruned
        .iter()
        .take(MAX_LISTED)
        .map(|r| resource::record(ctx, store, r))
        .collect();
    for r in &pruned {
        removed_quads.extend(resource::forget_quads(store, r, &leaving));
    }

    let mut person = resource::user(store);
    let mut added: Vec<NamedNode> = Vec::new();
    let mut media_types: BTreeMap<String, usize> = BTreeMap::new();
    for (i, facts) in new.iter().enumerate() {
        if taken.contains(&i) {
            continue;
        }
        let (r, quads) =
            resource::catalog_quads(ctx, facts, None, &[], &[], &mut person, user_name, &now)?;
        added_quads.extend(quads);
        added.push(r);
        *media_types.entry(facts.media_type.clone()).or_default() += 1;
    }

    let changes = if added_quads.is_empty() && removed_quads.is_empty() {
        Vec::new()
    } else {
        validate::guard(store, &added_quads, &removed_quads)?;
        apply(store, &added_quads, &removed_quads)
    };

    skipped.sort_by(|a, b| a.path.cmp(&b.path));
    relinked.sort();
    ambiguous.sort_by_cached_key(|(r, _)| (resource::recorded_path(store, r), r.clone()));
    let counts = Counts {
        added: added.len(),
        refreshed: refreshed.len(),
        relinked: relinked.len(),
        ambiguous: ambiguous.len(),
        unchanged,
        missing: missing.len(),
        pruned: pruned.len(),
        skipped: skipped.len(),
    };
    let truncated = [
        counts.added,
        counts.refreshed,
        counts.relinked,
        counts.ambiguous,
        counts.missing,
        counts.pruned,
        counts.skipped,
    ]
    .iter()
    .any(|&n| n > MAX_LISTED);
    let store: &Store = store;
    let records = |list: &[NamedNode]| -> Vec<Record> {
        list.iter()
            .take(MAX_LISTED)
            .map(|r| resource::record(ctx, store, r))
            .collect()
    };
    skipped.truncate(MAX_LISTED);
    Ok(Mutation {
        value: Scanned {
            counts,
            media_types,
            added: records(&added),
            refreshed: records(&refreshed),
            relinked: relinked
                .into_iter()
                .take(MAX_LISTED)
                .map(|(_, r, from)| Relinked {
                    resource: resource::record(ctx, store, &r),
                    from,
                })
                .collect(),
            ambiguous: ambiguous
                .into_iter()
                .take(MAX_LISTED)
                .map(|(r, candidates)| Ambiguous {
                    resource: resource::record(ctx, store, &r),
                    candidates,
                })
                .collect(),
            missing: records(&missing),
            pruned: pruned_records,
            skipped,
            truncated,
        },
        changes,
    })
}
