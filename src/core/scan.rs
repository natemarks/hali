//! `hali scan` (spec §7, §9.1 **Scan:**): registers the new files a walk of a Scan root found,
//! refreshes the Stale Resources under it and reports the missing ones, in one mutation. The
//! walk, `stat` and hashing come in as values and functions; `core` reads no file.

use super::resource::{self, FileFacts, Hashed, MediaPattern, Record};
use super::{Context, Error, Mutation, apply, validate};
use crate::ontology::RESOURCE;
use crate::storage::Store;
use crate::storage::walk::{SkipReason, Walked};
use oxigraph::model::NamedNode;
use oxigraph::model::vocab::rdf;
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};

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

/// How many Resources a scan added, refreshed, left unchanged and found missing, and how many
/// entries it skipped. Always exact.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub added: usize,
    pub refreshed: usize,
    pub unchanged: usize,
    pub missing: usize,
    pub skipped: usize,
}

/// The outcome of a scan. Each list is in byte order of the path and holds at most 100 entries;
/// `truncated` is true when one was cut. The records' `missing` and `stale` are left false.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Scanned {
    pub counts: Counts,
    /// The added Resources by media type.
    pub media_types: BTreeMap<String, usize>,
    pub added: Vec<Record>,
    pub refreshed: Vec<Record>,
    pub missing: Vec<Record>,
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

/// Scans (spec §9.1 **Scan:**). `entries` is the walk of `options.root`; `stat` gives the size
/// and mtime (seconds) of the regular file at a path; `inspect` hashes the file at a real path;
/// `glob_type` gives the media type a file name alone implies, if any; `progress` is told the
/// entries seen and the files hashed so far, the last call with the totals.
///
/// A new file (one whose path no Resource records) is registered as `add` registers it, unless
/// it is over `max_size` (skipped `too_large`, never opened), its name or content rules it out
/// by the media type patterns (dropped, not reported), it vanished (dropped) or it cannot be
/// hashed (skipped). Every Resource under the root is `stat`ed: no regular file there is
/// missing, a Stale one is hashed and refreshed as `refresh` does, the rest are unchanged; the
/// filters never apply to them. New files get ids in byte order of their paths. All changes are
/// one mutation with one SHACL check; on an error the store is untouched.
#[allow(clippy::too_many_arguments)]
pub fn scan(
    ctx: &Context,
    store: &mut Store,
    options: &ScanOptions,
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
    let mut taken: HashSet<String> = HashSet::new();
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
                if known.contains(path.as_str()) || taken.contains(&path) {
                    // Cataloged, or already seen through a symlink.
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
                                taken.insert(path);
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

    new.sort_by(|a, b| a.path.cmp(&b.path));
    let mut person = resource::user(store);
    let mut added: Vec<NamedNode> = Vec::new();
    let mut media_types: BTreeMap<String, usize> = BTreeMap::new();
    for facts in &new {
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
    let counts = Counts {
        added: added.len(),
        refreshed: refreshed.len(),
        unchanged,
        missing: missing.len(),
        skipped: skipped.len(),
    };
    let truncated = [added.len(), refreshed.len(), missing.len(), skipped.len()]
        .iter()
        .any(|&n| n > MAX_LISTED);
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
            missing: records(&missing),
            skipped,
            truncated,
        },
        changes,
    })
}
