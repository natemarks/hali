//! Goal tests for the examples at the top of every `--help` (#27, spec §11.1), over the real clap
//! tree (`hali::cli::command()`) and the real binary. The example world, the example-line
//! runner and the command classification are documented in tests/example_world/mod.rs.
//!
//! Assumed behaviour (the contract of issue #27, "Groomed contract"):
//! - every visible command (the root, each group and each leaf; not the hidden `generate`, and
//!   not `help` / `help agents`) has `before_help` of the form `Examples:\n` followed by 2 to 4
//!   lines `  hali …`; `hali <command> --help` and `-h` start with the same block, then a blank
//!   line;
//! - flag rules: a mutating command, a group and the root have at least one example with
//!   `--json` and at least one starting `hali --dry-run `; a read-only command has at least one
//!   with `--json`; `--dry-run` is always the word straight after `hali`;
//! - each example parses with the clap tree (a help request does not count here), and names the
//!   command it documents (or a descendant, for the root and the groups);
//! - each example exits 0 in a fresh copy of the example world;
//! - `help_examples_snapshot` pins every command path with its examples. It is not locked: the
//!   spec gives no expected text, so the implementer accepts the snapshot after review.

mod example_world;

mod goal {
    use crate::example_world::{
        BIN, Kind, Node, World, commands, example_block, parse_line, parse_with_clap,
    };

    const HOME: &str = "/home/example";

    /// Every example of every command: (node, example line).
    fn all_examples(nodes: &[Node]) -> Vec<(&Node, String)> {
        nodes
            .iter()
            .flat_map(|n| {
                n.examples()
                    .unwrap_or_default()
                    .into_iter()
                    .map(move |e| (n, e))
            })
            .collect()
    }

    fn assert_enough(nodes: &[Node], total: usize) {
        assert!(
            total >= 2 * nodes.len(),
            "{total} examples in all, but {} commands need at least 2 each ({} in all)",
            nodes.len(),
            2 * nodes.len()
        );
    }

    #[test]
    fn every_help_starts_with_examples() {
        let world = World::empty();
        let mut problems = Vec::new();
        for node in commands() {
            let from_tree = node.examples();
            for flag in ["--help", "-h"] {
                let mut args: Vec<&str> = node.path.iter().map(String::as_str).collect();
                args.push(flag);
                let out = world.run(&args);
                if out.code != Some(0) {
                    problems.push(format!("{} {flag}: exit {:?}", node.name(), out.code));
                    continue;
                }
                match example_block(&out.out()) {
                    Err(e) => problems.push(format!("{} {flag}: {e}", node.name())),
                    Ok((examples, rest)) => {
                        if !(2..=4).contains(&examples.len()) {
                            problems.push(format!(
                                "{} {flag}: {} examples, want 2 to 4",
                                node.name(),
                                examples.len()
                            ));
                        }
                        if !rest.starts_with('\n') {
                            problems.push(format!(
                                "{} {flag}: the block is not followed by a blank line",
                                node.name()
                            ));
                        }
                        if from_tree.as_ref().is_ok_and(|tree| *tree != examples) {
                            problems
                                .push(format!("{} {flag}: differs from before_help", node.name()));
                        }
                    }
                }
            }
            if let Err(e) = from_tree {
                problems.push(format!("{} before_help: {e}", node.name()));
            }
        }
        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }

    #[test]
    fn examples_follow_the_flag_rules() {
        let nodes = commands();
        let mut problems = Vec::new();
        for node in &nodes {
            let examples = node.examples().unwrap_or_default();
            let words: Vec<Vec<String>> = examples
                .iter()
                .map(|e| match parse_line(e, HOME) {
                    Ok(l) => l.args,
                    Err(err) => {
                        problems.push(format!("{}: {err}", node.name()));
                        Vec::new()
                    }
                })
                .collect();
            let json = words
                .iter()
                .filter(|a| a.iter().any(|w| w == "--json"))
                .count();
            let dry = examples
                .iter()
                .filter(|e| e.starts_with("hali --dry-run "))
                .count();
            if json == 0 {
                problems.push(format!(
                    "{} has {} examples, none with --json",
                    node.name(),
                    examples.len()
                ));
            }
            let needs_dry = matches!(node.kind, Kind::Root | Kind::Group | Kind::Mutating);
            if needs_dry && dry == 0 {
                problems.push(format!(
                    "{} ({:?}) has no example starting `hali --dry-run `",
                    node.name(),
                    node.kind
                ));
            }
            for (example, args) in examples.iter().zip(&words) {
                if args
                    .iter()
                    .position(|w| w == "--dry-run")
                    .is_some_and(|at| at != 0)
                {
                    problems.push(format!(
                        "{}: `{example}` has --dry-run, but not straight after `hali`",
                        node.name()
                    ));
                }
            }
        }
        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }

    #[test]
    fn examples_parse() {
        let nodes = commands();
        let examples = all_examples(&nodes);
        assert_enough(&nodes, examples.len());
        let mut problems = Vec::new();
        for (node, example) in &examples {
            let args = match parse_line(example, HOME) {
                Ok(l) if l.agent.is_none() => l.args,
                Ok(_) => {
                    problems.push(format!("{}: `{example}` sets HALI_AGENT", node.name()));
                    continue;
                }
                Err(e) => {
                    problems.push(format!("{}: {e}", node.name()));
                    continue;
                }
            };
            match parse_with_clap(&args) {
                Ok(Some(path)) => {
                    if !path.starts_with(&node.path) {
                        problems.push(format!(
                            "{}: `{example}` names `hali {}`, not the command it documents",
                            node.name(),
                            path.join(" ")
                        ));
                    }
                }
                Ok(None) => problems.push(format!(
                    "{}: `{example}` is a help request, not a use",
                    node.name()
                )),
                Err(e) => {
                    problems.push(format!("{}: `{example}` does not parse: {e}", node.name()))
                }
            }
        }
        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }

    #[test]
    fn examples_run() {
        let nodes = commands();
        let examples = all_examples(&nodes);
        assert_enough(&nodes, examples.len());
        let world = World::new();
        let mut problems = Vec::new();
        for (node, example) in &examples {
            world.reset();
            match world.run_line(example) {
                Err(e) => problems.push(format!("{}: {e}", node.name())),
                Ok((_, out)) => {
                    if out.code != Some(0) {
                        problems.push(format!(
                            "{}: `{example}` exits {:?}; stderr: {}",
                            node.name(),
                            out.code,
                            out.err().trim_end()
                        ));
                    }
                }
            }
        }
        assert!(problems.is_empty(), "{}", problems.join("\n"));
        let _ = BIN;
    }

    /// Not locked: the snapshot of every command path with its examples.
    #[test]
    fn help_examples_snapshot() {
        let mut text = String::new();
        for node in commands() {
            text.push_str(&format!("== {} ==\n", node.name()));
            text.push_str(&node.before_help());
            text.push_str("\n\n");
        }
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!("cli_help_examples__help_examples", text);
        });
    }
}
