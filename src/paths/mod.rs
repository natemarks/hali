//! The `paths` seam: where hali's own files live (spec §4), how a path argument resolves to
//! a real path (spec §6, §9.2), and the `file://` URL of a path (spec §2.1).
//!
//! The environment is passed in as a lookup function, so callers decide where it comes from.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// No location could be resolved from the environment.
#[derive(Debug)]
pub struct Unresolved;

/// The data dir: `HALI_DATA_DIR` as given, else `$XDG_DATA_HOME/hali`, else
/// `$HOME/.local/share/hali`. Empty values, and a relative `XDG_DATA_HOME`, are ignored.
pub fn data_dir(env: impl Fn(&str) -> Option<String>) -> Result<PathBuf, Unresolved> {
    if let Some(dir) = non_empty(&env, "HALI_DATA_DIR") {
        return Ok(PathBuf::from(dir));
    }
    xdg_dir(&env, "XDG_DATA_HOME", ".local/share").map(|base| base.join("hali"))
}

/// The config file: `$XDG_CONFIG_HOME/hali/config.toml`, else `$HOME/.config/hali/config.toml`.
pub fn config_file(env: impl Fn(&str) -> Option<String>) -> Result<PathBuf, Unresolved> {
    xdg_dir(&env, "XDG_CONFIG_HOME", ".config").map(|base| base.join("hali/config.toml"))
}

/// An XDG base dir: the variable if it is absolute, else `$HOME/<fallback>`.
fn xdg_dir(
    env: &impl Fn(&str) -> Option<String>,
    var: &str,
    fallback: &str,
) -> Result<PathBuf, Unresolved> {
    if let Some(dir) = non_empty(env, var).filter(|d| Path::new(d).is_absolute()) {
        return Ok(PathBuf::from(dir));
    }
    non_empty(env, "HOME")
        .map(|home| PathBuf::from(home).join(fallback))
        .ok_or(Unresolved)
}

fn non_empty(env: &impl Fn(&str) -> Option<String>, var: &str) -> Option<String> {
    env(var).filter(|v| !v.is_empty())
}

/// The bytes of a path that stay as they are in a `file://` URL; every other UTF-8 byte
/// becomes uppercase `%XX`.
const URL_ENCODED: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~')
    .remove(b'/')
    .remove(b'!')
    .remove(b'$')
    .remove(b'&')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')')
    .remove(b'*')
    .remove(b'+')
    .remove(b',')
    .remove(b';')
    .remove(b'=')
    .remove(b':')
    .remove(b'@');

/// The RFC 8089 `file://` URL of an absolute path.
pub fn file_url(path: &str) -> String {
    format!(
        "file://{}",
        percent_encoding::utf8_percent_encode(path, URL_ENCODED)
    )
}

/// The path of a `file:///…` URL, decoded; `None` for any other URL, a malformed `%` escape or
/// a path that is not UTF-8.
pub fn path_from_file_url(url: &str) -> Option<String> {
    let encoded = url.strip_prefix("file://").filter(|p| p.starts_with('/'))?;
    let bytes = encoded.as_bytes();
    for (i, _) in encoded.match_indices('%') {
        let escape = bytes.get(i + 1..i + 3)?;
        if !escape.iter().all(u8::is_ascii_hexdigit) {
            return None;
        }
    }
    percent_encoding::percent_decode_str(encoded)
        .decode_utf8()
        .ok()
        .map(|p| p.into_owned())
}

/// Why a path argument cannot name a file to catalog.
#[derive(Debug)]
pub enum PathError {
    /// The argument, or the real path it resolves to, is not UTF-8 (shown lossily).
    NonUtf8 { path: String },
    /// Nothing exists at the path (shown absolute).
    Missing { path: String },
}

/// The real path of a path argument: symlinks resolved, absolute, UTF-8.
pub fn real_path(arg: &OsStr) -> Result<String, PathError> {
    let lossy = |p: &Path| p.to_string_lossy().into_owned();
    let Some(text) = arg.to_str() else {
        return Err(PathError::NonUtf8 {
            path: lossy(&absolute(Path::new(arg))),
        });
    };
    let real = std::fs::canonicalize(text).map_err(|_| PathError::Missing {
        path: lossy(&absolute(Path::new(text))),
    })?;
    match real.to_str() {
        Some(real) => Ok(real.to_string()),
        None => Err(PathError::NonUtf8 { path: lossy(&real) }),
    }
}

/// The path a Resource reference names: its real path if it exists, else the argument made
/// absolute against the current directory. `None` if that is not UTF-8.
pub fn ref_path(arg: &str) -> Option<String> {
    let path = std::fs::canonicalize(arg).unwrap_or_else(|_| absolute(Path::new(arg)));
    path.to_str().map(str::to_string)
}

/// `path` made absolute against the current directory, without touching the filesystem.
fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}
