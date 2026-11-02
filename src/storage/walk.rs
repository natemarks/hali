//! The directory walker behind `hali scan` (spec §7), on the `ignore` crate. It only reads, it is
//! sequential, and it is the only place hali names that crate.
//!
//! The walk yields every regular, UTF-8 file it keeps, and every entry it has to skip with a
//! reason; a skip never stops it. Directory symlinks are never followed, so there are no cycles.
//! The only regular files it opens are ignore-file candidates and what the crate reads behind
//! `.git` (a `.git` file and the `commondir` it leads to), each with a bounded, non-blocking
//! read. Ignore files fail closed: one that fails the check, or whose rules would take the live
//! ignore-rule budget (the bytes of ignore files accepted along the current path) over 512 KiB,
//! is reported and its directory is not walked.

use ignore::gitignore::GitignoreBuilder;
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

/// The largest ignore file accepted, in bytes (256 KiB).
const MAX_IGNORE_FILE: u64 = 256 << 10;

/// The deepest `{` nesting accepted on a line of an ignore file. globset recurses once per level
/// when it compiles a glob, so a deep one overflows the stack and aborts the process.
const MAX_BRACE_NESTING: usize = 100;

/// The live ignore-rule budget, in bytes (512 KiB): the crate holds the compiled matcher of every
/// directory above the entry it walks, so the ignore files accepted along the current path (the
/// root's parents included) may total no more than this.
const MAX_LIVE_IGNORE_BYTES: u64 = 512 << 10;

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
    /// whitespace, starts with `!`, or nests `{` deeper than 100 levels.
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
    /// A directory the walk cannot read, an entry whose `stat` fails, or a symlink whose target
    /// cannot be reached for lack of permission.
    Unreadable,
    /// A FIFO, socket, device, a broken or looping symlink, or a symlink to something not
    /// regular.
    NotRegular,
    NonUtf8,
    /// A file symlink that resolves outside the root.
    OutsideRoot,
    /// An ignore file that fails the fail-closed check or would take the live ignore-rule budget
    /// over 512 KiB (its directory is not walked), or one with an invalid line (its valid lines
    /// still apply).
    BadIgnoreFile,
}

/// The user excludes as `ignore` overrides (each pattern negated, so a match is ignored).
fn overrides(root: &Path, excludes: &[String]) -> Result<Override, String> {
    let mut builder = OverrideBuilder::new(root);
    for pattern in excludes {
        // An empty or blank pattern would match nothing, or everything.
        // Over-nested braces are refused before `add`, whose compile would overflow the stack.
        if pattern.trim().is_empty()
            || pattern.starts_with('!')
            || brace_nesting(pattern) > MAX_BRACE_NESTING
        {
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
/// check or overflows the live ignore-rule budget, it is reported and nothing is walked: that
/// is the one entry, unless the crate still drops the rules of a file changed after the check,
/// when the fail-closed fallback reports every ignore file of the parents.
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
        not_walked: Vec::new(),
    };

    // The crate reads the ignore files of every parent (`parents(true)`) and of the root before
    // the filter sees anything, so they are checked here, from `/` down.
    // They stay charged to the live ignore-rule budget for the whole walk.
    let mut budget = Budget::default();
    if use_ignore_files {
        let mut dirs: Vec<&Path> = root.ancestors().collect();
        dirs.reverse();
        for dir in dirs {
            match check_ignore_files(dir, budget.enter(0)) {
                Ok(bytes) => budget.charge(0, bytes),
                Err(bad) => {
                    walk.report(&bad, SkipReason::BadIgnoreFile);
                    return walk;
                }
            }
        }
    }
    let budget = Mutex::new(budget);

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
        if is_dir && use_ignore_files {
            let mut budget = budget.lock().unwrap_or_else(|e| e.into_inner());
            match check_ignore_files(ent.path(), budget.enter(ent.depth())) {
                Ok(bytes) => budget.charge(ent.depth(), bytes),
                Err(bad) => {
                    lock(&pending).push((bad, SkipReason::BadIgnoreFile));
                    return false;
                }
            }
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
/// The ignore files (not `.git` or `commondir`, which hold no rules) are charged against
/// `remaining`, the bytes left of the live ignore-rule budget. Returns the bytes charged, or the
/// first file that fails, which may be the one that would take the budget over.
fn check_ignore_files(dir: &Path, remaining: u64) -> Result<u64, PathBuf> {
    let mut charged = 0;
    for name in IGNORE_FILES {
        charged += check_ignore_file(dir, &dir.join(name), remaining - charged)?;
    }
    let git = dir.join(".git");
    let git_type = match fs::symlink_metadata(&git) {
        Ok(meta) => meta.file_type(),
        Err(e) if absent(&e) => return Ok(charged),
        Err(_) => return Err(git),
    };
    // The crate follows a `.git` symlink; a directory holds the repository itself.
    let exclude = if git_type.is_dir() || (git_type.is_symlink() && git.is_dir()) {
        git.join("info/exclude")
    } else {
        // A `.git` file `gitdir: <dir>`, read the crate's way (its first line); `<dir>/commondir`
        // names the directory that holds `info/exclude`.
        let Some(text) = read_ignore_file(&git)? else {
            return Ok(charged);
        };
        let Some(git_dir) = text.lines().next().and_then(|l| l.strip_prefix("gitdir: ")) else {
            return Ok(charged);
        };
        let git_dir = PathBuf::from(git_dir);
        let Some(text) = read_ignore_file(&git_dir.join("commondir"))? else {
            return Ok(charged);
        };
        let Some(common) = text.lines().next() else {
            return Ok(charged);
        };
        let common = if common.starts_with('.') {
            git_dir.join(common)
        } else {
            PathBuf::from(common)
        };
        common.join("info/exclude")
    };
    Ok(charged + check_ignore_file(dir, &exclude, remaining - charged)?)
}

/// Checks the ignore file at `path`, read for `dir`: it must pass [`read_ignore_file`], no line
/// may nest `{` deeper than [`MAX_BRACE_NESTING`], its size may not exceed `remaining` (what is
/// left of the live ignore-rule budget), and its matcher must build (the crate drops every rule
/// of a file whose matcher it fails to build, on a regex size limit). The build mirrors the
/// crate's, one matcher per file, and is dropped at once; a line that is invalid on its own is
/// left to the crate, which reports it with the file's path and applies the other lines.
/// Returns the file's size in bytes (0 when there is none).
fn check_ignore_file(dir: &Path, path: &Path, remaining: u64) -> Result<u64, PathBuf> {
    let Some(text) = read_ignore_file(path)? else {
        return Ok(0);
    };
    let bytes = text.len() as u64;
    // Before the build, so a file over the budget never costs a compile.
    if bytes > remaining {
        return Err(path.to_path_buf());
    }
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    // Comment lines are never compiled; checked before the build, which would overflow.
    if text
        .lines()
        .any(|line| !line.starts_with('#') && brace_nesting(line) > MAX_BRACE_NESTING)
    {
        return Err(path.to_path_buf());
    }
    let mut builder = GitignoreBuilder::new(dir);
    for line in text.lines() {
        // The crate skips an invalid line too.
        let _ = builder.add_line(None, line);
    }
    builder
        .build()
        .map(|_| bytes)
        .map_err(|_| path.to_path_buf())
}

/// The live ignore-rule budget: the bytes of the ignore files accepted for the directories on
/// the current path of the walk. The walk is depth first, so a directory at some depth leaves
/// every directory charged at that depth or deeper. The root and its parents are charged at
/// depth 0, which no directory below the root has, so they stay charged for the whole walk.
#[derive(Debug, Default)]
struct Budget {
    /// The depth of each charged directory and its bytes, outermost first.
    path: Vec<(usize, u64)>,
}

impl Budget {
    /// Leaves the directories that a directory at `depth` is not under; returns the bytes left.
    fn enter(&mut self, depth: usize) -> u64 {
        while self.path.last().is_some_and(|&(d, _)| d > 0 && d >= depth) {
            self.path.pop();
        }
        let used: u64 = self.path.iter().map(|&(_, bytes)| bytes).sum();
        MAX_LIVE_IGNORE_BYTES.saturating_sub(used)
    }

    /// Charges `bytes` to the directory at `depth` just entered.
    fn charge(&mut self, depth: usize, bytes: u64) {
        if bytes > 0 {
            self.path.push((depth, bytes));
        }
    }
}

/// The deepest `{` nesting of a glob line, scanned the way globset parses it: `\` escapes the
/// next character, and a `[...]` class holds no braces (its first character, after an optional
/// `!` or `^`, is a member even when it is `]`). Once a `[` has no closing `]`, globset reads
/// every later `[` as a literal.
fn brace_nesting(line: &str) -> usize {
    let chars: Vec<char> = line.chars().collect();
    let (mut depth, mut deepest, mut unclosed_class) = (0_usize, 0, false);
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 1,
            '{' => {
                depth += 1;
                deepest = depth.max(deepest);
            }
            '}' => depth = depth.saturating_sub(1),
            '[' if !unclosed_class => {
                let mut first = i + 1;
                if matches!(chars.get(first), Some('!' | '^')) {
                    first += 1;
                }
                let close = chars
                    .get(first + 1..)
                    .and_then(|rest| rest.iter().position(|&c| c == ']'));
                match close {
                    Some(at) => i = first + 1 + at,
                    None => unclosed_class = true,
                }
            }
            _ => {}
        }
        i += 1;
    }
    deepest
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
/// none, its text when it is a regular, valid UTF-8 file of at most 256 KiB, else `Err(path)`.
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
    /// Directories whose ignore rules the crate dropped: nothing below them is yielded.
    not_walked: Vec<PathBuf>,
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
                if !self.below_not_walked(&path) {
                    self.report(&path, reason);
                }
            }
            match item {
                Some(Ok(ent)) if self.below_not_walked(ent.path()) => {}
                Some(Ok(ent)) => self.entry(&ent),
                Some(Err(err)) if error_path(&err).is_some_and(|p| self.below_not_walked(p)) => {}
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
        if let Some(err) = ent.error()
            && self.bad_ignore_file(err)
        {
            let dir = ent.path().to_path_buf();
            self.rules_dropped(std::slice::from_ref(&dir));
            self.not_walked.push(dir);
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
    /// A link that cannot be resolved for lack of permission is unreadable; a broken or looping
    /// one is not regular.
    fn symlink(&mut self, link: &Path) {
        let resolved = fs::canonicalize(link).and_then(|real| Ok((fs::metadata(&real)?, real)));
        let (meta, real) = match resolved {
            Ok(found) => found,
            Err(e) if e.kind() == ErrorKind::PermissionDenied => {
                return self.report(link, SkipReason::Unreadable);
            }
            Err(_) => return self.report(link, SkipReason::NotRegular),
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
        } else if self.bad_ignore_file(err) {
            let mut parents: Vec<PathBuf> = self
                .root
                .ancestors()
                .skip(1)
                .map(Path::to_path_buf)
                .collect();
            parents.reverse();
            self.rules_dropped(&parents);
            self.inner = None;
        }
    }

    /// Reports each ignore file named in `err` once per walk. Returns whether part of `err`
    /// names no file: the crate failed to build a matcher and dropped that file's rules.
    fn bad_ignore_file(&mut self, err: &ignore::Error) -> bool {
        let mut paths = Vec::new();
        let unnamed = ignore_files(err, None, &mut paths);
        for path in paths {
            if self.bad_ignore_files.insert(path.clone()) {
                self.report(&path, SkipReason::BadIgnoreFile);
            }
        }
        unnamed
    }

    /// Fails closed when the crate dropped the rules of an ignore file in one of `dirs`, which
    /// the pre-check builds the same way, so only a file changed in between gets here: reports
    /// every ignore file in `dirs` (or the first directory when none is left). The caller stops
    /// walking them.
    fn rules_dropped(&mut self, dirs: &[PathBuf]) {
        let mut found: Vec<PathBuf> = dirs
            .iter()
            .flat_map(|dir| {
                IGNORE_FILES
                    .iter()
                    .map(|name| dir.join(name))
                    .chain([dir.join(".git/info/exclude")])
            })
            .filter(|path| fs::symlink_metadata(path).is_ok())
            .collect();
        if found.is_empty() {
            found.extend(dirs.first().cloned());
        }
        for path in found {
            if self.bad_ignore_files.insert(path.clone()) {
                self.report(&path, SkipReason::BadIgnoreFile);
            }
        }
    }

    fn below_not_walked(&self, path: &Path) -> bool {
        self.not_walked
            .iter()
            .any(|dir| path != dir && path.starts_with(dir))
    }
}

/// Pushes the ignore files an error names, in order, onto `out`; `at` is the file named by an
/// enclosing error. Returns whether some part of it names no file.
fn ignore_files(err: &ignore::Error, at: Option<&Path>, out: &mut Vec<PathBuf>) -> bool {
    match err {
        ignore::Error::Partial(errs) => {
            // Every part, so that every named file is pushed.
            let mut unnamed = false;
            for err in errs {
                unnamed |= ignore_files(err, at, out);
            }
            unnamed
        }
        ignore::Error::WithPath { path, err } => ignore_files(err, Some(path), out),
        ignore::Error::WithLineNumber { err, .. } | ignore::Error::WithDepth { err, .. } => {
            ignore_files(err, at, out)
        }
        _ => match at {
            Some(path) => {
                out.push(path.to_path_buf());
                false
            }
            None => true,
        },
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_less_crate_error_stops_the_walk() {
        // A crate error naming no file (a matcher it failed to build, path unknown) fails
        // closed: the walk stops and reports.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let mut walk = Walk {
            inner: Some(WalkBuilder::new(&root).build()),
            root,
            pending: Arc::new(Mutex::new(Vec::new())),
            queue: VecDeque::new(),
            bad_ignore_files: HashSet::new(),
            not_walked: Vec::new(),
        };
        walk.error(&ignore::Error::Glob {
            glob: None,
            err: "regex too big".to_string(),
        });
        assert!(walk.inner.is_none(), "the walk stops");
        let all: Vec<Walked> = walk.collect();
        assert!(!all.is_empty(), "a report is produced");
        assert!(
            all.iter().all(|w| matches!(
                w,
                Walked::Skipped {
                    reason: SkipReason::BadIgnoreFile,
                    ..
                }
            )),
            "{all:?}"
        );
    }

    #[test]
    fn comment_lines_are_not_nesting_checked() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".ignore");
        let braces = format!("#{}a{}\n", "{".repeat(101), "}".repeat(101));
        fs::write(&path, &braces).unwrap();
        assert_eq!(
            check_ignore_file(dir.path(), &path, MAX_LIVE_IGNORE_BYTES),
            Ok(braces.len() as u64)
        );
        fs::write(&path, &braces[1..]).unwrap();
        assert_eq!(
            check_ignore_file(dir.path(), &path, MAX_LIVE_IGNORE_BYTES),
            Err(path.clone())
        );
    }

    #[test]
    fn budget_follows_the_current_path() {
        let mut budget = Budget::default();
        // The root and its parents, at depth 0.
        assert_eq!(budget.enter(0), MAX_LIVE_IGNORE_BYTES);
        budget.charge(0, 100);
        budget.charge(0, 0);
        assert_eq!(budget.enter(0), MAX_LIVE_IGNORE_BYTES - 100);
        // a/ then a/b/: both charged.
        assert_eq!(budget.enter(1), MAX_LIVE_IGNORE_BYTES - 100);
        budget.charge(1, 10);
        assert_eq!(budget.enter(2), MAX_LIVE_IGNORE_BYTES - 110);
        budget.charge(2, 1);
        assert_eq!(budget.enter(3), MAX_LIVE_IGNORE_BYTES - 111);
        // a/c/ leaves a/b/; d/ leaves a/; the root stays charged.
        assert_eq!(budget.enter(2), MAX_LIVE_IGNORE_BYTES - 110);
        assert_eq!(budget.enter(1), MAX_LIVE_IGNORE_BYTES - 100);
        // A directory with no ignore files charges nothing, and its child pops nothing above.
        budget.charge(1, 0);
        budget.charge(0, MAX_LIVE_IGNORE_BYTES);
        assert_eq!(budget.enter(2), 0, "never below zero");
    }

    #[test]
    fn brace_nesting_follows_globset() {
        assert_eq!(brace_nesting("a"), 0);
        assert_eq!(brace_nesting("{{a}}"), 2);
        assert_eq!(brace_nesting(&"{a}".repeat(200)), 1);
        // Escaped braces are literals.
        assert_eq!(brace_nesting(r"\{\{{a}"), 1);
        // A `}` inside a class closes nothing, also as its first member.
        assert_eq!(brace_nesting("{[}]{[]}]{[!}]a"), 3);
        // A `[` with no `]` is a literal, and so is every later `[`.
        assert_eq!(brace_nesting("[{{[}"), 2);
        // Inside a class `\` is a member, not an escape.
        assert_eq!(brace_nesting(r"[\]{a}"), 1);
        // A stray `}` at depth 0 does not go below zero.
        assert_eq!(brace_nesting("}{{"), 2);
        // A negated class `[^}]` holds no brace.
        assert_eq!(brace_nesting("{[^}]{a}}"), 2);
        // A trailing `\` escapes nothing.
        assert_eq!(brace_nesting("{a\\"), 1);
        assert_eq!(brace_nesting("\\"), 0);
        // Multibyte characters count as one.
        assert_eq!(brace_nesting("{é{ü€}}"), 2);
    }
}
