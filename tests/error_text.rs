//! Goal tests for the text-mode rendering of errors (#73, spec §9.3 "Escaped for the terminal",
//! §9.4 text bullet), over the new public function
//!
//!     hali::cli::error_text(err: &hali::core::Error) -> String
//!
//! assumed here to return what text mode writes to stderr: `error: {message}\n`, then
//! `hint: {fix} Try: {command}\n` (or `hint: {fix}\n` with no command, nothing with no fix), with
//! `message`, `suggested_fix` and `suggested_command` each escaped separately (every character of
//! the escaped set becomes `\u{<lowercase hex>}`), so the output holds exactly one `error:` line
//! and one `hint:` line whatever the outside text. `details` is never printed.
//!
//! The escaped set is written out below from the contract, independently of the code under test.
//!
//! Wording the contract leaves open and these tests fix (change them, with the human, if the
//! wording should differ):
//! - `error_text` ends with a newline after the last line, as `eprintln!` would;
//! - the hostile text H is `a ESC ] 0 ; x BEL b LF "error: y" U+202E U+2028 U+061C U+FEFF U+200B c`;
//! - the kept set is checked with a list of sample characters (letters, CJK, emoji, combining
//!   marks, U+034F, VS16, U+FFFD, NBSP, U+200C, U+200D, and the neighbours just outside each
//!   escaped range, including U+3164, which the contract leaves alone on purpose).

mod goal {
    use hali::cli::error_text;
    use hali::core::{Error, flag_value, positional};
    use std::path::Path;

    /// Hostile text: an OSC title sequence, a forged `error:` line, a bidi override, a line
    /// separator, an Arabic letter mark, a BOM and a zero-width space.
    const H: &str = "a\u{1b}]0;x\u{7}b\nerror: y\u{202e}\u{2028}\u{61c}\u{feff}\u{200b}c";

    /// Whether the contract escapes `c` in text mode.
    fn is_escaped(c: char) -> bool {
        let u = u32::from(c);
        c.is_control() // C0 (TAB, LF, CR included), DEL, C1 (U+0085 included)
            || matches!(u,
                0x2028 | 0x2029                                   // line, paragraph separators
                | 0x061C | 0x200E | 0x200F                        // bidi marks
                | 0x202A..=0x202E | 0x2066..=0x2069               // bidi embeddings, isolates
                | 0x00AD | 0x180E | 0x200B                        // invisible format characters
                | 0x2060..=0x2064 | 0x206A..=0x206F
                | 0xFEFF | 0xFFF9..=0xFFFB
                | 0xE0000..=0xE007F) // tag characters
    }

    fn escape(c: char) -> String {
        format!("\\u{{{:x}}}", u32::from(c))
    }

    /// Every public `Error` constructor, each given `text` in every text argument. The bool is
    /// whether the error repeats outside text (so the escaped form of ESC must show).
    fn all_constructors(text: &str) -> Vec<(&'static str, bool, Error)> {
        let path = Path::new(text);
        vec![
            ("usage", true, Error::usage(text)),
            (
                "shacl_violation",
                false,
                Error::shacl_violation(&[], 1, true),
            ),
            (
                "shacl_violation_store",
                false,
                Error::shacl_violation(&[], 2, false),
            ),
            ("shapes_invalid", false, Error::shapes_invalid()),
            ("label_taken", true, Error::label_taken(text, text, text)),
            ("in_use", true, Error::in_use(text, 1, 1)),
            ("in_use_untagged", true, Error::in_use(text, 0, 1)),
            ("type_in_use", true, Error::type_in_use(text, 1, 1)),
            ("type_in_use_untyped", true, Error::type_in_use(text, 0, 1)),
            (
                "type_label_taken",
                true,
                Error::type_label_taken(text, text, text),
            ),
            ("relation_in_use", true, Error::relation_in_use(text, 1, 1)),
            (
                "relation_label_taken",
                true,
                Error::relation_label_taken(text, text, text),
            ),
            (
                "forget_blocked_by_relations",
                true,
                Error::forget_blocked_by_relations(text, 1, 1),
            ),
            ("store_io", true, Error::store_io(Some(path))),
            ("store_io_unresolvable", false, Error::store_io(None)),
            (
                "store_unparseable",
                true,
                Error::store_unparseable(text, 1, 1),
            ),
            ("config_invalid", true, Error::config_invalid(path, text)),
            (
                "confirmation_required",
                true,
                Error::confirmation_required(text, text, false),
            ),
            (
                "confirmation_required_declined",
                true,
                Error::confirmation_required(text, text, true),
            ),
            (
                "not_a_regular_file_missing",
                true,
                Error::not_a_regular_file(text, "missing"),
            ),
            (
                "not_a_regular_file_directory",
                true,
                Error::not_a_regular_file(text, "directory"),
            ),
            (
                "not_a_regular_file_other",
                true,
                Error::not_a_regular_file(text, "other"),
            ),
            ("non_utf8_path", true, Error::non_utf8_path(text)),
            ("file_unreadable", true, Error::file_unreadable(text)),
            ("unknown_resource", true, Error::unknown_resource(text)),
            ("unknown_concept", true, Error::unknown_concept(text, &[])),
            ("unknown_type", true, Error::unknown_type(text, &[])),
            ("unknown_relation", true, Error::unknown_relation(text, &[])),
        ]
    }

    /// Asserts the shape of `text` for an error with or without a fix: no escaped character
    /// except the structural newlines, exactly one `error:` line, one `hint:` line with a fix.
    fn assert_two_lines(name: &str, text: &str, has_fix: bool) {
        assert!(
            text.ends_with('\n'),
            "{name}: ends with a newline: {text:?}"
        );
        for c in text.chars().filter(|&c| c != '\n') {
            assert!(!is_escaped(c), "{name}: raw {c:?} in {text:?}");
        }
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines.len(),
            if has_fix { 2 } else { 1 },
            "{name}: lines of {text:?}"
        );
        assert!(lines[0].starts_with("error: "), "{name}: {text:?}");
        assert_eq!(
            text.lines().filter(|l| l.starts_with("error:")).count(),
            1,
            "{name}: one error: line in {text:?}"
        );
        assert_eq!(
            text.lines().filter(|l| l.starts_with("hint:")).count(),
            usize::from(has_fix),
            "{name}: hint: lines in {text:?}"
        );
    }

    #[test]
    fn every_constructor_escapes_outside_text() {
        for (name, echoes, err) in all_constructors(H) {
            let has_fix = err.suggested_fix.is_some();
            let text = error_text(&err);
            assert_two_lines(name, &text, has_fix);
            if echoes {
                assert!(
                    text.contains("\\u{1b}"),
                    "{name}: the escaped ESC is shown in {text:?}"
                );
            }
        }
    }

    #[test]
    fn hostile_text_in_every_field_of_one_error() {
        // The same checks with the hostile text planted straight into the three fields, and
        // without a fix (one line), without a command (no `Try:`).
        let mut err = Error::usage("x");
        err.message = H.to_string();
        let text = error_text(&err);
        assert_two_lines("usage", &text, false);
        assert_eq!(
            text,
            "error: a\\u{1b}]0;x\\u{7}b\\u{a}error: y\\u{202e}\\u{2028}\\u{61c}\\u{feff}\\u{200b}c\n"
        );

        let mut err = Error::file_unreadable("p");
        err.suggested_fix = Some(H.to_string());
        err.suggested_command = Some(H.to_string());
        let text = error_text(&err);
        assert_two_lines("file_unreadable", &text, true);
        let escaped_h =
            "a\\u{1b}]0;x\\u{7}b\\u{a}error: y\\u{202e}\\u{2028}\\u{61c}\\u{feff}\\u{200b}c";
        assert_eq!(
            text,
            format!("error: cannot read p\nhint: {escaped_h} Try: {escaped_h}\n")
        );

        err.suggested_command = None;
        assert_eq!(
            error_text(&err),
            format!("error: cannot read p\nhint: {escaped_h}\n")
        );
    }

    #[test]
    fn ordinary_text_is_unchanged() {
        for s in ["a;b", "it's", "café 日本", "👨‍👩‍👧"] {
            for (name, _, err) in all_constructors(s) {
                let mut expected = format!("error: {}\n", err.message);
                match (&err.suggested_fix, &err.suggested_command) {
                    (Some(fix), Some(cmd)) => {
                        expected.push_str(&format!("hint: {fix} Try: {cmd}\n"))
                    }
                    (Some(fix), None) => expected.push_str(&format!("hint: {fix}\n")),
                    (None, _) => {}
                }
                assert_eq!(error_text(&err), expected, "{name} with {s:?}");
            }
        }
        // One worked example written out, so the check above cannot agree with a wrong render.
        assert_eq!(
            error_text(&Error::unknown_concept("it's", &[])),
            "error: no Concept matches it's\nhint: Use the slug or a label of an existing Concept, or add the Concept first. Try: hali --dry-run concept add 'it'\\''s'\n"
        );
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
            let err = Error::unknown_resource(&c.to_string());
            let fix = err.suggested_fix.clone().unwrap();
            assert_eq!(
                error_text(&err),
                format!(
                    "error: no cataloged Resource matches {}\nhint: {fix}\n",
                    escape(c)
                ),
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
            '\u{034F}',  // combining grapheme joiner: a combining mark, kept
            '\u{FE0F}',  // variation selector-16
            '\u{FE00}',  // variation selector-1
            '\u{E0100}', // variation selector-17 (outside the tag range)
            '\u{180B}',  // Mongolian free variation selector (next to the escaped U+180E)
            '\u{FFFD}',  // replacement character
            '\u{FFFC}',  // object replacement character (next to U+FFFB)
            '\u{00A0}',  // no-break space
            '\u{00AC}',  // not sign (next to U+00AD)
            '\u{00AE}',  // registered sign (next to U+00AD)
            '\u{200A}',  // hair space (next to U+200B)
            '\u{200C}',  // zero width non-joiner: kept
            '\u{200D}',  // zero width joiner: kept
            '\u{2027}',  // hyphenation point (next to U+2028)
            '\u{202F}',  // narrow no-break space (next to U+202E)
            '\u{205F}',  // medium mathematical space (next to U+2060)
            '\u{2065}',  // unassigned, between the two invisible ranges
            '\u{3164}',  // Hangul filler: left alone on purpose
        ];
        for c in kept {
            assert!(!is_escaped(c), "the test's own set: {c:?}");
            let err = Error::unknown_resource(&c.to_string());
            let fix = err.suggested_fix.clone().unwrap();
            assert_eq!(
                error_text(&err),
                format!("error: no cataloged Resource matches {c}\nhint: {fix}\n"),
                "kept {c:?} (U+{:04X})",
                u32::from(c)
            );
        }
        // Emoji sequences keep their joiners and selectors.
        for s in [
            "👨\u{200D}👩\u{200D}👧",
            "❤\u{FE0F}",
            "🇳🇴",
            "می\u{200C}خواهم",
        ] {
            let err = Error::unknown_resource(s);
            assert!(
                error_text(&err)
                    .starts_with(&format!("error: no cataloged Resource matches {s}\n")),
                "{s:?}"
            );
        }
    }

    #[test]
    fn fix_prose_dash_ids() {
        // An id that starts with `-` must not read back as a flag in the fix's example command.
        let fix = Error::relation_in_use("-evil", 1, 0).suggested_fix.unwrap();
        assert!(
            fix.contains("hali unrelate <ref> -- -evil <ref>"),
            "fix contains \"-- -evil\": {fix}"
        );
        assert!(!fix.contains("<ref> -evil"), "{fix}");

        let fix = Error::in_use("-evil", 1, 1).suggested_fix.unwrap();
        assert!(fix.contains("--remove-tag=-evil"), "{fix}");
        assert!(fix.contains("--remove-broader=-evil"), "{fix}");
        assert!(!fix.contains("--remove-tag -evil") && !fix.contains("--remove-broader -evil"));

        let fix = Error::forget_blocked_by_relations("-evil", 1, 1)
            .suggested_fix
            .unwrap();
        assert!(
            fix.contains("<relation> -- -evil"),
            "fix contains \"-- -evil\": {fix}"
        );

        // Ordinary ids are unchanged: quoted as before, no `--`, no `=`.
        let fix = Error::relation_in_use("it's", 1, 0).suggested_fix.unwrap();
        assert!(
            fix.contains(&format!("hali unrelate <ref> {} <ref>", positional("it's"))),
            "{fix}"
        );
        assert!(fix.contains("'it'\\''s'"), "{fix}");
        let fix = Error::in_use("ml", 1, 1).suggested_fix.unwrap();
        assert!(fix.contains("--remove-tag ml)"), "{fix}");
        assert!(
            fix.contains("hali concept edit <C> --remove-broader ml)"),
            "{fix}"
        );
        let fix = Error::forget_blocked_by_relations("r1", 1, 1)
            .suggested_fix
            .unwrap();
        assert!(fix.contains("hali unrelate <ref> <relation> r1)"), "{fix}");
        assert_eq!(flag_value("--remove-tag", "-evil"), "--remove-tag=-evil");
    }
}
