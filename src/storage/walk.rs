//! The directory walker behind `hali scan` (spec §7), on the `ignore` crate. It only reads, it is
//! sequential, and it is the only place hali names that crate.
//!
//! The walk yields every regular, UTF-8 file it keeps, and every entry it has to skip with a
//! reason; a skip never stops it. Directory symlinks are never followed, so there are no cycles.

use ignore::overrides::{Override, OverrideBuilder};
use ignore::{DirEntry, WalkBuilder};
use std::collections::{HashSet, VecDeque};
use std::fs;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

/// Directories never entered, at any depth, whatever the options.
const BUILTIN_EXCLUDES: [&str; 5] = [".git", "node_modules", "target", "__pycache__", "venv"];

/// The custom ignore file, in gitignore syntax.
const HALIIGNORE: &str = ".haliignore";

/// How to walk. Build it with [`WalkOptions::new`], which validates `excludes`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WalkOptions {
    /// Walk names starting with `.` too.
    pub include_hidden: bool,
    /// Turn off the five ignore-file sources (`.gitignore`, `.git/info/exclude`, the global
    /// gitignore, `.ignore`, `.haliignore`), and nothing else.
    pub no_ignore: bool,
    /// User excludes, in gitignore syntax relative to the root.
    pub excludes: Vec<String>,
    /// Absolute paths never entered (the data dir).
    pub prune: Vec<PathBuf>,
}

impl WalkOptions {
    /// Returns the first pattern in `excludes` that is not a valid glob, or starts with `!`.
    pub fn new(
        include_hidden: bool,
        no_ignore: bool,
        excludes: Vec<String>,
        prune: Vec<PathBuf>,
    ) -> Result<WalkOptions, String> {
        overrides(Path::new("/"), &excludes)?;
        Ok(WalkOptions {
            include_hidden,
            no_ignore,
            excludes,
            prune,
        })
    }
}

/// One entry of a walk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Walked {
    /// A regular file, at its real path.
    File {
        path: String,
        byte_size: u64,
        mtime: SystemTime,
    },
    /// An entry not walked. `path` is lossy when it is not UTF-8; for a symlink it is the
    /// symlink's own path, and for `BadIgnoreFile` the ignore file's.
    Skipped { path: String, reason: SkipReason },
}

/// Why an entry is skipped (spec §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// Over `max_size`; the walk itself never produces it.
    TooLarge,
    Unreadable,
    /// A FIFO, socket, device or broken symlink.
    NotRegular,
    NonUtf8,
    /// A file symlink that resolves outside the root.
    OutsideRoot,
    /// An ignore file with an invalid line; its valid lines still apply.
    BadIgnoreFile,
}

/// The user excludes as `ignore` overrides (each pattern negated, so a match is ignored).
fn overrides(root: &Path, excludes: &[String]) -> Result<Override, String> {
    let mut builder = OverrideBuilder::new(root);
    for pattern in excludes {
        if pattern.starts_with('!') {
            return Err(pattern.clone());
        }
        builder
            .add(&format!("!{pattern}"))
            .map_err(|_| pattern.clone())?;
    }
    builder.build().map_err(|_| {
        // Only the compiled set as a whole failed (it grew too large): name the first pattern
        // that fails alone, else the last one.
        excludes
            .iter()
            .find(|p| overrides(root, std::slice::from_ref(p)).is_err())
            .or(excludes.last())
            .cloned()
            .unwrap_or_default()
    })
}

/// Walks `root`, an absolute, canonical directory: depth first, each directory's entries sorted
/// by file name (byte order). The root itself is always walked, even if hidden or excluded.
///
/// Panics if `opts.excludes` holds a pattern that `WalkOptions::new` rejects.
pub fn walk(root: &Path, opts: &WalkOptions) -> impl Iterator<Item = Walked> + use<> {
    let use_ignore_files = !opts.no_ignore;
    let mut builder = WalkBuilder::new(root);
    builder
        .follow_links(false)
        .hidden(!opts.include_hidden)
        .git_ignore(use_ignore_files)
        .git_exclude(use_ignore_files)
        .git_global(use_ignore_files)
        .ignore(use_ignore_files)
        .parents(true)
        .require_git(true)
        .sort_by_file_name(|a, b| a.cmp(b))
        .overrides(
            overrides(root, &opts.excludes).expect("excludes are validated by WalkOptions::new"),
        );
    if use_ignore_files {
        builder.add_custom_ignore_filename(HALIIGNORE);
    }

    // The data dir as given and as its real path.
    let mut prune = opts.prune.clone();
    prune.extend(opts.prune.iter().filter_map(|p| p.canonicalize().ok()));
    let prune: Arc<[PathBuf]> = prune.into();

    // `filter_entry` cannot yield, so it hands non-UTF-8 names over to the iterator.
    let non_utf8 = Arc::new(Mutex::new(Vec::new()));
    let (filter_prune, filter_non_utf8) = (Arc::clone(&prune), Arc::clone(&non_utf8));
    builder.filter_entry(move |ent| {
        let is_dir = ent.file_type().is_some_and(|t| t.is_dir());
        if is_dir && BUILTIN_EXCLUDES.iter().any(|b| ent.file_name() == *b) {
            return false;
        }
        if filter_prune.iter().any(|p| ent.path() == p) {
            return false;
        }
        if ent.file_name().to_str().is_none() {
            lock(&filter_non_utf8).push(ent.path().to_path_buf());
            return false;
        }
        true
    });

    Walk {
        inner: builder.build(),
        root: root.canonicalize().unwrap_or_else(|_| root.to_path_buf()),
        prune,
        non_utf8,
        queue: VecDeque::new(),
        yielded: HashSet::new(),
        bad_ignore_files: HashSet::new(),
    }
}

fn lock(names: &Mutex<Vec<PathBuf>>) -> std::sync::MutexGuard<'_, Vec<PathBuf>> {
    names.lock().unwrap_or_else(|e| e.into_inner())
}

struct Walk {
    inner: ignore::Walk,
    root: PathBuf,
    prune: Arc<[PathBuf]>,
    non_utf8: Arc<Mutex<Vec<PathBuf>>>,
    queue: VecDeque<Walked>,
    /// The real paths yielded so far, so a file reached through a symlink is yielded once.
    yielded: HashSet<PathBuf>,
    bad_ignore_files: HashSet<PathBuf>,
}

impl Iterator for Walk {
    type Item = Walked;

    fn next(&mut self) -> Option<Walked> {
        loop {
            if let Some(walked) = self.queue.pop_front() {
                return Some(walked);
            }
            let item = self.inner.next();
            // Names the filter met during that step come before the entry it returned.
            let names: Vec<PathBuf> = lock(&self.non_utf8).drain(..).collect();
            for path in names {
                self.skip(&path, SkipReason::NonUtf8);
            }
            match item {
                Some(Ok(ent)) => self.entry(&ent),
                Some(Err(err)) => self.error(&err),
                None if self.queue.is_empty() => return None,
                None => {}
            }
        }
    }
}

impl Walk {
    fn skip(&mut self, path: &Path, reason: SkipReason) {
        self.queue.push_back(Walked::Skipped {
            path: path.to_string_lossy().into_owned(),
            reason,
        });
    }

    fn entry(&mut self, ent: &DirEntry) {
        // Errors from reading a directory's ignore files ride on its entry.
        if let Some(err) = ent.error() {
            self.bad_ignore_file(err, ent.path());
        }
        let (path, Some(file_type)) = (ent.path(), ent.file_type()) else {
            return;
        };
        if file_type.is_dir() {
            return;
        }
        if path.to_str().is_none() {
            self.skip(path, SkipReason::NonUtf8);
        } else if file_type.is_symlink() {
            self.symlink(path);
        } else if !file_type.is_file() {
            self.skip(path, SkipReason::NotRegular);
        } else {
            match ent.metadata() {
                Ok(meta) => self.file(path, &meta, path),
                Err(_) => self.skip(path, SkipReason::Unreadable),
            }
        }
    }

    /// Resolves a symlink without opening anything: a directory is ignored silently, a file is
    /// yielded at its real path if that is inside the root.
    fn symlink(&mut self, link: &Path) {
        let Ok(real) = fs::canonicalize(link) else {
            return self.skip(link, SkipReason::NotRegular);
        };
        let Ok(meta) = fs::metadata(&real) else {
            return self.skip(link, SkipReason::NotRegular);
        };
        if meta.is_dir() {
            return;
        }
        let Ok(inside) = real.strip_prefix(&self.root) else {
            return self.skip(link, SkipReason::OutsideRoot);
        };
        if !meta.is_file() {
            return self.skip(link, SkipReason::NotRegular);
        }
        // A file in a directory the walk never enters is not reached through a link either.
        let in_excluded_dir = inside.parent().is_some_and(|dirs| {
            dirs.iter()
                .any(|name| BUILTIN_EXCLUDES.iter().any(|b| name == *b))
        });
        if in_excluded_dir || self.prune.iter().any(|p| real.starts_with(p)) {
            return;
        }
        if real.to_str().is_none() {
            return self.skip(link, SkipReason::NonUtf8);
        }
        self.file(&real, &meta, link);
    }

    /// Yields the regular file at the real path `real`, once; skips are reported at `report`.
    fn file(&mut self, real: &Path, meta: &fs::Metadata, report: &Path) {
        if self.yielded.contains(real) {
            return;
        }
        let Some(mtime) = meta.modified().ok().filter(|_| readable(real)) else {
            return self.skip(report, SkipReason::Unreadable);
        };
        self.yielded.insert(real.to_path_buf());
        self.queue.push_back(Walked::File {
            path: real.to_string_lossy().into_owned(),
            byte_size: meta.len(),
            mtime,
        });
    }

    /// An error the walk itself returns: a directory it could not read, or an ignore file in
    /// one of the root's parents.
    fn error(&mut self, err: &ignore::Error) {
        if is_walk_error(err) {
            let path = error_path(err).unwrap_or(&self.root).to_path_buf();
            self.skip(&path, SkipReason::Unreadable);
        } else {
            let root = self.root.clone();
            self.bad_ignore_file(err, &root);
        }
    }

    /// Reports each ignore file named in `err` once per walk; `at` stands in for an error that
    /// names none.
    fn bad_ignore_file(&mut self, err: &ignore::Error, at: &Path) {
        let mut paths = Vec::new();
        ignore_files(err, at, &mut paths);
        for path in paths {
            if self.bad_ignore_files.insert(path.clone()) {
                self.skip(&path, SkipReason::BadIgnoreFile);
            }
        }
    }
}

/// The ignore files an error names, in order.
fn ignore_files(err: &ignore::Error, at: &Path, out: &mut Vec<PathBuf>) {
    match err {
        ignore::Error::Partial(errs) => errs.iter().for_each(|e| ignore_files(e, at, out)),
        ignore::Error::WithPath { path, err } => ignore_files(err, path, out),
        ignore::Error::WithLineNumber { err, .. } | ignore::Error::WithDepth { err, .. } => {
            ignore_files(err, at, out)
        }
        _ => out.push(at.to_path_buf()),
    }
}

/// Whether an error comes from reading the tree (tagged with a depth) rather than an ignore file.
fn is_walk_error(err: &ignore::Error) -> bool {
    match err {
        ignore::Error::WithDepth { .. } | ignore::Error::Loop { .. } => true,
        ignore::Error::WithPath { err, .. } | ignore::Error::WithLineNumber { err, .. } => {
            is_walk_error(err)
        }
        _ => false,
    }
}

fn error_path(err: &ignore::Error) -> Option<&Path> {
    match err {
        ignore::Error::WithPath { path, .. } => Some(path),
        ignore::Error::WithDepth { err, .. } | ignore::Error::WithLineNumber { err, .. } => {
            error_path(err)
        }
        _ => None,
    }
}

/// Whether the regular file at `path` can be opened for reading. Non-blocking and without
/// following a symlink, so a FIFO swapped in after the walk's look cannot block it; nothing is
/// read.
fn readable(path: &Path) -> bool {
    fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW | libc::O_NOCTTY)
        .open(path)
        .is_ok()
}
