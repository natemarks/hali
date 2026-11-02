//! The directory walker behind `hali scan` (spec §7), on the `ignore` crate. It only reads, it is
//! sequential, and it is the only place hali names that crate.
//!
//! The walk yields every regular, UTF-8 file it keeps, and every entry it has to skip with a
//! reason; a skip never stops it. Directory symlinks are never followed, so there are no cycles.
//! It never opens a regular file other than an ignore file.

use ignore::overrides::{Override, OverrideBuilder};
use ignore::{DirEntry, WalkBuilder};
use std::collections::{HashSet, VecDeque};
use std::fs;
use std::io::{ErrorKind, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

/// Directories never entered, at any depth, whatever the options.
const BUILTIN_EXCLUDES: [&str; 5] = [".git", "node_modules", "target", "__pycache__", "venv"];

/// The custom ignore file, in gitignore syntax.
const HALIIGNORE: &str = ".haliignore";

/// The ignore files the crate reads in each directory it enters (with the ignore sources on).
const IGNORE_FILES: [&str; 3] = [".gitignore", ".ignore", HALIIGNORE];

/// The largest ignore file accepted, in bytes (1 MiB).
const MAX_IGNORE_FILE: u64 = 1 << 20;

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
    /// Returns the first pattern in `excludes` that is not a valid glob, is empty or only
    /// whitespace, or starts with `!`.
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
    /// A directory the walk cannot read, or an entry whose `stat` fails.
    Unreadable,
    /// A FIFO, socket, device, or a broken or looping symlink.
    NotRegular,
    NonUtf8,
    /// A file symlink that resolves outside the root.
    OutsideRoot,
    /// An ignore file that fails the fail-closed check (its directory is not walked), or one
    /// with an invalid line (its valid lines still apply).
    BadIgnoreFile,
}

/// The user excludes as `ignore` overrides (each pattern negated, so a match is ignored).
fn overrides(root: &Path, excludes: &[String]) -> Result<Override, String> {
    let mut builder = OverrideBuilder::new(root);
    for pattern in excludes {
        // An empty or blank pattern would match nothing, or everything.
        if pattern.trim().is_empty() || pattern.starts_with('!') {
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

/// Walks `root`, an absolute directory, from its real path: depth first, each directory's
/// entries sorted by file name (byte order). The root itself is always walked, even if hidden
/// or excluded. If an ignore file of the root or of one of its parents fails the fail-closed
/// check, that file is the only entry and nothing is walked.
///
/// Panics if `opts.excludes` holds a pattern that `WalkOptions::new` rejects.
pub fn walk(root: &Path, opts: &WalkOptions) -> impl Iterator<Item = Walked> + use<> {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let use_ignore_files = !opts.no_ignore;
    let pending = Arc::new(Mutex::new(Vec::new()));
    let mut walk = Walk {
        inner: None,
        root: root.clone(),
        pending: Arc::clone(&pending),
        queue: VecDeque::new(),
        bad_ignore_files: HashSet::new(),
    };

    // The crate reads the ignore files of every parent (`parents(true)`) and of the root before
    // the filter sees anything, so they are checked here, from `/` down.
    if use_ignore_files {
        let mut dirs: Vec<&Path> = root.ancestors().collect();
        dirs.reverse();
        if let Some(bad) = dirs
            .into_iter()
            .find_map(|dir| check_ignore_files(dir).err())
        {
            walk.report(&bad, SkipReason::BadIgnoreFile);
            return walk;
        }
    }

    let mut builder = WalkBuilder::new(&root);
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
            overrides(&root, &opts.excludes).expect("excludes are validated by WalkOptions::new"),
        );
    if use_ignore_files {
        builder.add_custom_ignore_filename(HALIIGNORE);
    }

    // The data dir as given and as its real path.
    let mut prune = opts.prune.clone();
    prune.extend(opts.prune.iter().filter_map(|p| p.canonicalize().ok()));

    // `filter_entry` cannot yield, so it hands its skips over to the iterator. It runs on a
    // directory after the hidden and ignore rules and before the crate reads its ignore files.
    builder.filter_entry(move |ent| {
        let is_dir = ent.file_type().is_some_and(|t| t.is_dir());
        if is_dir && BUILTIN_EXCLUDES.iter().any(|b| ent.file_name() == *b) {
            return false;
        }
        if prune.iter().any(|p| ent.path() == p) {
            return false;
        }
        if ent.file_name().to_str().is_none() {
            lock(&pending).push((ent.path().to_path_buf(), SkipReason::NonUtf8));
            return false;
        }
        if is_dir
            && use_ignore_files
            && let Err(bad) = check_ignore_files(ent.path())
        {
            lock(&pending).push((bad, SkipReason::BadIgnoreFile));
            return false;
        }
        true
    });

    walk.inner = Some(builder.build());
    walk
}

type Pending = Mutex<Vec<(PathBuf, SkipReason)>>;

fn lock(pending: &Pending) -> std::sync::MutexGuard<'_, Vec<(PathBuf, SkipReason)>> {
    pending.lock().unwrap_or_else(|e| e.into_inner())
}

/// Checks, before the crate reads them, the ignore files it would read for `dir`: its
/// `.gitignore`, `.ignore` and `.haliignore`, and through `.git` the repository's
/// `info/exclude` (for a `.git` file also the file itself and the `commondir` it leads to).
/// Returns the first one that fails.
fn check_ignore_files(dir: &Path) -> Result<(), PathBuf> {
    for name in IGNORE_FILES {
        read_ignore_file(&dir.join(name))?;
    }
    let git = dir.join(".git");
    let git_type = match fs::symlink_metadata(&git) {
        Ok(meta) => meta.file_type(),
        Err(e) if absent(&e) => return Ok(()),
        Err(_) => return Err(git),
    };
    // The crate follows a `.git` symlink; a directory holds the repository itself.
    if git_type.is_dir() || (git_type.is_symlink() && git.is_dir()) {
        return read_ignore_file(&git.join("info/exclude")).map(drop);
    }
    // A `.git` file `gitdir: <dir>`, read the crate's way (its first line); `<dir>/commondir`
    // names the directory that holds `info/exclude`.
    let Some(text) = read_ignore_file(&git)? else {
        return Ok(());
    };
    let Some(git_dir) = text.lines().next().and_then(|l| l.strip_prefix("gitdir: ")) else {
        return Ok(());
    };
    let git_dir = PathBuf::from(git_dir);
    let Some(text) = read_ignore_file(&git_dir.join("commondir"))? else {
        return Ok(());
    };
    let Some(common) = text.lines().next() else {
        return Ok(());
    };
    let common = if common.starts_with('.') {
        git_dir.join(common)
    } else {
        PathBuf::from(common)
    };
    read_ignore_file(&common.join("info/exclude")).map(drop)
}

/// Whether a failed `lstat` means there is nothing the crate could read: the file is missing,
/// a parent is not a directory, or the directory cannot be searched (then the crate cannot
/// open the file either, and the directory itself comes out unreadable).
fn absent(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        ErrorKind::NotFound | ErrorKind::NotADirectory | ErrorKind::PermissionDenied
    )
}

/// Reads the ignore file at `path` with a bounded, non-blocking read: `Ok(None)` when there is
/// none, its text when it is a regular, valid UTF-8 file of at most 1 MiB, else `Err(path)`.
/// Only a regular file is ever opened, so a FIFO or a device cannot block or flood the walk.
fn read_ignore_file(path: &Path) -> Result<Option<String>, PathBuf> {
    let bad = || path.to_path_buf();
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_file() => {}
        Ok(_) => return Err(bad()),
        Err(e) if absent(&e) => return Ok(None),
        Err(_) => return Err(bad()),
    }
    // O_NONBLOCK and O_NOFOLLOW: a FIFO or a symlink swapped in after that look can neither
    // block the open nor redirect it; `fstat` then confirms what was opened.
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW | libc::O_NOCTTY)
        .open(path)
        .map_err(|_| bad())?;
    let meta = file.metadata().map_err(|_| bad())?;
    if !meta.is_file() || meta.len() > MAX_IGNORE_FILE {
        return Err(bad());
    }
    let mut bytes = Vec::new();
    file.take(MAX_IGNORE_FILE + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| bad())?;
    if bytes.len() as u64 > MAX_IGNORE_FILE {
        return Err(bad());
    }
    String::from_utf8(bytes).map(Some).map_err(|_| bad())
}

struct Walk {
    /// `None` when the root's or a parent's ignore files failed the check.
    inner: Option<ignore::Walk>,
    root: PathBuf,
    pending: Arc<Pending>,
    queue: VecDeque<Walked>,
    bad_ignore_files: HashSet<PathBuf>,
}

impl Iterator for Walk {
    type Item = Walked;

    fn next(&mut self) -> Option<Walked> {
        loop {
            if let Some(walked) = self.queue.pop_front() {
                return Some(walked);
            }
            let item = self.inner.as_mut()?.next();
            // Skips the filter met during that step come before the entry it returned.
            let skips: Vec<(PathBuf, SkipReason)> = lock(&self.pending).drain(..).collect();
            for (path, reason) in skips {
                self.report(&path, reason);
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
    fn report(&mut self, path: &Path, reason: SkipReason) {
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
            self.report(path, SkipReason::NonUtf8);
        } else if file_type.is_symlink() {
            self.symlink(path);
        } else if !file_type.is_file() {
            self.report(path, SkipReason::NotRegular);
        } else {
            // `lstat` only: the file itself is never opened.
            match ent
                .metadata()
                .ok()
                .and_then(|m| Some((m.len(), m.modified().ok()?)))
            {
                Some((byte_size, mtime)) => self.queue.push_back(Walked::File {
                    path: path.to_string_lossy().into_owned(),
                    byte_size,
                    mtime,
                }),
                None => self.report(path, SkipReason::Unreadable),
            }
        }
    }

    /// Resolves a symlink without opening anything. A directory, or a regular file inside the
    /// root, is silent: the walk reaches the real file directly, under every filter.
    fn symlink(&mut self, link: &Path) {
        let Ok(real) = fs::canonicalize(link) else {
            return self.report(link, SkipReason::NotRegular);
        };
        let Ok(meta) = fs::metadata(&real) else {
            return self.report(link, SkipReason::NotRegular);
        };
        if meta.is_dir() {
            return;
        }
        if !real.starts_with(&self.root) {
            return self.report(link, SkipReason::OutsideRoot);
        }
        if !meta.is_file() {
            self.report(link, SkipReason::NotRegular);
        }
    }

    /// An error the walk itself returns: a directory it could not read, or an ignore file in
    /// one of the root's parents.
    fn error(&mut self, err: &ignore::Error) {
        if is_walk_error(err) {
            let path = error_path(err).unwrap_or(&self.root).to_path_buf();
            self.report(&path, SkipReason::Unreadable);
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
                self.report(&path, SkipReason::BadIgnoreFile);
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
