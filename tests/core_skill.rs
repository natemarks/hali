//! Goal tests for `hali::core::skill` (#28, spec §11.2, §11.3): the pure seam that renders the
//! embedded skill and decides what an install would do. No I/O.
//!
//! Assumed API (the contract of issue #28, "Groomed contract"; the exact types are fixed here):
//! - `skill_md() -> String`: `skill/SKILL.md` with the placeholder `{{HALI_VERSION}}` replaced by
//!   `env!("CARGO_PKG_VERSION")` (the frontmatter line `  hali-version: "<version>"`);
//! - `files() -> Vec<(&'static str, String)>`: the files an install writes below the skill
//!   directory, in order: `("SKILL.md", skill_md())`, then
//!   `("references/usage.md", core::guide::text())` byte for byte;
//! - `installed_version(text: &str) -> Option<String>`: the `hali-version` of the frontmatter
//!   (the lines between the first `---` line and the next one), which must match
//!   `^[0-9A-Za-z.+-]{1,64}$`; `None` otherwise (no frontmatter, no such line, a line outside the
//!   frontmatter, a value that does not match);
//! - `state(installed_text: Option<&str>) -> State` with public fields `installed:
//!   Option<String>`, `binary: String` (the crate version) and `stale: bool`: no file gives
//!   (None, binary, false); a readable version equal to the binary's gives (Some(v), binary,
//!   false), any other (Some(v), binary, true); a file with no readable version gives
//!   (None, binary, true);
//! - `action(current: Option<&[u8]>, new: &[u8]) -> &'static str`: `"created"` for no current
//!   file, `"unchanged"` for equal bytes, `"updated"` otherwise.

mod goal {
    use hali::core::{guide, skill};

    const V: &str = env!("CARGO_PKG_VERSION");

    fn with_version(version: &str) -> String {
        format!(
            "---\nname: hali\ndescription: x\nmetadata:\n  hali-version: \"{version}\"\n\
             allowed-tools: Bash(hali find *)\n---\n# body\n"
        )
    }

    #[test]
    fn render_has_binary_version() {
        let md = skill::skill_md();
        assert!(md.starts_with("---\n"), "{}", &md[..md.len().min(40)]);
        assert!(
            md.contains(&format!("\n  hali-version: \"{V}\"\n")),
            "the frontmatter lacks the binary version {V}"
        );
        assert!(
            !md.contains("{{HALI_VERSION}}"),
            "the placeholder is still there"
        );
        assert_eq!(skill::installed_version(&md), Some(V.to_string()));

        let files = skill::files();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].0, "SKILL.md");
        assert_eq!(files[0].1, md);
        assert_eq!(files[1].0, "references/usage.md");
        assert_eq!(files[1].1, guide::text());
    }

    #[test]
    fn installed_version_parse() {
        assert_eq!(
            skill::installed_version(&with_version("9.9.9")),
            Some("9.9.9".to_string())
        );
        assert_eq!(
            skill::installed_version(&with_version("1.2.3-rc.1+build.5")),
            Some("1.2.3-rc.1+build.5".to_string())
        );
        let longest = "1".repeat(64);
        assert_eq!(
            skill::installed_version(&with_version(&longest)),
            Some(longest)
        );

        let none = [
            ("empty text", String::new()),
            ("a version with a space", with_version("1.0 beta")),
            ("an empty version", with_version("")),
            ("a version of 65 characters", with_version(&"1".repeat(65))),
            ("a path", with_version("../../etc")),
            ("a quote", with_version("1.0'x")),
            (
                "no hali-version line",
                "---\nname: hali\nmetadata:\n  other: \"1.0.0\"\n---\n".to_string(),
            ),
            (
                "no frontmatter",
                "  hali-version: \"1.0.0\"\nname: hali\n".to_string(),
            ),
            (
                "a version line after the frontmatter",
                "---\nname: hali\n---\nmetadata:\n  hali-version: \"1.0.0\"\n".to_string(),
            ),
            (
                "an unterminated frontmatter",
                "---\nname: hali\nmetadata:\n  hali-version: \"1.0.0\"\n".to_string(),
            ),
        ];
        for (what, text) in none {
            assert_eq!(skill::installed_version(&text), None, "{what}");
        }
    }

    #[test]
    fn state_cases() {
        let none = skill::state(None);
        assert_eq!(none.installed, None);
        assert_eq!(none.binary, V);
        assert!(!none.stale, "no file is not stale");

        let md = skill::skill_md();
        let current = skill::state(Some(&md));
        assert_eq!(current.installed.as_deref(), Some(V));
        assert_eq!(current.binary, V);
        assert!(!current.stale, "the current version is not stale");

        let other = skill::state(Some(&with_version("9.9.9")));
        assert_eq!(other.installed.as_deref(), Some("9.9.9"));
        assert_eq!(other.binary, V);
        assert!(other.stale, "another version is stale");

        for text in ["", "no frontmatter at all\n", "---\nname: hali\n---\n"] {
            let unreadable = skill::state(Some(text));
            assert_eq!(unreadable.installed, None, "{text:?}");
            assert_eq!(unreadable.binary, V, "{text:?}");
            assert!(unreadable.stale, "{text:?}: no readable version is stale");
        }
    }

    #[test]
    fn action_cases() {
        assert_eq!(skill::action(None, b"new"), "created");
        assert_eq!(skill::action(None, b""), "created");
        assert_eq!(skill::action(Some(b"new"), b"new"), "unchanged");
        assert_eq!(skill::action(Some(b""), b""), "unchanged");
        assert_eq!(skill::action(Some(b"old"), b"new"), "updated");
        assert_eq!(skill::action(Some(b""), b"new"), "updated");
        assert_eq!(skill::action(Some(b"new"), b""), "updated");
        assert_eq!(skill::action(Some(b"new "), b"new"), "updated");
    }
}
