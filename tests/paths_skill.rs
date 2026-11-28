//! Goal test for `hali::paths::skill_dirs` (#28, spec §11.3): the two directories `hali skill
//! install` writes, from `HOME` only.
//!
//! Assumed API: `skill_dirs(env: impl Fn(&str) -> Option<String>) -> Result<(PathBuf, PathBuf),
//! Unresolved>`: `($HOME/.claude/skills/hali, $HOME/.agents/skills/hali)`. An unset, empty or
//! relative `HOME` is `Unresolved`; no other variable (`CLAUDE_CONFIG_DIR`, `XDG_*`) counts.

#[cfg(feature = "goal")]
mod goal {
    use hali::paths;
    use std::path::PathBuf;

    fn env_of(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
        let owned: Vec<(String, String)> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |k: &str| {
            owned
                .iter()
                .find(|(name, _)| name == k)
                .map(|(_, v)| v.clone())
        }
    }

    #[test]
    fn skill_dirs() {
        let (claude, agents) = paths::skill_dirs(env_of(&[("HOME", "/home/u")])).unwrap();
        assert_eq!(claude, PathBuf::from("/home/u/.claude/skills/hali"));
        assert_eq!(agents, PathBuf::from("/home/u/.agents/skills/hali"));

        // Only HOME counts.
        let (claude, agents) = paths::skill_dirs(env_of(&[
            ("HOME", "/home/u"),
            ("CLAUDE_CONFIG_DIR", "/elsewhere"),
            ("XDG_CONFIG_HOME", "/xdg"),
            ("XDG_DATA_HOME", "/xdg"),
        ]))
        .unwrap();
        assert_eq!(claude, PathBuf::from("/home/u/.claude/skills/hali"));
        assert_eq!(agents, PathBuf::from("/home/u/.agents/skills/hali"));

        let unresolved: [(&str, Vec<(&str, &str)>); 6] = [
            ("HOME unset", vec![]),
            ("HOME empty", vec![("HOME", "")]),
            ("HOME relative", vec![("HOME", "home/u")]),
            ("HOME dot", vec![("HOME", ".")]),
            ("HOME tilde", vec![("HOME", "~")]),
            (
                "only other variables",
                vec![("XDG_CONFIG_HOME", "/xdg"), ("CLAUDE_CONFIG_DIR", "/c")],
            ),
        ];
        for (what, vars) in unresolved {
            assert!(paths::skill_dirs(env_of(&vars)).is_err(), "{what}");
        }
    }
}
