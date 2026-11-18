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

/// The state dir, where the remembered Scan roots live: `$XDG_STATE_HOME/hali`, else
/// `$HOME/.local/state/hali`. Empty values, and a relative `XDG_STATE_HOME`, are ignored.
pub fn state_dir(env: impl Fn(&str) -> Option<String>) -> Result<PathBuf, Unresolved> {
    xdg_dir(&env, "XDG_STATE_HOME", ".local/state").map(|base| base.join("hali"))
}

/// The most bytes of `user-dirs.dirs` read: it holds a few short lines.
const MAX_USER_DIRS: u64 = 64 * 1024;

/// `files.dir` (spec §4): the first one defined of `configured` (`[files] dir`, a leading `~`
/// being `$HOME`), `$XDG_DOCUMENTS_DIR/hali` from `$XDG_CONFIG_HOME/user-dirs.dirs` (a value of
/// `"$HOME/…"` or an absolute path), and `$HOME/Documents/hali`. `None` when the one defined
/// cannot be resolved (`~` or `$HOME` without `HOME`). Nothing is checked to exist.
pub fn files_dir(
    env: impl Fn(&str) -> Option<String>,
    configured: Option<&str>,
) -> Option<PathBuf> {
    let home = || non_empty(&env, "HOME").map(PathBuf::from);
    if let Some(dir) = configured {
        return match dir.strip_prefix('~') {
            Some("") => home(),
            Some(rest) if rest.starts_with('/') => home().map(|h| h.join(&rest[1..])),
            _ => Some(PathBuf::from(dir)),
        };
    }
    if let Some(documents) = user_dirs_documents(&env) {
        return Some(documents.join("hali"));
    }
    home().map(|h| h.join("Documents/hali"))
}

/// `XDG_DOCUMENTS_DIR` of `user-dirs.dirs` (the last assignment), if the file sets one hali can
/// read: `"$HOME"`, `"$HOME/…"` or an absolute path, in double quotes or bare.
fn user_dirs_documents(env: &impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    use std::io::Read;
    let path = xdg_dir(env, "XDG_CONFIG_HOME", ".config")
        .ok()?
        .join("user-dirs.dirs");
    let mut text = String::new();
    std::fs::File::open(path)
        .ok()?
        .take(MAX_USER_DIRS)
        .read_to_string(&mut text)
        .ok()?;
    let value = text
        .lines()
        .filter_map(|line| line.trim().strip_prefix("XDG_DOCUMENTS_DIR="))
        .next_back()?
        .trim();
    let value = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(value);
    match value.strip_prefix("$HOME") {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => {
            non_empty(env, "HOME").map(|home| PathBuf::from(format!("{home}{rest}")))
        }
        Some(_) => None,
        None => Some(PathBuf::from(value)).filter(|p| p.is_absolute()),
    }
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

/// Why a path argument cannot be a Scan root.
#[derive(Debug)]
pub enum RootError {
    /// The argument, or its real path, is not UTF-8 (shown lossily).
    NonUtf8 { path: String },
    /// Nothing is there, or not a directory: the real path if it resolves, else the argument
    /// made absolute.
    NotADirectory { path: String },
    /// A directory that cannot be read, at its real path; or a path that cannot be resolved for
    /// lack of permission, at the argument made absolute.
    Unreadable { path: String },
}

/// The real path of a Scan root argument: made absolute against the current directory, symlinks
/// resolved, UTF-8, a directory that can be read. Reads the directory's entries, nothing more.
pub fn scan_root(arg: &OsStr) -> Result<String, RootError> {
    let lossy = |p: &Path| p.to_string_lossy().into_owned();
    let Some(text) = arg.to_str() else {
        return Err(RootError::NonUtf8 {
            path: lossy(&absolute(Path::new(arg))),
        });
    };
    let real = std::fs::canonicalize(text).map_err(|e| {
        let path = lossy(&absolute(Path::new(text)));
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            RootError::Unreadable { path }
        } else {
            RootError::NotADirectory { path }
        }
    })?;
    let Some(real_text) = real.to_str() else {
        return Err(RootError::NonUtf8 { path: lossy(&real) });
    };
    let real_text = real_text.to_string();
    if !real.is_dir() {
        return Err(RootError::NotADirectory { path: real_text });
    }
    if std::fs::read_dir(&real).is_err() {
        return Err(RootError::Unreadable { path: real_text });
    }
    Ok(real_text)
}

/// The real path of a directory, if it exists.
pub fn real_dir(dir: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(dir).ok().filter(|p| p.is_dir())
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

/// Why `files.dir` cannot hold the files `hali new` creates (spec §9.1 **New:**).
#[derive(Debug, PartialEq)]
pub enum FilesDirError {
    /// It cannot be resolved: `HOME` is not set.
    Unresolved,
    /// `[files] dir` is a relative path (as configured).
    Relative { path: String },
    /// Its real path is not UTF-8 (shown lossily).
    NonUtf8 { path: String },
    /// Something other than a directory is there (its real path).
    NotADirectory { path: String },
    /// It is one of `own` (hali's data dir or state dir) or inside one (its real path).
    OwnDir { path: String },
}

/// The real path `files.dir` (see `files_dir`) has, or would have once created, for
/// `hali new`: a relative one is refused, and so is one that is in any of the `own` dirs (each
/// resolved the same way), that is not UTF-8, or that holds something other than a directory.
/// Creates nothing.
pub fn new_files_dir(
    env: impl Fn(&str) -> Option<String>,
    configured: Option<&str>,
    own: &[&Path],
) -> Result<String, FilesDirError> {
    let lossy = |p: &Path| p.to_string_lossy().into_owned();
    let dir = files_dir(env, configured).ok_or(FilesDirError::Unresolved)?;
    if !dir.is_absolute() {
        return Err(FilesDirError::Relative { path: lossy(&dir) });
    }
    let (real, usable) = real_target(&dir);
    if own
        .iter()
        .filter(|d| d.is_absolute())
        .any(|d| real.starts_with(real_target(d).0))
    {
        return Err(FilesDirError::OwnDir { path: lossy(&real) });
    }
    let Some(text) = real.to_str() else {
        return Err(FilesDirError::NonUtf8 { path: lossy(&real) });
    };
    if !usable {
        return Err(FilesDirError::NotADirectory {
            path: text.to_string(),
        });
    }
    Ok(text.to_string())
}

/// The real path of an absolute path that may not exist yet, walked one component at a time:
/// each existing component is resolved through symlinks; after the first missing one the rest
/// are applied lexically, and a `..` that pops the last missing one resumes the resolving from
/// the real prefix. The flag is false when the deepest existing prefix is not a directory (a
/// file, a dangling symlink, anything else), so the path cannot be a directory.
fn real_target(path: &Path) -> (PathBuf, bool) {
    use std::path::Component;
    let mut real = PathBuf::from("/");
    let mut missing = 0usize;
    let mut usable = true;
    for part in path.components() {
        match part {
            Component::Normal(name) => {
                let next = real.join(name);
                if missing > 0 || !usable {
                    missing += 1;
                    real = next;
                } else if std::fs::symlink_metadata(&next).is_err() {
                    missing = 1;
                    real = next;
                } else {
                    match std::fs::canonicalize(&next) {
                        Ok(resolved) => {
                            usable = resolved.is_dir();
                            real = resolved;
                        }
                        Err(_) => {
                            usable = false;
                            real = next;
                        }
                    }
                }
            }
            Component::ParentDir => {
                real.pop();
                missing = missing.saturating_sub(1);
            }
            Component::RootDir => real = PathBuf::from("/"),
            Component::CurDir | Component::Prefix(_) => {}
        }
    }
    (real, usable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_files_dir_resolves_the_real_target_without_creating_it() {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        let real = root.join("real");
        std::fs::create_dir(&real).unwrap();
        std::os::unix::fs::symlink(&real, root.join("link")).unwrap();
        let home = root.to_str().unwrap().to_string();
        let env = |k: &str| (k == "HOME").then(|| home.clone());
        let data = root.join("data");
        let own = [data.as_path()];
        let get = |dir: &str| new_files_dir(env, Some(dir), &own);

        // Through a symlink, and through `..` past a missing component; nothing is made.
        let linked = format!("{home}/link/a/b");
        assert_eq!(get(&linked), Ok(format!("{home}/real/a/b")));
        let dotted = format!("{home}/nope/../real/c");
        assert_eq!(get(&dotted), Ok(format!("{home}/real/c")));
        assert!(!real.join("a").exists() && !root.join("nope").exists());
        assert_eq!(get("~/real"), Ok(format!("{home}/real")));

        assert_eq!(
            get("docs"),
            Err(FilesDirError::Relative {
                path: "docs".to_string()
            })
        );
        let inside = format!("{home}/data/files");
        assert_eq!(
            get(&inside),
            Err(FilesDirError::OwnDir {
                path: inside.clone()
            })
        );
        std::fs::write(root.join("afile"), b"").unwrap();
        let file = format!("{home}/afile");
        assert_eq!(
            get(&file),
            Err(FilesDirError::NotADirectory { path: file.clone() })
        );
        let none = |_: &str| None;
        assert_eq!(
            new_files_dir(none, None, &own),
            Err(FilesDirError::Unresolved)
        );
    }
}
