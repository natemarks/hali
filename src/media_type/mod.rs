//! The `media_type` seam: detection from the freedesktop shared-mime-info database, in process
//! (spec §5.1).
//!
//! The database is user-writable (`~/.local/share/mime`), so it is only ever matched against
//! the bounded head of a file, and a panic inside `xdg-mime` counts as "no match".

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Read;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use xdg_mime::SharedMimeInfo;

/// How much of a file's content detection looks at.
pub const HEAD_LEN: usize = 8 * 1024;

/// The media type of a file, lowercase `type/subtype`, from its final path component and the
/// first bytes of its content (at most `HEAD_LEN`): filename glob first, then content magic
/// (empty content uses the glob only). With no match, text-looking content is `text/plain`
/// and anything else `application/octet-stream`.
pub fn detect(file_name: &str, head: &[u8]) -> String {
    let head = &head[..head.len().min(HEAD_LEN)];
    quietly(|| lookup(file_name, head))
        .flatten()
        .filter(|m| is_plain(m))
        .unwrap_or_else(|| {
            if looks_like_text(head) {
                "text/plain".to_string()
            } else {
                "application/octet-stream".to_string()
            }
        })
}

/// The media type a file name alone gives (its glob match), lowercase `type/subtype`; `None`
/// when the name matches no glob. A cheap pre-filter: it reads no content.
pub fn glob_type(file_name: &str) -> Option<String> {
    quietly(|| {
        database()
            .get_mime_types_from_file_name(file_name)
            .into_iter()
            .map(|m| m.essence_str().to_ascii_lowercase())
            .next()
            .filter(|m| m != "application/octet-stream")
    })
    .flatten()
    .filter(|m| is_plain(m))
}

/// Whether `media_type` is `text/plain` or lies under it: every `text/*` type counts (the
/// implicit rule), and so does any type with an ancestor of top-level type `text` in the
/// subclass tree, aliases resolved (spec §5.2). The walk is breadth first with a visited set
/// and a node cap, so a cyclic or oversized database cannot hang it. With no database only the
/// implicit rule applies.
///
/// `xdg-mime` is not used for the tree: its `get_parents` answers only for aliased types and
/// its `mime_type_subclass` recurses without a cycle check. The `subclasses` and `aliases`
/// files are read here instead (each bounded to `MAX_DB_FILE` bytes, all of them together to
/// `MAX_DB_TOTAL`). The answer is remembered per media type for the process.
pub fn is_text(media_type: &str) -> bool {
    static MEMO: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new();
    let media_type = media_type.to_ascii_lowercase();
    if media_type.starts_with("text/") {
        return true;
    }
    let memo = MEMO.get_or_init(Default::default);
    if let Some(&known) = memo
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&media_type)
    {
        return known;
    }
    let answer = walk(&media_type);
    memo.lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(media_type, answer);
    answer
}

/// The breadth-first walk behind `is_text`, over at most `MAX_NODES` types.
fn walk(media_type: &str) -> bool {
    /// The most types one walk looks at.
    const MAX_NODES: usize = 256;
    quietly(|| {
        let tree = tree();
        let canonical = |t: &str| {
            tree.aliases
                .get(t)
                .cloned()
                .unwrap_or_else(|| t.to_string())
        };
        let mut seen: HashSet<String> = HashSet::new();
        let mut queue = VecDeque::new();
        let start = canonical(media_type);
        seen.insert(start.clone());
        queue.push_back(start);
        while let Some(current) = queue.pop_front() {
            if current.starts_with("text/") {
                return true;
            }
            for parent in tree.parents.get(&current).into_iter().flatten() {
                // Once the cap is reached no type is added, so the rest of a fan-out is moot.
                if seen.len() >= MAX_NODES {
                    break;
                }
                let parent = canonical(parent);
                if seen.insert(parent.clone()) {
                    queue.push_back(parent);
                }
            }
        }
        false
    })
    .unwrap_or(false)
}

/// The most bytes read from one `subclasses` or `aliases` file; the database is user-writable.
const MAX_DB_FILE: u64 = 4 * 1024 * 1024;

/// The most bytes read from all the `subclasses` and `aliases` files together.
const MAX_DB_TOTAL: u64 = 16 * 1024 * 1024;

#[derive(Default)]
struct Tree {
    /// Alias -> canonical type.
    aliases: HashMap<String, String>,
    /// Type -> direct parents.
    parents: HashMap<String, Vec<String>>,
}

fn tree() -> &'static Tree {
    static TREE: OnceLock<Tree> = OnceLock::new();
    TREE.get_or_init(|| {
        let mut tree = Tree::default();
        let mut edges: HashSet<(String, String)> = HashSet::new();
        // A directory or file listed twice (or hard-linked) is read once.
        let mut dirs_read: HashSet<(u64, u64)> = HashSet::new();
        let mut files_read: HashSet<(u64, u64)> = HashSet::new();
        let mut budget = MAX_DB_TOTAL;
        for dir in data_dirs() {
            let Ok(meta) = std::fs::metadata(&dir) else {
                continue;
            };
            if !meta.is_dir() || !dirs_read.insert((meta.dev(), meta.ino())) {
                continue;
            }
            for (name, parents) in [("aliases", false), ("subclasses", true)] {
                if budget == 0 {
                    break;
                }
                let Some(text) =
                    read_bounded(&dir.join("mime").join(name), &mut files_read, &mut budget)
                else {
                    continue;
                };
                for line in text.lines() {
                    let mut words = line.split_whitespace();
                    let (Some(a), Some(b), None) = (words.next(), words.next(), words.next())
                    else {
                        continue;
                    };
                    let (a, b) = (a.to_ascii_lowercase(), b.to_ascii_lowercase());
                    if !is_plain(&a) || !is_plain(&b) {
                        continue;
                    }
                    if parents {
                        if edges.insert((a.clone(), b.clone())) {
                            tree.parents.entry(a).or_default().push(b);
                        }
                    } else {
                        tree.aliases.entry(a).or_insert(b);
                    }
                }
            }
        }
        tree
    })
}

/// The data directories of the shared-mime-info database, user first. Only absolute ones: a
/// relative `XDG_DATA_HOME` falls back to `$HOME/.local/share`, and a relative `HOME` or
/// `XDG_DATA_DIRS` entry is ignored.
fn data_dirs() -> Vec<PathBuf> {
    let var = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
    };
    let mut dirs = Vec::new();
    match var("XDG_DATA_HOME") {
        Some(home) => dirs.push(home),
        None => dirs.extend(var("HOME").map(|h| h.join(".local/share"))),
    }
    let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty());
    let system = var("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    dirs.extend(std::env::split_paths(&system).filter(|p| p.is_absolute()));
    dirs
}

/// The text of the database file at `path`, at most `MAX_DB_FILE` bytes and what is left of
/// `budget` (which it lowers); `None` when it is absent, not a regular file (a symlink, a FIFO,
/// a directory, a device), already read under another name (`seen` holds (dev, inode)), or not
/// UTF-8. Only a regular file is ever opened, and never blocking.
fn read_bounded(path: &Path, seen: &mut HashSet<(u64, u64)>, budget: &mut u64) -> Option<String> {
    // A first look without opening, so a FIFO or device is never opened.
    if !std::fs::symlink_metadata(path).ok()?.file_type().is_file() {
        return None;
    }
    // O_NONBLOCK and O_NOFOLLOW: a FIFO or a symlink swapped in since cannot block the open or
    // redirect it. O_NOCTTY: a terminal never becomes the controlling one. `fstat` then
    // confirms what was opened.
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW | libc::O_NOCTTY)
        .open(path)
        .ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || !seen.insert((meta.dev(), meta.ino())) {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_DB_FILE.min(*budget))
        .read_to_end(&mut bytes)
        .ok()?;
    *budget -= bytes.len() as u64;
    String::from_utf8(bytes).ok()
}

fn database() -> &'static SharedMimeInfo {
    static DB: OnceLock<SharedMimeInfo> = OnceLock::new();
    DB.get_or_init(SharedMimeInfo::new)
}

fn lookup(file_name: &str, head: &[u8]) -> Option<String> {
    let db = database();
    // With no glob match, xdg-mime answers `application/octet-stream`.
    let globbed = db
        .get_mime_types_from_file_name(file_name)
        .into_iter()
        .map(|m| m.essence_str().to_ascii_lowercase())
        .next()
        .filter(|m| m != "application/octet-stream");
    match globbed {
        Some(m) => Some(m),
        None if head.is_empty() => None,
        None => db
            .get_mime_type_for_data(head)
            .map(|(m, _)| m.essence_str().to_ascii_lowercase()),
    }
}

/// `type/subtype` made of characters that are safe in an IRI. The database is user-writable,
/// and the media type becomes part of an IRI (spec §2.1).
fn is_plain(media_type: &str) -> bool {
    let part = |p: &str| {
        !p.is_empty()
            && p.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"+-._".contains(&b))
    };
    matches!(media_type.split_once('/'), Some((t, s)) if part(t) && part(s))
}

/// No NUL byte and valid UTF-8, allowing a final sequence cut off by the head's end. Empty
/// content counts as text.
fn looks_like_text(head: &[u8]) -> bool {
    if head.contains(&0) {
        return false;
    }
    match std::str::from_utf8(head) {
        Ok(_) => true,
        // `error_len() == None`: the input ended inside a sequence that could still be valid.
        Err(e) => e.error_len().is_none(),
    }
}

/// Runs `f` with the panic hook silenced, turning a panic into `None`.
fn quietly<T>(f: impl FnOnce() -> T) -> Option<T> {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    std::panic::set_hook(hook);
    result.ok()
}

#[cfg(test)]
mod tests {
    use super::{is_plain, looks_like_text};

    #[test]
    fn only_plain_media_types_are_used() {
        assert!(is_plain("application/vnd.oasis.opendocument.text"));
        assert!(is_plain("image/svg+xml"));
        assert!(!is_plain("text/a|b"));
        assert!(!is_plain("text/%41"));
        assert!(!is_plain("text"));
        assert!(!is_plain("text/"));
        assert!(!is_plain("a/b/c"));
    }

    #[test]
    fn text_heuristic() {
        assert!(looks_like_text(b""));
        assert!(looks_like_text("caf\u{e9}".as_bytes()));
        assert!(looks_like_text(b"caf\xc3"));
        assert!(!looks_like_text(b"a\x00b"));
        assert!(!looks_like_text(b"bad \xff\xfe bytes"));
    }
}
