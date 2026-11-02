//! Goal tests for the program name in clap's output (#123, spec §9): help, usage errors and
//! version always call the binary `hali`, whatever `argv[0]` the process was started with, so a
//! hostile name (an `exec -a`, a symlink or a hard link) cannot reach the terminal. The binary is
//! run through `Command::arg0` (assert_cmd has none). The `pty` module runs it under `script(1)`;
//! it is ignored by default (`make test-external` and `.goal/checks/03-pty.sh` run it).
//!
//! Every run clears the environment, points HALI_DATA_DIR / XDG_CONFIG_HOME / HOME at a temp dir
//! and sets HALI_TEST_TTY=0 (not on the pty, as in tests/cli_usage_text.rs). None of the argument
//! sets needs a store, and nothing here is mode based, so no case is skipped for root.
//!
//! The oracle `same(name, args)`: the run under `name` has the exit code, stdout and stderr of
//! the run under `arg0("hali")`, byte for byte, the same line count, and no escape-set character
//! (§9.3, without the structural LF) in the output.
//!
//! Wording the contract leaves open and these tests fix (change them, with the human, if the
//! wording should differ):
//! - the hostile names are: LF; `x LF error: forged`; CR; TAB; `ESC[2J`; an OSC title ended by
//!   BEL and one ended by ST (`ESC \`); U+009B; U+202E; U+2028; U+E0041; the composite H of
//!   tests/cli_usage_text.rs; and a name of 4096 characters. Names for links are at most 255
//!   bytes (a file name limit), so the long name is only run through `arg0`;
//! - the argument sets and their exit codes are `zz` 2, `concept add` 2, `--help` 0,
//!   `help concept` 0, `concept add --help` 0, `--version` 0, none 2, `concept` 2, `schema zz` 2;
//! - the ordinary names are `h`, `/some/dir/h` and `foo/`; the links are a symlink and a hard
//!   link named `h` and ones named `x LF error: forged`, run by path (no `arg0`);
//! - the degenerate names are empty, `\xff\xfe`, `x LF \xff`, `..` and `/`; they are compared
//!   with the same oracle over `zz`, `--help` and no arguments;
//! - `anchors` runs under `x LF error: forged`; help is compared with
//!   `command().styles(Styles::styled()).render_help()` as tests/cli_usage_text.rs does;
//! - on a pty the hostile name is H, reaches the binary through a symlink (/bin/sh may be dash,
//!   which has no `exec -a`), and bytes after CRLF to LF must equal the run through a symlink
//!   named `hali`, for `--help`, no arguments and `concept`; `zz` must have no ESC at all.

#[cfg(feature = "goal")]
mod goal {
    use clap::builder::Styles;
    use std::ffi::{OsStr, OsString};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::process::Stdio;
    use std::sync::mpsc;
    use std::time::Duration;

    /// The composite hostile text of tests/cli_usage_text.rs.
    pub const H: &str = "a\u{1b}]0;PWN\u{7}\u{1b}[2J\r\u{9b}\u{202e}\u{2028}\u{200b}\u{e0041}b";

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

    pub struct Out {
        pub code: Option<i32>,
        pub stdout: Vec<u8>,
        pub stderr: Vec<u8>,
    }

    impl Out {
        fn out(&self) -> String {
            String::from_utf8_lossy(&self.stdout).into_owned()
        }
        fn err(&self) -> String {
            String::from_utf8_lossy(&self.stderr).into_owned()
        }
    }

    pub struct Env {
        _tmp: tempfile::TempDir,
        root: PathBuf,
    }

    impl Env {
        pub fn new() -> Env {
            let tmp = tempfile::tempdir().unwrap();
            let root = std::fs::canonicalize(tmp.path()).unwrap();
            for d in ["cfg", "home", "data"] {
                std::fs::create_dir_all(root.join(d)).unwrap();
            }
            Env { _tmp: tmp, root }
        }

        /// Runs `program` with `arg0` (None: the program's own path) and `args`.
        pub fn run_prog(&self, program: &Path, arg0: Option<&OsStr>, args: &[&str]) -> Out {
            let mut cmd = std::process::Command::new(program);
            if let Some(a) = arg0 {
                cmd.arg0(a);
            }
            let child = cmd
                .args(args)
                .env_clear()
                .env("XDG_CONFIG_HOME", self.root.join("cfg"))
                .env("HOME", self.root.join("home"))
                .env("HALI_DATA_DIR", self.root.join("data"))
                .env("HALI_TEST_TTY", "0")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(child.wait_with_output());
            });
            let out = rx
                .recv_timeout(Duration::from_secs(30))
                .expect("hali did not finish in 30 s")
                .unwrap();
            Out {
                code: out.status.code(),
                stdout: out.stdout,
                stderr: out.stderr,
            }
        }

        pub fn run_as(&self, name: &OsStr, args: &[&str]) -> Out {
            self.run_prog(Path::new(env!("CARGO_BIN_EXE_hali")), Some(name), args)
        }

        /// The baseline: the installed name.
        pub fn hali(&self, args: &[&str]) -> Out {
            self.run_as(OsStr::new("hali"), args)
        }

        /// The oracle: `got` is the baseline run for `args`, byte for byte, with the same line
        /// count, and has no escape-set character but LF.
        pub fn assert_same(&self, label: &str, got: &Out, args: &[&str]) {
            let want = self.hali(args);
            assert_eq!(got.err(), want.err(), "{label} {args:?}: stderr");
            assert_eq!(got.out(), want.out(), "{label} {args:?}: stdout");
            assert_eq!(got.code, want.code, "{label} {args:?}: exit code");
            assert_eq!(got.stderr, want.stderr, "{label} {args:?}: stderr bytes");
            assert_eq!(got.stdout, want.stdout, "{label} {args:?}: stdout bytes");
            assert_eq!(
                got.err().lines().count(),
                want.err().lines().count(),
                "{label} {args:?}: stderr line count"
            );
            assert_eq!(
                got.out().lines().count(),
                want.out().lines().count(),
                "{label} {args:?}: stdout line count"
            );
            for text in [got.out(), got.err()] {
                for c in text.chars().filter(|&c| c != '\n') {
                    assert!(!is_escaped(c), "{label} {args:?}: raw {c:?} in {text:?}");
                }
            }
        }
    }

    /// The argument sets and the exit code each gives.
    pub const ARGS: &[(&[&str], i32)] = &[
        (&["zz"], 2),
        (&["concept", "add"], 2),
        (&["--help"], 0),
        (&["help", "concept"], 0),
        (&["concept", "add", "--help"], 0),
        (&["--version"], 0),
        (&[], 2),
        (&["concept"], 2),
        (&["schema", "zz"], 2),
    ];

    /// The hostile names a link can carry (no `/`, at most 255 bytes).
    pub fn short_names() -> Vec<String> {
        vec![
            "\n".to_string(),
            "x\nerror: forged".to_string(),
            "\r".to_string(),
            "\t".to_string(),
            "\u{1b}[2J".to_string(),
            "\u{1b}]0;PWN\u{7}".to_string(),
            "\u{1b}]0;PWN\u{1b}\\".to_string(),
            "\u{9b}".to_string(),
            "\u{202e}".to_string(),
            "\u{2028}".to_string(),
            "\u{e0041}".to_string(),
            H.to_string(),
        ]
    }

    #[test]
    fn hostile_arg0_matches_hali() {
        let env = Env::new();
        let mut names = short_names();
        names.push("a".repeat(4096));
        for name in &names {
            let label = format!("{:?}", name.chars().take(40).collect::<String>());
            for (args, code) in ARGS {
                let got = env.run_as(OsStr::new(name), args);
                assert_eq!(got.code, Some(*code), "{label} {args:?}: {}", got.err());
                env.assert_same(&label, &got, args);
            }
        }
    }

    #[test]
    fn anchors() {
        let env = Env::new();
        let name = OsStr::new("x\nerror: forged");
        let zz = env.run_as(name, &["zz"]);
        assert_eq!(zz.code, Some(2));
        assert_eq!(zz.out(), "");
        assert_eq!(
            zz.err(),
            "error: unrecognized subcommand 'zz'\n\nUsage: hali [OPTIONS] <COMMAND>\n\nFor more information, try '--help'.\n"
        );
        let add = env.run_as(name, &["concept", "add"]);
        assert_eq!(add.code, Some(2));
        assert_eq!(
            add.err(),
            "error: the following required arguments were not provided:\n  <LABEL>\n\nUsage: hali concept add <LABEL>\n\nFor more information, try '--help'.\n"
        );
        let help = env.run_as(name, &["--help"]);
        assert_eq!(help.code, Some(0));
        assert_eq!(
            help.out(),
            hali::cli::command()
                .styles(Styles::styled())
                .render_help()
                .to_string()
        );
        let version = env.run_as(name, &["--version"]);
        assert_eq!(version.code, Some(0));
        assert_eq!(
            version.out(),
            format!("hali {}\n", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn renamed_and_linked() {
        let env = Env::new();
        let sets: [&[&str]; 3] = [&["zz"], &["--help"], &[]];
        for name in ["h", "/some/dir/h", "foo/"] {
            for args in sets {
                let got = env.run_as(OsStr::new(name), args);
                env.assert_same(name, &got, args);
            }
        }
        let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
        let bin = Path::new(env!("CARGO_BIN_EXE_hali"));
        let sym_dir = dir.path().join("sym");
        let hard_dir = dir.path().join("hard");
        std::fs::create_dir_all(&sym_dir).unwrap();
        std::fs::create_dir_all(&hard_dir).unwrap();
        for name in ["h", "x\nerror: forged"] {
            let sym = sym_dir.join(name);
            std::os::unix::fs::symlink(bin, &sym).unwrap();
            let hard = hard_dir.join(name);
            std::fs::hard_link(bin, &hard).unwrap();
            for (kind, link) in [("symlink", &sym), ("hard link", &hard)] {
                for args in sets {
                    let got = env.run_prog(link, None, args);
                    env.assert_same(&format!("{kind} {name:?}"), &got, args);
                }
            }
        }
    }

    /// A guard against a clap upgrade: these names already fall back to `hali`.
    #[test]
    fn degenerate_arg0() {
        let env = Env::new();
        let sets: [&[&str]; 3] = [&["zz"], &["--help"], &[]];
        let names: Vec<OsString> = vec![
            OsString::new(),
            OsStr::from_bytes(b"\xff\xfe").to_os_string(),
            OsStr::from_bytes(b"x\n\xff").to_os_string(),
            OsString::from(".."),
            OsString::from("/"),
        ];
        for name in &names {
            for args in sets {
                let got = env.run_as(name, args);
                env.assert_same(&format!("{name:?}"), &got, args);
            }
        }
    }

    #[test]
    fn command_bin_name() {
        assert_eq!(hali::cli::command().get_bin_name(), Some("hali"));
        let err = hali::cli::command()
            .try_get_matches_from([H, "zz"])
            .unwrap_err();
        let text = err.render().to_string();
        assert!(
            text.contains("Usage: hali [OPTIONS] <COMMAND>"),
            "usage line: {text:?}"
        );
    }

    mod pty {
        use super::{Env, H, is_escaped};
        use std::path::{Path, PathBuf};
        use std::process::Stdio;

        /// Runs `"$HALI_LINK" <shell_args>` under script(1). Returns the exit status and the
        /// terminal output.
        fn on_a_pty(env: &Env, link: &Path, shell_args: &str) -> (Option<i32>, Vec<u8>) {
            let command = format!("\"$HALI_LINK\" {shell_args}");
            let child = std::process::Command::new("/usr/bin/script")
                .args(["-qec", &command, "/dev/null"])
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("TERM", "xterm")
                .env("XDG_CONFIG_HOME", env.root.join("cfg"))
                .env("HOME", env.root.join("home"))
                .env("HALI_DATA_DIR", env.root.join("data"))
                .env("HALI_LINK", link)
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

        fn link_named(dir: &Path, sub: &str, name: &str) -> PathBuf {
            let d = dir.join(sub);
            std::fs::create_dir_all(&d).unwrap();
            let link = d.join(name);
            std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_hali"), &link).unwrap();
            link
        }

        #[test]
        #[ignore = "needs script(1) from util-linux"]
        fn hostile_name_on_a_pty() {
            let env = Env::new();
            let dir = tempfile::tempdir().unwrap();
            let good = link_named(dir.path(), "good", "hali");
            let bad = link_named(dir.path(), "bad", H);
            for (label, shell_args) in [
                ("--help", "--help"),
                ("bare hali", ""),
                ("bare hali concept", "concept"),
            ] {
                let (want_code, want) = on_a_pty(&env, &good, shell_args);
                let (code, got) = on_a_pty(&env, &bad, shell_args);
                assert_eq!(code, want_code, "{label}: exit code");
                assert_eq!(lines(&got), lines(&want), "{label}: bytes after CRLF to LF");
            }
            let (code, got) = on_a_pty(&env, &bad, "zz");
            assert_eq!(code, Some(2), "zz: {}", lines(&got));
            assert!(!got.contains(&0x1b), "zz: raw ESC in {:?}", lines(&got));
            for c in lines(&got).chars() {
                assert!(
                    c == '\n' || !is_escaped(c),
                    "zz: raw {c:?} in {:?}",
                    lines(&got)
                );
            }
            let (_, want) = on_a_pty(&env, &good, "zz");
            assert_eq!(lines(&got), lines(&want), "zz: bytes after CRLF to LF");
        }
    }
}
