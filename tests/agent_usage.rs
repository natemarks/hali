//! Goal tests for `docs/agent-usage.md` (#27, spec §11.3, §9.6): the single-source agent usage
//! guide. The example world and the line runner are documented in tests/example_world/mod.rs.
//!
//! What counts as a `hali …` mention (the extraction rule; keep guide text to it):
//! - Fenced code blocks (a line whose trimmed text starts with three backticks opens or closes
//!   one): every line inside is a candidate. A leading `$ ` is dropped and a trailing `# …`
//!   comment is dropped (quote-aware, see the world's splitter); the line is a mention when it
//!   then starts with `hali ` or `HALI_AGENT=<name> hali `.
//! - Inline code spans outside fences: the text between a pair of single backticks on one line
//!   (a line with an odd number of backticks is not scanned), a mention when it starts with
//!   `hali ` or `HALI_AGENT=<name> hali `. A span that is just `hali` is not one.
//! - A mention parses with the clap tree (`try_get_matches_from`; a `DisplayHelp` result is
//!   success). Placeholders such as `<concept>` are fine, an ellipsis or a pipe is not (it
//!   would be an argument). The one exception: a mention whose words after `hali` are all among
//!   `--json`, `--dry-run`, `--yes` is a global-flag prefix (`hali --dry-run`) and is accepted
//!   without parsing.
//!
//! A runnable example is a line of a fenced block whose info string is exactly `sh`: blank lines
//! and lines starting `#` are skipped, every other line must be `[HALI_AGENT=<name>] hali …`
//! and is run in the world, in file order, in one world (no reset between lines), expecting the
//! exit code in a trailing `# exit N` comment (default 0). At least 5 such lines are required.
//!
//! The content checks look for fixed tokens, paragraph by paragraph (a paragraph is the run of
//! non-blank lines, joined by a space):
//! - workflow: `hali describe --json`, `hali find`, `hali show`, a backticked `path`,
//!   `hali --dry-run`, `changes`, `UNKNOWN_CONCEPT`, `suggested_command`, `suggested_fix`,
//!   `CONFIRMATION_REQUIRED`, `--yes`, `absolute`, `hali query`; each of the exit codes 0, 1, 2
//!   and 3 appears as a whole word on a line that also says `exit` (any case); `consent` or
//!   `approval` appears (case-insensitive: no `--yes` without it);
//! - identity (#171): `HALI_AGENT`, `--agent`, `self-asserted`, and one paragraph that holds
//!   `add`, `scan` and `new` each in backticks (the only commands that record the agent);
//! - scan roots (#169): `hali scan --list`, `hali scan --forget-root`, the span `hali scan`
//!   (bare), `roots.json`, and one paragraph holding `--prune --yes`, `forget` and `without
//!   asking`;
//! - hygiene: no §9.3-escaped character but LF, at most 300 lines, each snippet marker on a
//!   line of its own exactly once and nowhere else, and no mention of `skill install` (#28).

#[cfg(feature = "goal")]
mod example_world;

#[cfg(feature = "goal")]
mod goal {
    use crate::example_world::{
        SNIPPET_END, SNIPPET_START, World, expected_exit, guide_text, is_escaped, parse_line,
        parse_with_clap,
    };

    /// The mentions of the guide, with the line number they come from.
    fn mentions(text: &str) -> Vec<(usize, String)> {
        let mut found = Vec::new();
        let mut in_fence = false;
        for (i, line) in text.lines().enumerate() {
            let n = i + 1;
            if line.trim_start().starts_with("```") {
                in_fence = !in_fence;
                continue;
            }
            if in_fence {
                let mut code = line.trim();
                if let Some(rest) = code.strip_prefix("$ ") {
                    code = rest;
                }
                if is_mention(code) {
                    found.push((n, code.to_string()));
                }
            } else if line.matches('`').count() % 2 == 0 {
                for (k, span) in line.split('`').enumerate() {
                    if k % 2 == 1 && is_mention(span) {
                        found.push((n, span.to_string()));
                    }
                }
            }
        }
        found
    }

    fn is_mention(code: &str) -> bool {
        let code = match code.strip_prefix("HALI_AGENT=") {
            Some(rest) => match rest.split_once(' ') {
                Some((_, after)) => after.trim_start(),
                None => return false,
            },
            None => code,
        };
        code.starts_with("hali ")
    }

    #[test]
    fn every_mention_parses() {
        let text = guide_text();
        let found = mentions(&text);
        assert!(
            found.len() >= 10,
            "only {} hali mentions found",
            found.len()
        );
        let mut problems = Vec::new();
        for (n, mention) in &found {
            let line = match parse_line(mention, "/home/example") {
                Ok(l) => l,
                Err(e) => {
                    problems.push(format!("line {n}: {e}"));
                    continue;
                }
            };
            if line
                .args
                .iter()
                .all(|w| matches!(w.as_str(), "--json" | "--dry-run" | "--yes"))
            {
                continue;
            }
            if let Err(e) = parse_with_clap(&line.args) {
                problems.push(format!("line {n}: `{mention}` does not parse: {e}"));
            }
        }
        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }

    /// The commands of the guide's `sh` blocks: (line number, line).
    fn sh_lines(text: &str) -> Vec<(usize, String)> {
        let mut found = Vec::new();
        let mut in_sh = false;
        let mut in_other = false;
        for (i, line) in text.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("```") {
                if in_sh || in_other {
                    in_sh = false;
                    in_other = false;
                } else if trimmed == "```sh" {
                    in_sh = true;
                } else {
                    in_other = true;
                }
                continue;
            }
            if in_sh && !trimmed.is_empty() && !trimmed.starts_with('#') {
                found.push((i + 1, trimmed.to_string()));
            }
        }
        found
    }

    #[test]
    fn sh_examples_run() {
        let text = guide_text();
        let lines = sh_lines(&text);
        assert!(
            lines.len() >= 5,
            "only {} sh lines in the guide",
            lines.len()
        );
        let world = World::new();
        let mut problems = Vec::new();
        for (n, line) in &lines {
            match world.run_line(line) {
                Err(e) => problems.push(format!("line {n}: {e}")),
                Ok((parsed, out)) => match expected_exit(parsed.comment.as_deref()) {
                    Err(e) => problems.push(format!("line {n}: {e}")),
                    Ok(want) if out.code != Some(want) => problems.push(format!(
                        "line {n}: `{line}` exits {:?}, want {want}; stderr: {}",
                        out.code,
                        out.err().trim_end()
                    )),
                    Ok(_) => {}
                },
            }
        }
        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }

    fn paragraphs(text: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut cur: Vec<&str> = Vec::new();
        for line in text.lines() {
            if line.trim().is_empty() {
                if !cur.is_empty() {
                    out.push(cur.join(" "));
                    cur.clear();
                }
            } else {
                cur.push(line.trim());
            }
        }
        if !cur.is_empty() {
            out.push(cur.join(" "));
        }
        out
    }

    fn missing<'a>(text: &str, needles: &[&'a str]) -> Vec<&'a str> {
        needles
            .iter()
            .copied()
            .filter(|n| !text.contains(n))
            .collect()
    }

    fn has_word(line: &str, word: &str) -> bool {
        line.split(|c: char| !c.is_alphanumeric())
            .any(|w| w == word)
    }

    #[test]
    fn covers_the_agent_workflow() {
        let text = guide_text();
        let lacking = missing(
            &text,
            &[
                "hali describe --json",
                "hali find",
                "hali show",
                "`path`",
                "hali --dry-run",
                "changes",
                "UNKNOWN_CONCEPT",
                "suggested_command",
                "suggested_fix",
                "CONFIRMATION_REQUIRED",
                "--yes",
                "absolute",
                "hali query",
            ],
        );
        assert!(lacking.is_empty(), "the guide lacks {lacking:?}");
        for code in ["0", "1", "2", "3"] {
            assert!(
                text.lines()
                    .any(|l| l.to_lowercase().contains("exit") && has_word(l, code)),
                "no line mentions `exit` and the exit code {code}"
            );
        }
        let lower = text.to_lowercase();
        assert!(
            lower.contains("consent") || lower.contains("approval"),
            "the guide says nothing of the user's consent or approval"
        );
    }

    #[test]
    fn covers_agent_identity() {
        let text = guide_text();
        let lacking = missing(&text, &["HALI_AGENT", "--agent", "self-asserted"]);
        assert!(lacking.is_empty(), "the guide lacks {lacking:?}");
        assert!(
            paragraphs(&text)
                .iter()
                .any(|p| p.contains("`add`") && p.contains("`scan`") && p.contains("`new`")),
            "no paragraph names `add`, `scan` and `new` as the only commands that record the agent"
        );
    }

    #[test]
    fn covers_remembered_scan_roots() {
        let text = guide_text();
        let lacking = missing(
            &text,
            &[
                "hali scan --list",
                "hali scan --forget-root",
                "`hali scan`",
                "roots.json",
            ],
        );
        assert!(lacking.is_empty(), "the guide lacks {lacking:?}");
        assert!(
            paragraphs(&text).iter().any(|p| p.contains("--prune --yes")
                && p.contains("forget")
                && p.contains("without asking")),
            "no paragraph says that `scan --prune --yes` forgets without asking"
        );
    }

    #[test]
    fn guide_is_plain_and_bounded() {
        let text = guide_text();
        for (i, line) in text.lines().enumerate() {
            for c in line.chars() {
                assert!(
                    !is_escaped(c),
                    "line {}: the §9.3-escaped character {c:?}",
                    i + 1
                );
            }
        }
        assert!(!text.contains('\r'), "the guide holds a CR");
        let lines = text.lines().count();
        assert!(lines <= 300, "{lines} lines, at most 300");
        for marker in [SNIPPET_START, SNIPPET_END] {
            assert_eq!(text.matches(marker).count(), 1, "{marker} appears not once");
            assert_eq!(
                text.lines().filter(|l| *l == marker).count(),
                1,
                "{marker} is not on a line of its own"
            );
        }
        assert!(
            !text.contains("skill install"),
            "the guide mentions `skill install` (#28 has not landed)"
        );
    }
}
