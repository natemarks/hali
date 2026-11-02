//! Goal tests for the text rendering of clap usage errors (#117, spec §9.3 "Escaped for the
//! terminal", §9.4 text bullet), over two public functions of `hali::cli`:
//!
//!     hali::cli::usage_text(err: clap::Error) -> String
//!     hali::cli::command() -> clap::Command      (already exists; to be built with plain styles)
//!
//! `usage_text` is assumed to return what text mode writes to stderr for a usage error: clap's
//! layout without colour (`error:` line, any `tip:` line, the `Usage:` block, `For more
//! information, try '--help'.`), ending with a newline, with every argument clap repeats, and the
//! rest of clap's text, escaped (every character of the §9.3 set becomes `\u{<lowercase hex>}`)
//! except the line breaks clap itself writes. Errors come from `command().try_get_matches_from(..)`.
//!
//! The escaped set is the #73 list, written out below independently of the code under test; it
//! must not change here (#120 may change it later).
//!
//! Wording the contract leaves open and these tests fix (change them, with the human, if the
//! wording should differ):
//! - the probe argv is `hali find --limit zz<c>`: the value parser rejects it with
//!   `invalid digit found in string`, and clap repeats the value in the `error:` line;
//! - `matches_stripped_clap` compares with the same command under `Styles::styled()`, rendered
//!   through `Display` (which strips clap's colour codes);
//! - a styled error given to `usage_text` shows clap's own colour codes escaped, never raw.

#[cfg(feature = "goal")]
mod goal {
    use clap::builder::Styles;
    use hali::cli::{command, usage_text};

    /// Whether the contract escapes `c` (the #73 list, LF included).
    fn is_escaped(c: char) -> bool {
        let u = u32::from(c);
        c.is_control()
            || matches!(u,
                0x2028 | 0x2029
                | 0x061C | 0x200E | 0x200F
                | 0x202A..=0x202E | 0x2066..=0x2069
                | 0x00AD | 0x180E | 0x200B
                | 0x2060..=0x2064 | 0x206A..=0x206F
                | 0xFEFF | 0xFFF9..=0xFFFB
                | 0xE0000..=0xE007F)
    }

    fn escape(c: char) -> String {
        format!("\\u{{{:x}}}", u32::from(c))
    }

    /// The error `find --limit <value>` raises.
    fn limit_error(value: &str) -> clap::Error {
        command()
            .try_get_matches_from(["hali", "find", "--limit", value])
            .unwrap_err()
    }

    /// What `usage_text` returns for that error, with `shown` standing for the value.
    fn limit_text(shown: &str) -> String {
        format!(
            "error: invalid value '{shown}' for '--limit <LIMIT>': invalid digit found in string\n\nFor more information, try '--help'.\n"
        )
    }

    #[test]
    fn escape_set() {
        let mut escaped: Vec<char> = Vec::new();
        for range in [
            0x0000..=0x001F,
            0x007F..=0x009F,
            0x2028..=0x2029,
            0x061C..=0x061C,
            0x200E..=0x200F,
            0x202A..=0x202E,
            0x2066..=0x2069,
            0x00AD..=0x00AD,
            0x180E..=0x180E,
            0x200B..=0x200B,
            0x2060..=0x2064,
            0x206A..=0x206F,
            0xFEFF..=0xFEFF,
            0xFFF9..=0xFFFB,
            0xE0000..=0xE007F,
        ] {
            escaped.extend(range.map(|u| char::from_u32(u).unwrap()));
        }
        assert_eq!(
            escaped.len(),
            32 + 33 + 2 + 1 + 2 + 5 + 4 + 1 + 1 + 1 + 5 + 6 + 1 + 3 + 128
        );
        for c in escaped {
            assert!(is_escaped(c), "the test's own set: {c:?}");
            let text = usage_text(limit_error(&format!("zz{c}")));
            assert_eq!(
                text,
                limit_text(&format!("zz{}", escape(c))),
                "escaped {c:?} ({})",
                escape(c)
            );
        }
    }

    #[test]
    fn kept_set() {
        let kept = [
            'a',
            'Z',
            '0',
            ' ',
            '~',
            '\u{00A0}', // no-break space
            'é',
            'ß',
            '日',
            'ひ',
            '한',
            'א',
            'م',
            '😀',
            '👍',
            '\u{0301}',  // combining acute accent
            '\u{034F}',  // combining grapheme joiner: kept
            '\u{FE0F}',  // variation selector-16
            '\u{FE00}',  // variation selector-1
            '\u{E0100}', // variation selector-17 (outside the tag range)
            '\u{FFFD}',  // replacement character
            '\u{200C}',  // zero width non-joiner: kept
            '\u{200D}',  // zero width joiner: kept
            '\u{3164}',  // Hangul filler: left alone on purpose
            // The neighbours just outside each escaped range.
            '\u{00AC}', // next to U+00AD
            '\u{00AE}',
            '\u{061B}', // next to U+061C
            '\u{061D}',
            '\u{180B}', // Mongolian free variation selectors, next to U+180E
            '\u{180D}',
            '\u{180F}',
            '\u{200A}', // next to U+200B
            '\u{2027}', // next to U+2028
            '\u{202F}', // next to U+202E
            '\u{205F}', // next to U+2060
            '\u{2065}', // unassigned, between two escaped ranges
            '\u{2070}', // next to U+206F
            '\u{FEFE}', // next to U+FEFF
            '\u{FF00}',
            '\u{FFF8}',  // next to U+FFF9
            '\u{FFFC}',  // next to U+FFFB
            '\u{DFFFF}', // next to U+E0000
            '\u{E0080}', // next to U+E007F
        ];
        for c in kept {
            assert!(!is_escaped(c), "the test's own set: {c:?}");
            let text = usage_text(limit_error(&format!("zz{c}")));
            assert_eq!(
                text,
                limit_text(&format!("zz{c}")),
                "kept {c:?} (U+{:04X})",
                u32::from(c)
            );
        }
        // Emoji and Persian sequences keep their joiners and selectors.
        for s in [
            "👨\u{200D}👩\u{200D}👧",
            "❤\u{FE0F}",
            "🇳🇴",
            "می\u{200C}خواهم",
        ] {
            assert_eq!(usage_text(limit_error(s)), limit_text(s), "{s:?}");
        }
    }

    #[test]
    fn matches_stripped_clap() {
        let styled = command().styles(Styles::styled());
        for argv in [
            vec!["hali", "--bogus"],
            vec!["hali", "fnd"],
            vec!["hali", "concept", "add"],
            vec!["hali", "edit", "x"],
            vec!["hali", "describe", "extra"],
            vec!["hali", "refresh"],
            vec!["hali", "refresh", "--all", "R1"],
            vec!["hali", "find", "--limit", "zz"],
            // clap's `tip: to pass '<arg>' as a value` line (ContextKind::Suggested).
            vec!["hali", "schema", "--bogus"],
            vec!["hali", "concept", "show", "--bogus"],
            vec!["hali", "edit", "--bogus"],
        ] {
            let plain = usage_text(command().try_get_matches_from(&argv).unwrap_err());
            let stripped = styled
                .clone()
                .try_get_matches_from(&argv)
                .unwrap_err()
                .render()
                .to_string();
            assert_eq!(plain, stripped, "{argv:?}");
        }
        // The usage block stays on its own lines.
        let text = usage_text(
            command()
                .try_get_matches_from(["hali", "concept", "add"])
                .unwrap_err(),
        );
        assert_eq!(
            text,
            "error: the following required arguments were not provided:\n  <LABEL>\n\nUsage: hali concept add <LABEL>\n\nFor more information, try '--help'.\n"
        );
    }

    #[test]
    fn newline_in_value() {
        let text = usage_text(limit_error("zz\nerror: forged"));
        assert_eq!(text, limit_text("zz\\u{a}error: forged"));
        assert_eq!(text.lines().count(), limit_text("zz").lines().count());
        // An unknown subcommand repeats its argument too.
        let text = usage_text(
            command()
                .try_get_matches_from(["hali", "zz\nerror: forged"])
                .unwrap_err(),
        );
        assert_eq!(
            text,
            "error: unrecognized subcommand 'zz\\u{a}error: forged'\n\nUsage: hali [OPTIONS] <COMMAND>\n\nFor more information, try '--help'.\n"
        );
    }

    #[test]
    fn styled_error_stays_safe() {
        // A styled command's error carries clap's own colour codes; they must not reach the
        // terminal raw.
        let err = command()
            .styles(Styles::styled())
            .try_get_matches_from(["hali", "zz\u{1b}]0;x\u{7}"])
            .unwrap_err();
        let text = usage_text(err);
        assert!(!text.contains('\u{1b}'), "raw ESC in {text:?}");
        assert!(text.contains("\\u{1b}]0;x\\u{7}"), "{text:?}");
    }

    /// Escapes every character of the contract's set, as `usage_text` shows it.
    fn escape_str(s: &str) -> String {
        s.chars()
            .map(|c| {
                if is_escaped(c) {
                    escape(c)
                } else {
                    c.to_string()
                }
            })
            .collect()
    }

    fn stripped_clap(argv: &[String]) -> String {
        command()
            .styles(Styles::styled())
            .try_get_matches_from(argv)
            .unwrap_err()
            .render()
            .to_string()
    }

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    /// clap's `tip: to pass '<arg>' as a value, use '-- <arg>'` line survives, escaped.
    #[test]
    fn tip_line_escaped() {
        let ordinary = usage_text(
            command()
                .try_get_matches_from(["hali", "schema", "--bogus"])
                .unwrap_err(),
        );
        assert!(
            ordinary.contains("\n  tip: to pass '--bogus' as a value, use '-- --bogus'\n"),
            "{ordinary:?}"
        );
        let hostile = "--a\u{1b}]0;x\u{7}";
        let text = usage_text(
            command()
                .try_get_matches_from(["hali", "schema", hostile])
                .unwrap_err(),
        );
        assert!(!text.contains('\u{1b}'), "raw ESC in {text:?}");
        let shown = "--a\\u{1b}]0;x\\u{7}";
        assert!(
            text.contains(&format!(
                "\n  tip: to pass '{shown}' as a value, use '-- {shown}'\n"
            )),
            "{text:?}"
        );
    }

    /// One argv per clap `ContextKind` this command tree can emit, with the hostile value in
    /// each position it can occupy. Ordinary values equal clap's own text without colour;
    /// hostile ones leave no character of the escaped set (but LF) and add no line.
    ///
    /// Kinds, and where the table reaches them:
    /// - InvalidSubcommand: `hali X`; SuggestedSubcommand: `hali fnd` (the suggestion comes from
    ///   the command names, never argv, so it only has an ordinary case);
    /// - InvalidArg: `hali --X`, `hali find --X`; SuggestedArg: `hali find --lmit` (likewise);
    /// - Suggested (the `tip:` line): `schema --X`, `concept show --X`, `edit --X`;
    /// - InvalidValue and ValidValue: `find --limit X`, `find --sort X`; SuggestedValue:
    ///   `find --sort modifid` (from the valid values, so ordinary only);
    /// - PriorArg: `refresh --all R1` (the text names the other argument, never argv, so ordinary only);
    ///   Usage: `concept add`, `describe X`;
    /// - Custom domain messages (InvalidValue sources): `concept add X`, `type add X`,
    ///   `relation add X`.
    ///
    /// Not emittable here: ActualNumValues, ExpectedNumValues and MinValues (no argument sets
    /// `num_args`), TrailingArg (no `trailing_var_arg`), SuggestedCommand (clap raises it only
    /// for a built-in `help` misuse this tree cannot reach through argv).
    #[test]
    fn every_context_kind() {
        type Build = fn(&str) -> Vec<String>;
        let table: Vec<(&str, Build)> = vec![
            ("InvalidSubcommand", |x| argv(&["hali", x])),
            ("InvalidSubcommand (nested)", |x| {
                argv(&["hali", "concept", x])
            }),
            ("InvalidArg", |x| vec!["hali".into(), format!("--{x}")]),
            ("InvalidArg (nested)", |x| {
                vec!["hali".into(), "find".into(), format!("--{x}")]
            }),
            ("Suggested schema", |x| {
                vec!["hali".into(), "schema".into(), format!("--{x}")]
            }),
            ("Suggested concept show", |x| {
                vec![
                    "hali".into(),
                    "concept".into(),
                    "show".into(),
                    format!("--{x}"),
                ]
            }),
            ("Suggested edit", |x| {
                vec!["hali".into(), "edit".into(), format!("--{x}")]
            }),
            ("InvalidValue limit", |x| {
                argv(&["hali", "find", "--limit", x])
            }),
            ("InvalidValue/ValidValue sort", |x| {
                argv(&["hali", "find", "--sort", x])
            }),
            ("Usage describe", |x| argv(&["hali", "describe", x])),
            ("Custom concept add", |x| {
                argv(&["hali", "concept", "add", x])
            }),
            ("Custom type add", |x| argv(&["hali", "type", "add", x])),
            ("Custom relation add", |x| {
                argv(&["hali", "relation", "add", x])
            }),
        ];
        for (label, build) in &table {
            // Ordinary value: clap's own text.
            // (The domain parsers accept "bogus"; these are their ordinary rejections.)
            let ordinary = match *label {
                "Custom type add" | "Custom relation add" => "1x",
                "Custom concept add" => "",
                _ => "bogus",
            };
            let v = build(ordinary);
            let plain = usage_text(command().try_get_matches_from(&v).unwrap_err());
            assert_eq!(plain, stripped_clap(&v), "{label} {v:?}");
            // Hostile values: no raw character of the set, the escaped text shown, no extra line.
            for hostile in [
                "a\u{1b}]0;x\u{7}b",
                "a\u{9b}\u{202e}\u{200b}\u{e0041}b",
                "a\nerror: forged",
            ] {
                let v = build(hostile);
                let text = usage_text(command().try_get_matches_from(&v).unwrap_err());
                for c in text.chars().filter(|&c| c != '\n') {
                    assert!(!is_escaped(c), "{label}: raw {c:?} in {text:?}");
                }
                assert!(
                    text.contains(&escape_str(hostile)),
                    "{label}: {:?} not in {text:?}",
                    escape_str(hostile)
                );
                assert_eq!(
                    text.lines().count(),
                    plain.lines().count(),
                    "{label} {hostile:?}: {text:?}"
                );
            }
        }
        // Kinds whose value comes from the command tree itself: ordinary only.
        for v in [
            argv(&["hali", "fnd"]),
            argv(&["hali", "find", "--lmit"]),
            argv(&["hali", "find", "--sort", "modifid"]),
            argv(&["hali", "refresh", "--all", "R1"]),
            argv(&["hali", "concept", "add"]),
        ] {
            let plain = usage_text(command().try_get_matches_from(&v).unwrap_err());
            assert_eq!(plain, stripped_clap(&v), "{v:?}");
        }
    }
}
