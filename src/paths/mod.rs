//! The `paths` seam: where hali's own files live (spec §4).
//!
//! The environment is passed in as a lookup function, so callers decide where it comes from.

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
