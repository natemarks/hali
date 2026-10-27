//! The `media_type` seam: detection from the freedesktop shared-mime-info database, in process
//! (spec §5.1).
//!
//! The database is user-writable (`~/.local/share/mime`), so it is only ever matched against
//! the bounded head of a file, and a panic inside `xdg-mime` counts as "no match".

use std::sync::OnceLock;
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
