//! The directory walker behind `hali scan` (spec §7), on the `ignore` crate. It only reads, it is
//! sequential, and it is the only place hali names that crate.
//!
//! The walk yields every regular, UTF-8 file it keeps, and every entry it has to skip with a
//! reason; a skip never stops it. Directory symlinks are never followed, so there are no cycles.
//! It never opens a regular file other than an ignore file.

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
        not_walked: Vec::new(),
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
        check_ignore_file(dir, &dir.join(name))?;
    }
    let git = dir.join(".git");
    let git_type = match fs::symlink_metadata(&git) {
        Ok(meta) => meta.file_type(),
        Err(e) if absent(&e) => return Ok(()),
        Err(_) => return Err(git),
    };
    // The crate follows a `.git` symlink; a directory holds the repository itself.
    if git_type.is_dir() || (git_type.is_symlink() && git.is_dir()) {
        return check_ignore_file(dir, &git.join("info/exclude"));
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
    check_ignore_file(dir, &common.join("info/exclude"))
}

/// Checks the ignore file at `path`, read for `dir`: it must pass [`read_ignore_file`], no line
/// may nest `{` deeper than [`MAX_BRACE_NESTING`], and its matcher must build (the crate drops
/// every rule of a file whose matcher it fails to build, on a regex size limit). The build
/// mirrors the crate's, one matcher per file; a line that is invalid on its own is left to the
/// crate, which reports it with the file's path and applies the other lines.
fn check_ignore_file(dir: &Path, path: &Path) -> Result<(), PathBuf> {
    let Some(text) = read_ignore_file(path)? else {
        return Ok(());
    };
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
    builder.build().map(drop).map_err(|_| path.to_path_buf())
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
    use super::brace_nesting;

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
    }
}
