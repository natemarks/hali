//! Goal test for `hali::core::guide` (#27, spec §11.3): the pure seam that embeds the agent
//! usage guide and cuts out its marked snippet.
//!
//! Assumed API (the contract of issue #27, "Groomed contract"):
//! - `guide::text() -> &'static str`: `docs/agent-usage.md`, byte for byte;
//! - `guide::snippet() -> &'static str`: the lines strictly between the line
//!   `<!-- hali:snippet:start -->` and the line `<!-- hali:snippet:end -->`, joined by LF, with
//!   no LF after the last (the CLI adds the trailing LF); non-empty, at most 25 lines, no
//!   marker in it, and it names `hali help agents`, `HALI_AGENT` and `hali --dry-run`.

#[cfg(feature = "goal")]
mod example_world;

#[cfg(feature = "goal")]
mod goal {
    use crate::example_world::{SNIPPET_END, SNIPPET_START, guide_text, snippet_section};
    use hali::core::guide;

    #[test]
    fn snippet_is_the_marked_section() {
        assert_eq!(guide::text(), guide_text(), "text() is the guide file");
        let expected = snippet_section(&guide_text())
            .unwrap_or_else(|| panic!("the guide has no {SNIPPET_START} .. {SNIPPET_END} section"));
        let snippet = guide::snippet();
        assert_eq!(snippet, expected, "snippet() is exactly the marked section");
        assert!(!snippet.trim().is_empty(), "the snippet is empty");
        assert!(
            snippet.split('\n').count() <= 25,
            "the snippet is over 25 lines"
        );
        assert!(
            !snippet.contains("hali:snippet:"),
            "the snippet holds a marker"
        );
        for needle in ["hali help agents", "HALI_AGENT", "hali --dry-run"] {
            assert!(snippet.contains(needle), "the snippet lacks `{needle}`");
        }
    }
}
