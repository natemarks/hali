//! Goal tests for `hali help agents [--snippet]` and the preserved `help` forms (#27, spec §9.1,
//! §9.6, §11.3), through the real binary. Every run clears the environment and points HOME,
//! XDG_*, HALI_DATA_DIR and GIT_CONFIG_* at a temp dir (see tests/example_world/mod.rs); the
//! real `~/.local/share/hali`, `~/.config/hali` and `~/.claude` are never touched.
//!
//! Assumed behaviour (the contract of issue #27, "Groomed contract"):
//! - `help agents` prints `docs/agent-usage.md` byte for byte, exit 0, empty stderr;
//!   `help agents --snippet` prints the section between the snippet markers plus one LF;
//! - `help agents` behaves like `generate`: it ignores `--json`, `--dry-run`, `--yes` and
//!   `--agent` (before or after), reads no `HALI_AGENT`, config, store or state, and creates
//!   nothing, whatever the environment, cwd, config or data dir;
//! - `help agents extra` is a usage error naming `unexpected argument 'extra'` (exit 2); a
//!   closed stdout exits 1 with nothing on stderr;
//! - `help`, `help <command>…` and `help nope` behave as before; the root help's `help` row
//!   names `agents`.

#[cfg(feature = "goal")]
mod example_world;

#[cfg(feature = "goal")]
mod goal {
    use crate::example_world::{
        BIN, Out, SNIPPET_END, SNIPPET_START, World, guide_bytes, is_escaped, snippet_section,
    };
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    use std::process::Stdio;

    /// Asserts a clean `help agents` run: exit 0, empty stderr, the guide on stdout.
    fn assert_guide(label: &str, out: &Out) {
        assert_eq!(out.code, Some(0), "{label}: exit; stderr: {}", out.err());
        assert_eq!(out.err(), "", "{label}: stderr");
        assert_eq!(
            out.stdout,
            guide_bytes(),
            "{label}: stdout is not the guide"
        );
    }

    #[test]
    fn prints_the_guide() {
        let world = World::empty();
        let out = world.run(&["help", "agents"]);
        assert_guide("help agents", &out);
        assert!(!out.stdout.is_empty(), "the guide is empty");
    }

    #[test]
    fn prints_the_snippet() {
        let world = World::empty();
        let out = world.run(&["help", "agents", "--snippet"]);
        assert_eq!(out.code, Some(0), "exit; stderr: {}", out.err());
        assert_eq!(out.err(), "", "stderr");
        let text = String::from_utf8(guide_bytes()).expect("guide is UTF-8");
        let section = snippet_section(&text)
            .unwrap_or_else(|| panic!("the guide has no {SNIPPET_START} .. {SNIPPET_END} section"));
        assert_eq!(
            out.out(),
            format!("{section}\n"),
            "stdout is not the marked section plus one LF"
        );
    }

    #[test]
    fn reads_nothing_and_ignores_global_flags() {
        let world = World::empty();
        let elsewhere = world.root.join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        // an unparseable config and an unusable data dir (a path below a regular file)
        let cfg_dir = world.cfg().join("hali");
        std::fs::create_dir_all(&cfg_dir).unwrap();
        std::fs::write(cfg_dir.join("config.toml"), "files = [not valid toml\n").unwrap();
        let not_a_dir = world.root.join("not-a-dir");
        std::fs::write(&not_a_dir, "x").unwrap();
        let before = world.tree();

        let plain = ["help", "agents"];
        let flag_cases: Vec<(&str, Vec<&str>)> = vec![
            ("--agent before", vec!["--agent", "x", "help", "agents"]),
            ("--agent after", vec!["help", "agents", "--agent", "x"]),
            ("--json before", vec!["--json", "help", "agents"]),
            ("--json after", vec!["help", "agents", "--json"]),
            ("--dry-run before", vec!["--dry-run", "help", "agents"]),
            ("--dry-run after", vec!["help", "agents", "--dry-run"]),
            ("--yes before", vec!["--yes", "help", "agents"]),
            ("--yes after", vec!["help", "agents", "--yes"]),
            (
                "all flags",
                vec![
                    "--json",
                    "--dry-run",
                    "--yes",
                    "help",
                    "agents",
                    "--agent",
                    "x",
                ],
            ),
        ];
        for (label, args) in &flag_cases {
            assert_guide(label, &world.run(args));
        }

        // HALI_AGENT with a control character, and one that is not UTF-8
        let mut cmd = world.command(BIN);
        cmd.args(plain).env("HALI_AGENT", "bad\u{1b}[2Jname");
        assert_guide("HALI_AGENT with a control character", &run(cmd));
        let mut cmd = world.command(BIN);
        cmd.args(plain)
            .env("HALI_AGENT", OsString::from_vec(vec![0xff, 0xfe]));
        assert_guide("HALI_AGENT not UTF-8", &run(cmd));

        // an unusable data dir, another cwd
        let mut cmd = world.command(BIN);
        cmd.args(plain).env("HALI_DATA_DIR", not_a_dir.join("data"));
        assert_guide("unusable data dir", &run(cmd));
        let mut cmd = world.command(BIN);
        cmd.args(plain).current_dir(&elsewhere);
        assert_guide("another cwd", &run(cmd));

        // `env -i`: no variable at all
        let mut cmd = std::process::Command::new(BIN);
        cmd.args(plain)
            .env_clear()
            .current_dir(&elsewhere)
            .stdin(Stdio::null());
        assert_guide("no environment", &run(cmd));

        assert_eq!(
            world.tree(),
            before,
            "help agents created or changed a file"
        );
    }

    fn run(mut cmd: std::process::Command) -> Out {
        let out = cmd.output().expect("run hali");
        Out {
            code: out.status.code(),
            stdout: out.stdout,
            stderr: out.stderr,
        }
    }

    #[test]
    fn usage_and_closed_stdout() {
        let world = World::empty();
        let out = world.run(&["help", "agents", "extra"]);
        assert_eq!(out.code, Some(2), "exit; stderr: {}", out.err());
        assert!(out.stdout.is_empty(), "stdout: {}", out.out());
        assert!(
            out.err().contains("unexpected argument 'extra'"),
            "stderr lacks `unexpected argument 'extra'`: {}",
            out.err()
        );

        // the reader is gone before hali writes: `hali help agents | head -1` at its worst
        for round in 0..3 {
            let mut cmd = world.command(BIN);
            cmd.args(["help", "agents"]);
            let mut child = cmd.spawn().expect("spawn hali");
            drop(child.stdout.take());
            let out = child.wait_with_output().expect("wait for hali");
            assert_eq!(out.status.code(), Some(1), "round {round}: exit code");
            assert!(
                out.stderr.is_empty(),
                "round {round}: stderr: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }

    #[test]
    fn help_command_forms() {
        let world = World::empty();
        let ok = |args: &[&str]| -> String {
            let out = world.run(args);
            assert_eq!(out.code, Some(0), "{args:?}: exit; stderr: {}", out.err());
            assert_eq!(out.err(), "", "{args:?}: stderr");
            out.out()
        };
        assert_eq!(ok(&["help"]), ok(&["--help"]), "help == --help");
        assert_eq!(ok(&["help", "find"]), ok(&["find", "--help"]), "help find");
        assert_eq!(
            ok(&["help", "concept", "add"]),
            ok(&["concept", "add", "--help"]),
            "help concept add"
        );
        let generate = ok(&["help", "generate"]);
        assert!(
            generate.contains("man") && generate.contains("completions"),
            "help generate: {generate}"
        );

        let out = world.run(&["help", "nope"]);
        assert_eq!(out.code, Some(2), "help nope: exit");
        assert!(out.stdout.is_empty(), "help nope: stdout");
        assert!(
            out.err().contains("error: unrecognized subcommand 'nope'"),
            "help nope: {}",
            out.err()
        );
        let hostile = "a\u{1b}]0;PWN\u{7}\u{1b}[2J\u{2028}b";
        let out = world.run(&["help", hostile]);
        assert_eq!(out.code, Some(2), "help <hostile>: exit");
        assert!(
            out.err().contains("unrecognized subcommand '"),
            "help <hostile>: {}",
            out.err()
        );
        for c in out.err().chars().filter(|&c| c != '\n') {
            assert!(!is_escaped(c), "raw {c:?} in stderr {:?}", out.err());
        }

        let root = ok(&["help"]);
        let row = root
            .lines()
            .find(|l| l.trim_start().starts_with("help "))
            .unwrap_or_else(|| panic!("no `help` row in:\n{root}"));
        assert!(row.contains("agents"), "the help row lacks `agents`: {row}");
    }
}
