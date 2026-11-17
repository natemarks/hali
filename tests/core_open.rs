//! Goal tests for the `editable` rule and the `[open]` config, in `core` (#11, spec §4.1, §5.2).
//!
//! Assumed API (invented here; the implementer follows it):
//!   `hali::core::config::OpenConfig { pub editor: Vec<MediaPattern>, pub system:
//!       Vec<MediaPattern> }`, `Default` (both empty), `Clone`, `Debug`.
//!   `hali::core::config::Config.open: OpenConfig`, read from the `[open]` table by `parse`.
//!   `hali::core::resource::editable(media_type: &str, open: &OpenConfig, is_text: impl
//!       Fn(&str) -> bool) -> bool`: the pure precedence rule. `is_text` answers "is this type
//!       `text/plain` or under it in the subclass tree" (the `media_type::is_text` seam); it is
//!       only asked when no config entry and no built-in exception decides.
//!   `hali::core::Context::with_open(self, OpenConfig) -> Context` and `Context::open(&self)
//!       -> &OpenConfig` (default: both lists empty).
//!
//! Precedence, highest first: `open.system` (false), `open.editor` (true), built-in
//! never-editable (false), then `is_text`. Config entries and built-ins match the stored media type exactly or by
//! `type/*`, case-insensitively; they never match subclasses or aliases. The built-ins are
//! `text/html`, `image/svg+xml`, `application/postscript`, `application/rtf` and
//! `application/x-desktop`, matched exactly. An empty or unparseable media type is not
//! editable (no config entry can match it).
//!
//! `[open] editor` and `[open] system` are optional lists of `type/subtype` or `type/*`
//! strings; anything else is `CONFIG_INVALID` (exit 3) whose fix text mentions
//! `[open] editor and system`.

#[cfg(feature = "goal")]
mod goal {
    use hali::core::config::{self, OpenConfig};
    use hali::core::resource::editable;
    use hali::core::{Context, FixedClock, SeededIdGen};
    use std::path::Path;

    /// Parses a config text with the given `[open]` body.
    fn parse_open(body: &str) -> Result<config::Config, hali::core::Error> {
        config::parse(body, Path::new("config.toml"))
    }

    fn open(editor: &[&str], system: &[&str]) -> OpenConfig {
        let list = |items: &[&str]| {
            let quoted: Vec<String> = items.iter().map(|i| format!("{i:?}")).collect();
            format!("[{}]", quoted.join(", "))
        };
        parse_open(&format!(
            "[open]\neditor = {}\nsystem = {}\n",
            list(editor),
            list(system)
        ))
        .unwrap()
        .open
    }

    fn never(_: &str) -> bool {
        false
    }

    fn always(_: &str) -> bool {
        true
    }

    // ---- criterion 1: is_a ------------------------------------------------------------------

    #[test]
    fn is_a_rule() {
        let none = OpenConfig::default();
        // With no config and no built-in exception, the answer is the `is_text` seam's.
        for ty in [
            "text/plain",
            "text/markdown",
            "application/json",
            "application/x-shellscript",
            "application/pdf",
            "application/octet-stream",
            "application/xhtml+xml",
        ] {
            assert!(editable(ty, &none, always), "{ty} with is_text true");
            assert!(!editable(ty, &none, never), "{ty} with is_text false");
        }
        // The seam is asked about the stored media type, as stored.
        let asked = std::cell::RefCell::new(Vec::new());
        editable("application/json", &none, |t| {
            asked.borrow_mut().push(t.to_string());
            true
        });
        assert_eq!(*asked.borrow(), ["application/json"]);
    }

    // ---- criterion 2: the five built-ins ----------------------------------------------------

    #[test]
    fn builtins_never_editable() {
        let none = OpenConfig::default();
        for ty in [
            "text/html",
            "image/svg+xml",
            "application/postscript",
            "application/rtf",
            "application/x-desktop",
            "Text/HTML",
            "IMAGE/SVG+XML",
        ] {
            assert!(
                !editable(ty, &none, always),
                "{ty} even when is_text is true"
            );
            assert!(!editable(ty, &none, never), "{ty}");
        }
        // Exact matches only: a near neighbour is decided by `is_text`.
        assert!(editable("application/xhtml+xml", &none, always));
        assert!(editable("text/htmlx", &none, always));
        assert!(editable("image/svg", &none, always));
    }

    // ---- criterion 3: precedence ------------------------------------------------------------

    #[test]
    fn precedence_matrix() {
        // (editor, system, media type, is_text, editable)
        type Row = (
            &'static [&'static str],
            &'static [&'static str],
            &'static str,
            bool,
            bool,
        );
        let rows: &[Row] = &[
            // editor beats a built-in exception, by exact entry and by `type/*`
            (&["text/html"], &[], "text/html", true, true),
            (&["image/*"], &[], "image/svg+xml", true, true),
            // editor beats a false is_text
            (&["application/pdf"], &[], "application/pdf", false, true),
            // system beats is_text, by exact entry and by `type/*`
            (&[], &["text/*"], "text/markdown", true, false),
            (&[], &["text/markdown"], "text/markdown", true, false),
            // a type in both lists goes to the OS opener
            (&["text/csv"], &["text/csv"], "text/csv", true, false),
            (&["text/*"], &["text/csv"], "text/csv", true, false),
            (&["text/csv"], &["text/*"], "text/csv", true, false),
            // matching is case-insensitive, on the entry and on the stored type
            (&["Text/HTML"], &[], "text/html", true, true),
            (&[], &["Text/CSV"], "text/csv", true, false),
            (&[], &["text/csv"], "TEXT/CSV", true, false),
            // entries match exactly or by type/*: not subclasses, not other subtypes
            (
                &["application/json5"],
                &[],
                "application/json",
                false,
                false,
            ),
            (&["text/*"], &[], "application/json", false, false),
            (&[], &["text/plain"], "text/markdown", true, true),
            (&["text/plain"], &[], "text/markdown", false, false),
            // an editor entry for another type changes nothing
            (&["application/pdf"], &[], "image/svg+xml", true, false),
            // the empty type: no entry can match it
            (&["text/*", "application/*"], &[], "", true, false),
            (&[], &[], "", true, false),
            (&["text/*"], &[], "nonsense", false, false),
        ];
        for (editor, system, ty, text, want) in rows {
            let cfg = open(editor, system);
            let got = editable(ty, &cfg, |_| *text);
            assert_eq!(
                got, *want,
                "editor {editor:?} system {system:?} type {ty:?} is_text {text}"
            );
        }
    }

    #[test]
    fn empty_type_not_editable() {
        assert!(!editable("", &OpenConfig::default(), always));
        assert!(!editable("", &open(&["text/*"], &[]), always));
    }

    // ---- criterion 6: the [open] config -----------------------------------------------------

    #[test]
    fn open_config_parses() {
        let cfg = parse_open("").unwrap();
        assert!(cfg.open.editor.is_empty() && cfg.open.system.is_empty());

        let cfg = parse_open("[open]\n").unwrap();
        assert!(cfg.open.editor.is_empty() && cfg.open.system.is_empty());

        let cfg = parse_open("[open]\neditor = []\nsystem = []\n").unwrap();
        assert!(cfg.open.editor.is_empty() && cfg.open.system.is_empty());

        let cfg = parse_open("[open]\neditor = [\"Text/HTML\", \"image/*\"]\n").unwrap();
        let shown: Vec<String> = cfg.open.editor.iter().map(|p| p.to_string()).collect();
        assert_eq!(shown, ["text/html", "image/*"]);
        assert!(cfg.open.system.is_empty());

        let cfg = parse_open("[open]\nsystem = [\"application/pdf\"]\n").unwrap();
        assert!(cfg.open.editor.is_empty());
        let shown: Vec<String> = cfg.open.system.iter().map(|p| p.to_string()).collect();
        assert_eq!(shown, ["application/pdf"]);

        // other tables are still read next to [open]
        let cfg = parse_open("[user]\nname = \"N\"\n[open]\neditor = [\"text/*\"]\n").unwrap();
        assert_eq!(cfg.user_name.as_deref(), Some("N"));
        assert_eq!(cfg.open.editor.len(), 1);
    }

    #[test]
    fn bad_open_config_is_config_invalid() {
        for text in [
            "[open]\neditor = \"text/plain\"\n",
            "[open]\nsystem = \"text/plain\"\n",
            "[open]\neditor = 5\n",
            "[open]\neditor = [1]\n",
            "[open]\nsystem = [true]\n",
            "[open]\neditor = [\"text/plain\", 2]\n",
            "[open]\neditor = [[\"text/plain\"]]\n",
            "[open]\neditor = [\"\"]\n",
            "[open]\nsystem = [\"\"]\n",
            "[open]\neditor = [\"text\"]\n",
            "[open]\nsystem = [\"text\"]\n",
            "[open]\neditor = [\"text/\"]\n",
            "[open]\neditor = [\"/plain\"]\n",
            "[open]\neditor = [\"*/*\"]\n",
            "[open]\nsystem = [\"*/*\"]\n",
            "[open]\neditor = [\"text/plain; charset=utf-8\"]\n",
            "[open]\nsystem = [\"text/plain;charset=utf-8\"]\n",
            "[open]\neditor = [\" text/plain\"]\n",
            "open = 5\n",
            "open = \"text/plain\"\n",
            "open = [\"text/plain\"]\n",
        ] {
            let err = parse_open(text)
                .err()
                .unwrap_or_else(|| panic!("accepted {text:?}"));
            assert_eq!(err.rule, "CONFIG_INVALID", "{text:?}");
            assert_eq!(err.exit_code(), 3, "{text:?}");
            let fix = err.suggested_fix.clone().unwrap_or_default();
            assert!(
                fix.contains("[open] editor and system"),
                "{text:?}: fix text is {fix:?}"
            );
        }
    }

    // ---- criterion 7: the Context carries [open] --------------------------------------------

    #[test]
    fn context_carries_open() {
        let ctx = Context::new(
            "en",
            Box::new(FixedClock::new("2026-10-24T09:30:00Z")),
            Box::new(SeededIdGen::new(1)),
        );
        assert!(ctx.open().editor.is_empty() && ctx.open().system.is_empty());
        let ctx = ctx.with_open(open(&["text/html"], &["text/csv"]));
        assert_eq!(ctx.open().editor.len(), 1);
        assert_eq!(ctx.open().system.len(), 1);
    }
}
