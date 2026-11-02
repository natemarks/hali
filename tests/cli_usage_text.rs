//! Goal tests for clap usage errors through the real binary (#117, spec §9.3 "Escaped for the
//! terminal", §9.4 text bullet): whatever argument a usage error repeats, stderr holds no
//! escape-set character, keeps clap's layout without colour and is byte-identical to today's
//! output for ordinary arguments. The unit side is tests/usage_text.rs. The `pty` module runs the
//! binary under `script(1)` (the pattern of `real_pty_prompts` in tests/cli_forget.rs), is
//! ignored by default (`make test-external` and `.goal/checks/03-pty.sh` run it), and passes the
//! hostile arguments as environment variables so /bin/sh needs no `$'...'`.
//!
//! Every run clears the environment, points HALI_DATA_DIR / XDG_CONFIG_HOME / HOME at temp dirs
//! and sets HALI_TEST_TTY=0. A usage error is raised before the store is read, so the store is
//! empty. Nothing here is mode based, so no case is skipped for root.
//!
//! Wording the contract leaves open and these tests fix (change them, with the human, if the
//! wording should differ):
//! - the hostile argument H is `a ESC ] 0 ; PWN BEL ESC [ 2 J CR U+009B U+202E U+2028 U+200B
//!   U+E0041 b` (no NUL, valid UTF-8); the pty's first case uses P, the same without CR and the
//!   non-ASCII characters;
//! - the unknown flag is `--` followed by H; `find --media` is spelled `--media-type`, the flag's
//!   real name; `find --media-type` repeats the pattern twice, the second time in Rust's `{:?}`
//!   form (the domain message does that today), so its literal holds `\u{1b}`, `\r` and the like
//!   as the backslash text Debug produces; clap's own text does not escape that text further;
//! - `schema <x>` lists the valid names (read from `hali schema`); only the unknown subcommand
//!   and the value cases are asserted to take an argument holding `\n` without adding a line;
//!   `schema`'s message is custom text, not a clap context value, so `usage_text` cannot tell
//!   which part is argv: `show_schema` escapes the name with `printable` (newline included)
//!   before building the message, and `schema_newline_in_name` and
//!   `schema_hostile_name_families` here are the authority (no unit test can cover it);
//! - `--json` asserts only exit 2 and no raw §9.3 character other than `\n` in stdout or stderr
//!   (#84 and #118 may change the text);
//! - on a pty the output ends lines with CR LF, which is turned into LF before comparing; help on
//!   a pty is expected to keep its colour (ESC present) and exit 0.

mod goal {
    use assert_cmd::Command;
    use clap::builder::Styles;
    use std::ffi::{OsStr, OsString};
    use std::path::PathBuf;
    use std::time::Duration;
    use tempfile::TempDir;

    /// Hostile text: an OSC title sequence, a screen clear, a CR, an 8-bit CSI, a bidi override,
    /// a line separator, a zero-width space and a tag character.
    pub const H: &str = "a\u{1b}]0;PWN\u{7}\u{1b}[2J\r\u{9b}\u{202e}\u{2028}\u{200b}\u{e0041}b";
    /// H as the escaped text shows it.
    pub const EH: &str =
        "a\\u{1b}]0;PWN\\u{7}\\u{1b}[2J\\u{d}\\u{9b}\\u{202e}\\u{2028}\\u{200b}\\u{e0041}b";
    /// H in Rust's Debug form, quotes included (what the media type message repeats).
    pub const DEBUG_H: &str =
        "\"a\\u{1b}]0;PWN\\u{7}\\u{1b}[2J\\r\\u{9b}\\u{202e}\\u{2028}\\u{200b}\\u{e0041}b\"";

    /// The footer of a usage error that has no usage block.
    const FOOT: &str = "\nFor more information, try '--help'.\n";
    /// The footer of a usage error under the root command.
    const ROOT_FOOT: &str =
        "\nUsage: hali [OPTIONS] <COMMAND>\n\nFor more information, try '--help'.\n";

    pub struct Env {
        _root: TempDir,
        pub root: PathBuf,
    }

    pub struct Out {
        pub code: Option<i32>,
        pub stdout: String,
        pub stderr: String,
    }

    /// Whether text mode may not print `c` raw (the contract's set, without the structural LF).
    pub fn is_escaped(c: char) -> bool {
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

    impl Env {
        pub fn new() -> Env {
            let tmp = tempfile::tempdir().unwrap();
            let root = std::fs::canonicalize(tmp.path()).unwrap();
            for d in ["cfg", "home", "data"] {
                std::fs::create_dir_all(root.join(d)).unwrap();
            }
            Env { _root: tmp, root }
        }

        pub fn data(&self) -> PathBuf {
            self.root.join("data")
        }

        pub fn run<I, S>(&self, args: I) -> Out
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            let out = Command::cargo_bin("hali")
                .unwrap()
                .env_clear()
                .env("XDG_CONFIG_HOME", self.root.join("cfg"))
                .env("HOME", self.root.join("home"))
                .env("HALI_DATA_DIR", self.data())
                .env("HALI_TEST_NOW", "2026-10-24T09:30:00Z")
                .env("HALI_TEST_ID_SEED", "1")
                .env("HALI_TEST_TTY", "0")
                .timeout(Duration::from_secs(30))
                .args(args)
                .output()
                .unwrap();
            Out {
                code: out.status.code(),
                stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            }
        }

        /// The valid schema names, as `hali schema` lists them.
        pub fn schema_names(&self) -> String {
            let out = self.run(["schema"]);
            assert_eq!(out.code, Some(0), "{}", out.stderr);
            out.stdout.lines().collect::<Vec<_>>().join(", ")
        }
    }

    /// A failed text-mode run: exit 2, nothing on stdout, `expected` exactly, and no escape-set
    /// character (but LF) on stderr.
    fn assert_usage(label: &str, out: &Out, expected: &str) {
        assert_eq!(out.code, Some(2), "{label}: {}", out.stderr);
        assert_eq!(out.stdout, "", "{label}");
        for c in out.stderr.chars().filter(|&c| c != '\n') {
            assert!(
                !is_escaped(c),
                "{label}: raw {c:?} on stderr: {:?}",
                out.stderr
            );
        }
        assert_eq!(out.stderr, expected, "{label}");
    }

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn families_escape() {
        let env = Env::new();
        let names = env.schema_names();
        let flag = format!("--{H}");
        let cases: Vec<(&str, Vec<OsString>, String)> = vec![
            (
                "unknown subcommand",
                os(&[H]),
                format!("error: unrecognized subcommand '{EH}'\n{ROOT_FOOT}"),
            ),
            (
                "unknown flag",
                os(&[&flag]),
                format!("error: unexpected argument '--{EH}' found\n{ROOT_FOOT}"),
            ),
            (
                "invalid value (find --limit)",
                os(&["find", "--limit", H]),
                format!(
                    "error: invalid value '{EH}' for '--limit <LIMIT>': invalid digit found in string\n{FOOT}"
                ),
            ),
            (
                "invalid value (find --media-type)",
                os(&["find", "--media-type", H]),
                format!(
                    "error: invalid value '{EH}' for '--media-type <MEDIA_TYPE>': invalid media type pattern {DEBUG_H}: use type/subtype or type/*\n{FOOT}"
                ),
            ),
            (
                "concept add",
                os(&["concept", "add", H]),
                format!(
                    "error: invalid value '{EH}' for '<LABEL>': a label must not contain control characters\n{FOOT}"
                ),
            ),
            (
                "type add",
                os(&["type", "add", H]),
                format!(
                    "error: invalid value '{EH}' for '<NAME>': a Type name starts with an ASCII letter, has only ASCII letters, digits, `_` and `-`, and is at most 64 characters\n{FOOT}"
                ),
            ),
            (
                "relation add",
                os(&["relation", "add", H]),
                format!(
                    "error: invalid value '{EH}' for '<NAME>': a relation name starts with an ASCII lowercase letter, has only ASCII letters, digits, `_` and `-`, and is at most 64 characters\n{FOOT}"
                ),
            ),
            (
                "schema <x>",
                os(&["schema", H]),
                format!("error: unknown schema `{EH}`; valid names: {names}\n{ROOT_FOOT}"),
            ),
            (
                "help <x>",
                os(&["help", H]),
                format!("error: unrecognized subcommand '{EH}'\n{ROOT_FOOT}"),
            ),
        ];
        for (label, args, expected) in cases {
            assert_usage(label, &env.run(&args), &expected);
        }
    }

    /// Guard: ordinary usage errors are what clap prints today without a TTY.
    #[test]
    fn ordinary_unchanged() {
        let env = Env::new();
        let names = env.schema_names();
        let edit_group = "<--title <TITLE>|--add-type <TYPE>|--remove-type <TYPE>|--add-tag <CONCEPT>|--remove-tag <CONCEPT>>";
        let cases: Vec<(Vec<&str>, String)> = vec![
            (
                vec!["--bogus"],
                format!("error: unexpected argument '--bogus' found\n{ROOT_FOOT}"),
            ),
            (
                vec!["fnd"],
                format!(
                    "error: unrecognized subcommand 'fnd'\n\n  tip: a similar subcommand exists: 'find'\n{ROOT_FOOT}"
                ),
            ),
            (
                vec!["concept", "add"],
                "error: the following required arguments were not provided:\n  <LABEL>\n\nUsage: hali concept add <LABEL>\n\nFor more information, try '--help'.\n".to_string(),
            ),
            (
                vec!["edit", "x"],
                format!(
                    "error: the following required arguments were not provided:\n  {edit_group}\n\nUsage: hali edit {edit_group} <REF>\n\nFor more information, try '--help'.\n"
                ),
            ),
            (
                vec!["describe", "extra"],
                "error: unexpected argument 'extra' found\n\nUsage: hali describe [OPTIONS]\n\nFor more information, try '--help'.\n".to_string(),
            ),
            (
                vec!["schema", "nope"],
                format!("error: unknown schema `nope`; valid names: {names}\n{ROOT_FOOT}"),
            ),
            (
                vec!["schema", "--bogus"],
                "error: unexpected argument '--bogus' found\n\n  tip: to pass '--bogus' as a value, use '-- --bogus'\n\nUsage: hali schema [OPTIONS] [NAME]...\n\nFor more information, try '--help'.\n".to_string(),
            ),
            (
                vec!["concept", "show", "--bogus"],
                "error: unexpected argument '--bogus' found\n\n  tip: to pass '--bogus' as a value, use '-- --bogus'\n\nUsage: hali concept show [OPTIONS] <CONCEPT>\n\nFor more information, try '--help'.\n".to_string(),
            ),
            (
                vec!["refresh"],
                "error: the following required arguments were not provided:\n  <REF|--all>\n\nUsage: hali refresh <REF|--all>\n\nFor more information, try '--help'.\n".to_string(),
            ),
            (
                vec!["refresh", "--all", "R1"],
                "error: the argument '--all' cannot be used with '[REF]...'\n\nUsage: hali refresh <REF|--all>\n\nFor more information, try '--help'.\n".to_string(),
            ),
        ];
        for (args, expected) in cases {
            assert_usage(&args.join(" "), &env.run(&args), &expected);
        }
    }

    #[test]
    fn newline_in_argv() {
        let env = Env::new();
        let ordinary = env.run(["zz"]);
        let out = env.run(["zz\nerror: forged"]);
        assert_usage(
            "unknown subcommand",
            &out,
            &format!("error: unrecognized subcommand 'zz\\u{{a}}error: forged'\n{ROOT_FOOT}"),
        );
        assert_eq!(out.stderr.lines().count(), ordinary.stderr.lines().count());
    }

    #[test]
    fn newline_in_value() {
        let env = Env::new();
        let ordinary = env.run(["find", "--limit", "zz"]);
        let out = env.run(["find", "--limit", "zz\nerror: forged"]);
        assert_usage(
            "find --limit",
            &out,
            &format!(
                "error: invalid value 'zz\\u{{a}}error: forged' for '--limit <LIMIT>': invalid digit found in string\n{FOOT}"
            ),
        );
        assert_eq!(out.stderr.lines().count(), ordinary.stderr.lines().count());
    }

    /// `schema`'s message is custom text, not a clap context value: the name it repeats must be
    /// escaped by `show_schema` itself, newline included, so a name cannot add a line.
    #[test]
    fn schema_newline_in_name() {
        let env = Env::new();
        let names = env.schema_names();
        let ordinary = env.run(["schema", "zz"]);
        assert_eq!(ordinary.code, Some(2), "{}", ordinary.stderr);
        let out = env.run(["schema", "zz\nerror: forged"]);
        assert_usage(
            "schema newline",
            &out,
            &format!(
                "error: unknown schema `zz\\u{{a}}error: forged`; valid names: {names}\n{ROOT_FOOT}"
            ),
        );
        assert_eq!(out.stderr.lines().count(), ordinary.stderr.lines().count());
        // a name given as several words is joined before it is repeated
        let words = env.run(["schema", "zz", "error:\nforged"]);
        assert_eq!(
            words.stderr.lines().count(),
            ordinary.stderr.lines().count(),
            "{}",
            words.stderr
        );
    }

    /// Each hostile family, alone, in a `schema` name: shown as `\u{..}`, never raw, with the
    /// line count of an ordinary unknown name.
    #[test]
    fn schema_hostile_name_families() {
        let env = Env::new();
        let names = env.schema_names();
        let ordinary = env.run(["schema", "zz"]);
        for (label, raw, shown) in [
            ("CR", "zz\r", "zz\\u{d}"),
            ("U+009B", "zz\u{9b}", "zz\\u{9b}"),
            ("U+202E", "zz\u{202e}", "zz\\u{202e}"),
            ("ESC", "zz\u{1b}[2J", "zz\\u{1b}[2J"),
            ("OSC", "zz\u{1b}]0;PWN\u{7}", "zz\\u{1b}]0;PWN\\u{7}"),
        ] {
            let out = env.run(["schema", raw]);
            assert_usage(
                label,
                &out,
                &format!("error: unknown schema `{shown}`; valid names: {names}\n{ROOT_FOOT}"),
            );
            assert_eq!(out.stderr.lines().count(), ordinary.stderr.lines().count());
        }
    }

    /// Guard: help and version are clap's, on stdout with exit 0; a bare command prints its help
    /// on stderr and exits 2.
    #[test]
    fn help_and_version_unchanged() {
        let env = Env::new();
        let styled = hali::cli::command().styles(Styles::styled());

        let out = env.run(["--help"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        assert_eq!(out.stdout, styled.clone().render_help().to_string());

        let out = env.run(["concept", "add", "--help"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        assert!(
            out.stdout.contains("Usage: hali concept add"),
            "{}",
            out.stdout
        );
        assert!(!out.stdout.contains('\u{1b}'));

        let out = env.run(["--version"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        assert_eq!(out.stdout, format!("hali {}\n", env!("CARGO_PKG_VERSION")));

        // A bare command prints its help on stderr and exits 2, as today.
        let out = env.run::<[&str; 0], &str>([]);
        assert_eq!(out.code, Some(2));
        assert_eq!(out.stdout, "");
        assert_eq!(out.stderr, styled.clone().render_help().to_string());
        let out = env.run(["concept"]);
        assert_eq!(out.code, Some(2));
        assert_eq!(out.stdout, "");
        assert!(out.stderr.contains("Usage: hali concept"), "{}", out.stderr);
    }

    /// Under `--json` a usage error is still clap's text (the envelope is #84): only the escaping
    /// and the exit code are asserted.
    #[test]
    fn json_mode_escaped() {
        let env = Env::new();
        let flag = format!("--{H}");
        for args in [
            os(&["--json", H]),
            os(&["--json", &flag]),
            os(&["--json", "find", "--limit", H]),
            os(&["find", "--json", "--limit", H]),
            os(&["--json", "concept", "add", H]),
            os(&["--json", "schema", H]),
        ] {
            let out = env.run(&args);
            assert_eq!(out.code, Some(2), "{args:?}: {}", out.stderr);
            for c in out.stdout.chars().chain(out.stderr.chars()) {
                assert!(
                    c == '\n' || !is_escaped(c),
                    "{args:?}: raw {c:?} in {:?} / {:?}",
                    out.stdout,
                    out.stderr
                );
            }
        }
    }

    /// Whatever the binary prints on a pseudo-terminal: no raw escape-set character, no colour
    /// codes, exit 2, the escaped argument shown.
    mod pty {
        use super::{EH, Env, H, is_escaped};
        use std::process::Stdio;

        /// P: the hostile value of the original report (OSC title, screen clear).
        const P: &str = "a\u{1b}]0;PWN\u{7}\u{1b}[2Jb";
        const EP: &str = "a\\u{1b}]0;PWN\\u{7}\\u{1b}[2Jb";

        /// Runs `hali <shell_args>` under script(1); `$HOSTILE` in the arguments is `hostile`.
        /// Returns the exit status and the terminal output.
        fn on_a_pty(env: &Env, hostile: &str, shell_args: &str) -> (Option<i32>, Vec<u8>) {
            let command = format!("{} {shell_args}", env!("CARGO_BIN_EXE_hali"));
            let child = std::process::Command::new("/usr/bin/script")
                .args(["-qec", &command, "/dev/null"])
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("TERM", "xterm")
                .env("XDG_CONFIG_HOME", env.root.join("cfg"))
                .env("HOME", env.root.join("home"))
                .env("HALI_DATA_DIR", env.data())
                .env("HOSTILE", hostile)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let out = child.wait_with_output().unwrap();
            (out.status.code(), out.stdout)
        }

        /// The terminal output as text, lines ending in LF.
        fn lines(bytes: &[u8]) -> String {
            String::from_utf8_lossy(bytes).replace("\r\n", "\n")
        }

        fn assert_clean(label: &str, bytes: &[u8]) {
            assert!(
                !bytes.contains(&0x1b),
                "{label}: raw ESC in {:?}",
                lines(bytes)
            );
            for c in lines(bytes).chars() {
                assert!(
                    c == '\n' || !is_escaped(c),
                    "{label}: raw {c:?} in {:?}",
                    lines(bytes)
                );
            }
        }

        #[test]
        #[ignore = "needs script(1) from util-linux"]
        fn hostile_value_on_a_pty() {
            let env = Env::new();
            let (code, out) = on_a_pty(&env, P, "find --limit \"$HOSTILE\"");
            assert!(!out.contains(&0x1b), "raw ESC in {:?}", lines(&out));
            assert_clean("find --limit", &out);
            assert_eq!(code, Some(2), "{}", lines(&out));
            assert_eq!(
                lines(&out),
                format!(
                    "error: invalid value '{EP}' for '--limit <LIMIT>': invalid digit found in string\n\nFor more information, try '--help'.\n"
                )
            );
        }

        #[test]
        #[ignore = "needs script(1) from util-linux"]
        fn every_family_on_a_pty() {
            let env = Env::new();
            for (label, shell_args, shown) in [
                (
                    "unknown subcommand",
                    "\"$HOSTILE\"",
                    format!("unrecognized subcommand '{EH}'"),
                ),
                (
                    "unknown flag",
                    "\"--$HOSTILE\"",
                    format!("unexpected argument '--{EH}' found"),
                ),
                (
                    "concept add",
                    "concept add \"$HOSTILE\"",
                    format!("invalid value '{EH}' for '<LABEL>'"),
                ),
                (
                    "type add",
                    "type add \"$HOSTILE\"",
                    format!("invalid value '{EH}' for '<NAME>'"),
                ),
                (
                    "relation add",
                    "relation add \"$HOSTILE\"",
                    format!("invalid value '{EH}' for '<NAME>'"),
                ),
            ] {
                let (code, out) = on_a_pty(&env, H, shell_args);
                assert_clean(label, &out);
                assert_eq!(code, Some(2), "{label}: {}", lines(&out));
                assert!(lines(&out).contains(&shown), "{label}: {:?}", lines(&out));
            }
        }

        /// Guard: help keeps its colour and version works; both exit 0.
        #[test]
        #[ignore = "needs script(1) from util-linux"]
        fn help_and_version_on_a_pty() {
            let env = Env::new();
            let (code, out) = on_a_pty(&env, "", "--help");
            assert_eq!(code, Some(0), "{}", lines(&out));
            assert!(
                out.contains(&0x1b),
                "help lost its colour: {:?}",
                lines(&out)
            );
            let (code, out) = on_a_pty(&env, "", "--version");
            assert_eq!(code, Some(0), "{}", lines(&out));
            assert_eq!(lines(&out), format!("hali {}\n", env!("CARGO_PKG_VERSION")));
        }
    }
}
