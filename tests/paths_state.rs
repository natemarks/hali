//! Goal tests for the `paths` seam (#23, spec §4): the state dir, where the remembered scan
//! roots live. The environment is passed in as a lookup closure, never read from the process.
//!
//! Assumed API: `hali::paths::state_dir(env) -> Result<PathBuf, hali::paths::Unresolved>`:
//! `$XDG_STATE_HOME/hali`, else `$HOME/.local/state/hali`; empty values and a relative
//! `XDG_STATE_HOME` are ignored; with neither it is unresolvable.

mod goal {
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |key| map.get(key).cloned()
    }

    /// Row 1: an absolute `XDG_STATE_HOME` wins and gets `/hali` appended; the other XDG
    /// variables and `HALI_DATA_DIR` do not move it.
    #[test]
    fn state_dir_from_xdg_state_home() {
        let env = env_of(&[("XDG_STATE_HOME", "/xdg/state"), ("HOME", "/home/u")]);
        assert_eq!(
            hali::paths::state_dir(env).unwrap(),
            PathBuf::from("/xdg/state/hali")
        );

        let env = env_of(&[
            ("HALI_DATA_DIR", "/srv/hali-data"),
            ("XDG_STATE_HOME", "/xdg/state"),
            ("XDG_DATA_HOME", "/xdg/data"),
            ("HOME", "/home/u"),
        ]);
        assert_eq!(
            hali::paths::state_dir(env).unwrap(),
            PathBuf::from("/xdg/state/hali")
        );

        // No HOME needed when XDG_STATE_HOME is usable.
        let env = env_of(&[("XDG_STATE_HOME", "/xdg/state")]);
        assert_eq!(
            hali::paths::state_dir(env).unwrap(),
            PathBuf::from("/xdg/state/hali")
        );
    }

    /// Row 1: else `$HOME/.local/state/hali`, and a relative or empty `XDG_STATE_HOME` is
    /// ignored.
    #[test]
    fn state_dir_from_home() {
        let home = [("HOME", "/home/u")];
        let env = env_of(&home);
        assert_eq!(
            hali::paths::state_dir(env).unwrap(),
            PathBuf::from("/home/u/.local/state/hali")
        );
        for value in ["relative/state", ""] {
            let vars = [("XDG_STATE_HOME", value), ("HOME", "/home/u")];
            let env = env_of(&vars);
            assert_eq!(
                hali::paths::state_dir(env).unwrap(),
                PathBuf::from("/home/u/.local/state/hali"),
                "{value:?}"
            );
        }
    }

    /// Row 1: nothing to go on is unresolvable.
    #[test]
    fn state_dir_unresolvable() {
        assert!(hali::paths::state_dir(env_of(&[])).is_err());
        assert!(hali::paths::state_dir(env_of(&[("HOME", "")])).is_err());
        assert!(
            hali::paths::state_dir(env_of(&[("XDG_STATE_HOME", "relative")])).is_err(),
            "a relative XDG_STATE_HOME with no HOME is unresolvable"
        );
    }
}
