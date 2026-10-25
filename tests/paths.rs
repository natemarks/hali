//! Goal tests for the `paths` seam (#3): data dir and config file resolution.
//! The environment is passed in as a lookup closure, never read from the process.

use std::collections::HashMap;
use std::path::PathBuf;

fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let map: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    move |key| map.get(key).cloned()
}

#[test]
fn data_dir_precedence() {
    // HALI_DATA_DIR wins and is used as given.
    let env = env_of(&[
        ("HALI_DATA_DIR", "/srv/hali-data"),
        ("XDG_DATA_HOME", "/xdg/data"),
        ("HOME", "/home/u"),
    ]);
    assert_eq!(
        hali::paths::data_dir(env).unwrap(),
        PathBuf::from("/srv/hali-data")
    );

    // An empty HALI_DATA_DIR is ignored; an absolute XDG_DATA_HOME gets /hali appended.
    let env = env_of(&[
        ("HALI_DATA_DIR", ""),
        ("XDG_DATA_HOME", "/xdg/data"),
        ("HOME", "/home/u"),
    ]);
    assert_eq!(
        hali::paths::data_dir(env).unwrap(),
        PathBuf::from("/xdg/data/hali")
    );

    // A relative XDG_DATA_HOME is ignored (XDG spec); fall back to $HOME/.local/share/hali.
    let env = env_of(&[("XDG_DATA_HOME", "relative/data"), ("HOME", "/home/u")]);
    assert_eq!(
        hali::paths::data_dir(env).unwrap(),
        PathBuf::from("/home/u/.local/share/hali")
    );

    // An empty XDG_DATA_HOME is ignored too.
    let env = env_of(&[("XDG_DATA_HOME", ""), ("HOME", "/home/u")]);
    assert_eq!(
        hali::paths::data_dir(env).unwrap(),
        PathBuf::from("/home/u/.local/share/hali")
    );

    // Nothing to go on: unresolvable.
    assert!(hali::paths::data_dir(env_of(&[])).is_err());
}

#[test]
fn config_file_precedence() {
    let env = env_of(&[("XDG_CONFIG_HOME", "/xdg/config"), ("HOME", "/home/u")]);
    assert_eq!(
        hali::paths::config_file(env).unwrap(),
        PathBuf::from("/xdg/config/hali/config.toml")
    );

    let env = env_of(&[("XDG_CONFIG_HOME", "relative"), ("HOME", "/home/u")]);
    assert_eq!(
        hali::paths::config_file(env).unwrap(),
        PathBuf::from("/home/u/.config/hali/config.toml")
    );

    assert!(hali::paths::config_file(env_of(&[])).is_err());
}
