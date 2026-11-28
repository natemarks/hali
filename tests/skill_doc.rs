//! Goal tests for the rendered skill, `hali skill install --print` (#28, spec §11.2). The text
//! is read through the real binary with a cleared environment (no `HOME`: `--print` resolves no
//! directory), so the tests see what an agent installs.
//!
//! What they parse (no YAML library: the frontmatter is five fixed lines):
//! - the text starts with `---`; the frontmatter is the lines up to the next `---` line, and is
//!   exactly `name: hali`, `description: <one line, at most 1024 characters>`, `metadata:`,
//!   `  hali-version: "<crate version>"`, `allowed-tools: <the eight rules>` in that order;
//! - the description starts `Find, open, tag or catalog local files and documents;` and holds
//!   `ontology`, `concepts`, `types`, `related files` and `hali`;
//! - `allowed-tools` is, on one line, comma-and-space separated, exactly
//!   `Bash(hali describe *)`, `find`, `show`, `query`, `schema`, `validate`, `help` (each
//!   `Bash(hali <command> *)`) and `Bash(hali --dry-run *)`; no rule names a writing command
//!   (`skill` included: a rule may only name a top-level read-only command of the clap tree or
//!   `help`), and `--dry-run` is only ever the prefix rule;
//! - the body (after the closing `---`) is under 200 lines, holds the fixed words of the eight
//!   §11.2 points, links `references/usage.md`, every `hali …` mention (fenced lines and inline
//!   code spans, the extraction rule of tests/agent_usage.rs) parses with the clap tree, the word
//!   `--dry-run` only ever follows `hali ` directly, and `--yes` appears only in a paragraph that
//!   says `never`, `consent` or `approval`.

#[cfg(feature = "goal")]
mod example_world;

#[cfg(feature = "goal")]
mod goal {
    use crate::example_world::{Kind, commands, parse_line, parse_with_clap};
    use std::process::Command;

    const VERSION: &str = env!("CARGO_PKG_VERSION");

    const RULES: [&str; 8] = [
        "Bash(hali describe *)",
        "Bash(hali find *)",
        "Bash(hali show *)",
        "Bash(hali query *)",
        "Bash(hali schema *)",
        "Bash(hali validate *)",
        "Bash(hali help *)",
        "Bash(hali --dry-run *)",
    ];

    /// `hali skill install --print` stdout, with nothing from the environment.
    fn rendered() -> String {
        let cwd = tempfile::tempdir().unwrap();
        let out = Command::new(env!("CARGO_BIN_EXE_hali"))
            .env_clear()
            .current_dir(cwd.path())
            .args(["skill", "install", "--print"])
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).expect("the skill is UTF-8")
    }

    /// (frontmatter lines, body) of a rendered skill.
    fn split(text: &str) -> (Vec<&str>, &str) {
        let rest = text
            .strip_prefix("---\n")
            .unwrap_or_else(|| panic!("the skill does not start with a `---` line"));
        let mut lines = Vec::new();
        let mut consumed = 0;
        for line in rest.split_inclusive('\n') {
            consumed += line.len();
            let bare = line.trim_end_matches('\n');
            if bare == "---" {
                return (lines, &rest[consumed..]);
            }
            lines.push(bare);
        }
        panic!("the frontmatter is not closed by a `---` line");
    }

    #[test]
    fn frontmatter() {
        let text = rendered();
        let (fm, _) = split(&text);
        assert_eq!(fm.len(), 5, "the frontmatter is five lines: {fm:?}");
        assert_eq!(fm[0], "name: hali");
        let description = fm[1]
            .strip_prefix("description: ")
            .unwrap_or_else(|| panic!("line 2 is not `description: …`: {:?}", fm[1]));
        assert!(
            description.chars().count() <= 1024,
            "the description is {} characters, at most 1024",
            description.chars().count()
        );
        assert!(
            description.starts_with("Find, open, tag or catalog local files and documents;"),
            "the description does not lead with the trigger words: {description:?}"
        );
        for needle in ["ontology", "concepts", "types", "related files", "hali"] {
            assert!(
                description.contains(needle),
                "the description lacks `{needle}`: {description:?}"
            );
        }
        assert_eq!(fm[2], "metadata:");
        assert_eq!(fm[3], format!("  hali-version: \"{VERSION}\""));
        assert_eq!(fm[4], format!("allowed-tools: {}", RULES.join(", ")));
        assert!(
            !text.contains("{{HALI_VERSION}}"),
            "the placeholder is left"
        );
    }

    #[test]
    fn allowed_tools_read_only_prefix_only() {
        let text = rendered();
        let (fm, _) = split(&text);
        let line = fm
            .iter()
            .find_map(|l| l.strip_prefix("allowed-tools: "))
            .expect("no allowed-tools line");
        let rules: Vec<&str> = line.split(", ").collect();
        assert_eq!(rules, RULES, "exactly the eight rules of spec §11.2");

        // The read-only top-level commands of the clap tree.
        let mut read_only: Vec<String> = commands()
            .iter()
            .filter(|n| n.kind == Kind::ReadOnly && n.path.len() == 1)
            .map(|n| n.path[0].clone())
            .collect();
        read_only.push("help".to_string());
        for rule in &rules {
            let inner = rule
                .strip_prefix("Bash(hali ")
                .and_then(|r| r.strip_suffix(" *)"))
                .unwrap_or_else(|| panic!("rule {rule:?} is not `Bash(hali <word> *)`"));
            if rule.contains("--dry-run") {
                assert_eq!(*rule, "Bash(hali --dry-run *)", "a --dry-run rule");
            } else {
                assert!(
                    read_only.iter().any(|c| c == inner),
                    "rule {rule:?} names `{inner}`, which is not a read-only command ({read_only:?})"
                );
            }
            assert!(!rule.contains("skill"), "a rule names skill: {rule:?}");
        }
        // `--dry-run` appears in the frontmatter only as the prefix rule.
        for l in &fm {
            for (at, _) in l.match_indices("--dry-run") {
                assert!(
                    l[..at].ends_with("Bash(hali "),
                    "`--dry-run` outside the prefix rule: {l:?}"
                );
            }
        }
    }

    /// The `hali …` mentions of `text` (the extraction rule of tests/agent_usage.rs).
    fn mentions(text: &str) -> Vec<(usize, String)> {
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

    fn paragraphs(text: &str) -> Vec<String> {
        text.split("\n\n")
            .map(|p| p.lines().collect::<Vec<_>>().join(" "))
            .collect()
    }

    /// Review round 1: an env-var assignment before a read-only command would not match the
    /// `Bash(hali describe *)` rules, so the agent is told to name itself with `--agent <name>`
    /// on write commands only. Glossary: a Concept is never a "tag", and hali does not write
    /// "only its own store".
    #[test]
    fn body_agent_naming_and_glossary() {
        let text = rendered();
        let (_, body) = split(&text);
        let read_only: Vec<String> = commands()
            .iter()
            .filter(|n| n.kind == Kind::ReadOnly && n.path.len() == 1)
            .map(|n| n.path[0].clone())
            .collect();
        let mut problems = Vec::new();
        for (i, line) in body.lines().enumerate() {
            for (at, _) in line.match_indices("HALI_AGENT=") {
                let after_assign = line[at..].split_once(' ').map_or("", |(_, rest)| rest);
                if let Some(rest) = after_assign.trim_start().strip_prefix("hali ") {
                    let command = rest.split_whitespace().find(|w| !w.starts_with("--"));
                    if command.is_some_and(|c| read_only.iter().any(|r| r == c)) {
                        problems.push(format!(
                            "body line {}: a read-only command starts with an env-var assignment: {line:?}",
                            i + 1
                        ));
                    }
                }
            }
        }
        assert!(problems.is_empty(), "{}", problems.join("\n"));

        let found = mentions(body);
        assert!(
            found
                .iter()
                .any(|(_, m)| { m.starts_with("hali --dry-run ") && m.contains(" --agent ") }),
            "no `hali --dry-run … --agent <name>` example for a write command"
        );
        assert!(
            found.iter().any(|(_, m)| {
                m.starts_with("hali ")
                    && !m.starts_with("hali --dry-run")
                    && m.contains(" --agent ")
                    && !read_only
                        .iter()
                        .any(|r| m.split_whitespace().nth(1) == Some(r.as_str()))
            }),
            "no `hali <write command> … --agent <name>` example"
        );
        assert!(
            body.contains("--agent <name>") || body.contains("`--agent`"),
            "the body does not tell agents to pass `--agent <name>`"
        );

        // A Concept is not a "tag" (flag names such as `--tag` and `--add-tag` are fine).
        for (i, line) in body.lines().enumerate() {
            let words = line.split(|c: char| !(c.is_alphanumeric() || c == '-'));
            for w in words {
                let w = w.to_lowercase();
                if (w == "tag" || w == "tags") && !w.starts_with('-') {
                    problems.push(format!(
                        "body line {}: a Concept is not a \"tag\": {line:?}",
                        i + 1
                    ));
                }
            }
        }
        assert!(problems.is_empty(), "{}", problems.join("\n"));
        let flat = body.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            !flat.contains("only its own store") && !flat.contains("writes only its own"),
            "hali also writes the files `hali new` creates and the skill files"
        );
    }

    #[test]
    fn body() {
        let text = rendered();
        let (_, body) = split(&text);
        let lines = body.lines().count();
        assert!(lines < 200, "the body is {lines} lines, under 200");
        assert!(lines >= 20, "the body is only {lines} lines");

        // The eight §11.2 points, as fixed words.
        for needle in [
            "HALI_AGENT",
            "hali describe --json",
            "hali find",
            "hali show",
            "`path`",
            "hali --dry-run",
            "changes",
            "UNKNOWN_CONCEPT",
            "concept add",
            "suggested_command",
            "absolute",
            "hali query",
            "references/usage.md",
        ] {
            assert!(body.contains(needle), "the body lacks `{needle}`");
        }
        let lower = body.to_lowercase();
        assert!(
            lower.contains("user") && lower.contains("approval"),
            "the body does not say the user's approval comes first"
        );

        // Every mention parses with the clap tree.
        let found = mentions(body);
        assert!(found.len() >= 8, "only {} hali mentions", found.len());
        let mut problems = Vec::new();
        for (n, mention) in &found {
            let line = match parse_line(mention, "/home/example") {
                Ok(l) => l,
                Err(e) => {
                    problems.push(format!("body line {n}: {e}"));
                    continue;
                }
            };
            if line
                .args
                .iter()
                .all(|w| matches!(w.as_str(), "--json" | "--dry-run" | "--yes" | "*"))
            {
                continue;
            }
            if let Err(e) = parse_with_clap(&line.args) {
                problems.push(format!("body line {n}: `{mention}` does not parse: {e}"));
            }
        }
        assert!(problems.is_empty(), "{}", problems.join("\n"));

        // `--dry-run` only straight after `hali`.
        for (i, line) in body.lines().enumerate() {
            for (at, _) in line.match_indices("--dry-run") {
                assert!(
                    line[..at].ends_with("hali "),
                    "body line {}: `--dry-run` not straight after `hali`: {line:?}",
                    i + 1
                );
            }
        }

        // `--yes` only where the text says never, consent or approval.
        for p in paragraphs(body) {
            if p.contains("--yes") {
                let p_lower = p.to_lowercase();
                assert!(
                    p_lower.contains("never")
                        || p_lower.contains("consent")
                        || p_lower.contains("approval"),
                    "a paragraph names `--yes` without never, consent or approval: {p:?}"
                );
            }
        }
    }
}
