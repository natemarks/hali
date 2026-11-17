//! Goal tests for `hali generate man` and `hali generate completions <bash|zsh|fish>` (#4, spec
//! §9.1, §13.2) through the real binary. The `make install` half is `.goal/checks/04-install.sh`.
//!
//! Every run clears the environment, then sets HOME, XDG_CONFIG_HOME, XDG_DATA_HOME,
//! XDG_STATE_HOME and HALI_DATA_DIR to paths inside a temp dir (an empty store); no HALI_* or
//! other variable leaks in. Nothing here is mode based, so no case is skipped for root.
//!
//! Assumed API and behaviour (the contract of issue #4, "Groomed contract (approved)"):
//! - `hali::cli::command()` has a top-level subcommand `generate` with `is_hide_set()`, and under
//!   it the leaves `man` and `completions`;
//! - `generate man` prints one roff page for `hali` on stdout, exit 0, empty stderr; the page has
//!   a line starting `.TH hali 1`, mentions `describe` and `find`, and holds no `generate`;
//! - `generate completions <bash|zsh|fish>` prints the script, exit 0, empty stderr: bash has
//!   `complete -F _hali` and ends that line in ` hali`, zsh starts with `#compdef hali`, fish
//!   has `complete -c hali`; none contains `$0`, and none offers the hidden `generate`;
//! - an unknown shell (including `elvish`, `powershell` and hostile text) is a usage error: exit
//!   2, stdout empty, stderr lists `bash, zsh, fish` and holds no escape-set character but LF; a
//!   missing shell, a stray extra argument and bare `generate` are exit 2 as well, the last with
//!   help (`Usage: hali generate`, naming `man` and `completions`) on stderr;
//! - `generate --help` and `help generate` still exit 0, and the help names `man` and
//!   `completions`; `hali --help`, `-h` and `hali help` do not mention `generate`;
//! - the output is byte-identical whatever `argv[0]`, HOME, LANG, TZ, SOURCE_DATE_EPOCH, cwd and
//!   `--json`/`--dry-run`/`--yes` (before or after the leaf) are, holds no temp or host path, and
//!   is produced with a broken data dir and config and with no environment at all, creating
//!   nothing;
//! - the hostile `arg0` names are LF, `x LF error: forged`, CR, TAB, `ESC[2J`, an OSC title, the
//!   composite H, `evil` and `/some/dir/evil`; the links are a symlink named `hali-link` and one
//!   with a hostile name, run by path.
//!
//! Row 9 (dry-run guard) and the hidden-leaf skip in `every_command_has_a_schema` are edits to
//! tests/dry_run_guard.rs and tests/core_schema.rs.

#[cfg(feature = "goal")]
mod goal {
    use std::ffi::OsStr;
    use std::os::unix::fs::symlink;
    use std::os::unix::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::process::Stdio;

    const H: &str = "a\u{1b}]0;PWN\u{7}\u{1b}[2J\r\u{9b}\u{202e}\u{2028}\u{200b}\u{e0041}b";
    const SHELLS: [&str; 3] = ["bash", "zsh", "fish"];
    const BIN: &str = env!("CARGO_BIN_EXE_hali");

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

    struct Out {
        code: Option<i32>,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    }

    impl Out {
        fn out(&self) -> String {
            String::from_utf8_lossy(&self.stdout).into_owned()
        }
        fn err(&self) -> String {
            String::from_utf8_lossy(&self.stderr).into_owned()
        }
    }

    struct Env {
        _tmp: tempfile::TempDir,
        root: PathBuf,
    }

    impl Env {
        fn new() -> Env {
            let tmp = tempfile::tempdir().unwrap();
            let root = std::fs::canonicalize(tmp.path()).unwrap();
            for d in ["cfg", "home", "data", "share", "state", "cwd"] {
                std::fs::create_dir_all(root.join(d)).unwrap();
            }
            Env { _tmp: tmp, root }
        }

        fn command(&self, program: &Path, arg0: Option<&OsStr>) -> std::process::Command {
            let mut cmd = std::process::Command::new(program);
            if let Some(a) = arg0 {
                cmd.arg0(a);
            }
            cmd.env_clear()
                .env("HOME", self.root.join("home"))
                .env("XDG_CONFIG_HOME", self.root.join("cfg"))
                .env("XDG_DATA_HOME", self.root.join("share"))
                .env("XDG_STATE_HOME", self.root.join("state"))
                .env("HALI_DATA_DIR", self.root.join("data"))
                .current_dir(self.root.join("cwd"))
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            cmd
        }

        fn finish(mut cmd: std::process::Command, args: &[&str]) -> Out {
            let out = cmd.args(args).output().unwrap();
            Out {
                code: out.status.code(),
                stdout: out.stdout,
                stderr: out.stderr,
            }
        }

        fn run(&self, args: &[&str]) -> Out {
            Env::finish(self.command(Path::new(BIN), None), args)
        }

        fn run_as(&self, program: &Path, arg0: Option<&OsStr>, args: &[&str]) -> Out {
            Env::finish(self.command(program, arg0), args)
        }

        /// Every path under the temp root, sorted, with its size for files.
        fn tree(&self) -> Vec<String> {
            fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
                for e in std::fs::read_dir(dir).unwrap() {
                    let p = e.unwrap().path();
                    let meta = std::fs::symlink_metadata(&p).unwrap();
                    let rel = p.strip_prefix(root).unwrap().display().to_string();
                    out.push(format!(
                        "{rel} {}",
                        if meta.is_dir() { 0 } else { meta.len() }
                    ));
                    if meta.is_dir() {
                        walk(&p, root, out);
                    }
                }
            }
            let mut out = Vec::new();
            walk(&self.root, &self.root, &mut out);
            out.sort();
            out
        }
    }

    /// A successful run: exit 0, empty stderr, non-empty stdout; returns the stdout.
    fn ok(out: &Out, label: &str) -> String {
        assert_eq!(out.code, Some(0), "{label}: exit; stderr: {}", out.err());
        assert_eq!(out.err(), "", "{label}: stderr");
        assert!(!out.stdout.is_empty(), "{label}: empty stdout");
        out.out()
    }

    fn assert_no_escapes(label: &str, text: &str) {
        for c in text.chars().filter(|&c| c != '\n') {
            assert!(!is_escaped(c), "{label}: raw {c:?} in {text:?}");
        }
    }

    /// Every generator invocation (argv after `generate`).
    fn leaves() -> Vec<Vec<&'static str>> {
        let mut v = vec![vec!["man"]];
        for s in SHELLS {
            v.push(vec!["completions", s]);
        }
        v
    }

    fn generate(env: &Env, leaf: &[&str]) -> Out {
        let mut args = vec!["generate"];
        args.extend_from_slice(leaf);
        env.run(&args)
    }

    #[test]
    fn man_page() {
        let env = Env::new();
        let text = ok(&generate(&env, &["man"]), "generate man");
        assert!(
            text.lines().any(|l| l.starts_with(".TH hali 1")),
            "no `.TH hali 1` line:\n{text}"
        );
        assert!(text.contains("describe"), "the page does not list describe");
        assert!(text.contains("find"), "the page does not list find");
        assert!(
            !text.to_lowercase().contains("generate"),
            "the man page mentions the hidden generate"
        );
        assert!(!text.contains("$0"), "the man page holds $0");
    }

    #[test]
    fn completions_each_shell() {
        let env = Env::new();
        for shell in SHELLS {
            let text = ok(
                &generate(&env, &["completions", shell]),
                &format!("completions {shell}"),
            );
            match shell {
                "bash" => assert!(
                    text.lines().any(|l| l.contains("complete -F _hali")
                        && l.trim_end().ends_with(" hali")),
                    "bash: no `complete -F _hali ... hali` line"
                ),
                "zsh" => assert!(text.starts_with("#compdef hali"), "zsh: no #compdef hali"),
                _ => assert!(
                    text.contains("complete -c hali"),
                    "fish: no `complete -c hali`"
                ),
            }
            assert!(text.contains("describe"), "{shell}: describe not offered");
            assert!(text.contains("find"), "{shell}: find not offered");
            assert!(
                !text.to_lowercase().contains("generate"),
                "{shell}: the hidden generate is offered"
            );
            assert!(!text.contains("$0"), "{shell}: the script holds $0");
        }
    }

    type Surface = (
        std::collections::BTreeSet<String>,
        std::collections::BTreeSet<String>,
    );

    /// What `hali --help` offers at the top level: (flags, subcommand names).
    fn help_surface(help: &str) -> Surface {
        let mut flags = std::collections::BTreeSet::new();
        let mut subs = std::collections::BTreeSet::new();
        let mut section = "";
        for line in help.lines() {
            if line == "Commands:" || line == "Options:" {
                section = if line == "Commands:" { "c" } else { "o" };
                continue;
            }
            if !line.starts_with("  ") {
                if !line.trim().is_empty() {
                    section = "";
                }
                continue;
            }
            let body = line.trim();
            match section {
                "c" => {
                    if let Some(name) = body.split_whitespace().next() {
                        subs.insert(name.to_string());
                    }
                }
                "o" => {
                    let head = body.split("  ").next().unwrap_or("");
                    for t in head.split_whitespace() {
                        let t = t.trim_matches(',');
                        if t.starts_with('-') {
                            flags.insert(t.to_string());
                        }
                    }
                }
                _ => {}
            }
        }
        (flags, subs)
    }

    /// The top-level flags and subcommand names a generated script offers.
    fn script_surface(shell: &str, text: &str) -> Surface {
        let mut flags = std::collections::BTreeSet::new();
        let mut subs = std::collections::BTreeSet::new();
        let lines: Vec<&str> = text.lines().collect();
        match shell {
            "bash" => {
                let at = lines
                    .iter()
                    .position(|l| l.trim() == "hali)")
                    .expect("bash: no top-level `hali)` case");
                let opts = lines[at..]
                    .iter()
                    .find(|l| l.trim_start().starts_with("opts=\""))
                    .expect("bash: no top-level opts");
                let inner = opts.split('"').nth(1).unwrap();
                for t in inner.split_whitespace() {
                    if t.starts_with('-') {
                        flags.insert(t.to_string());
                    } else {
                        subs.insert(t.to_string());
                    }
                }
            }
            "zsh" => {
                let end = lines
                    .iter()
                    .position(|l| l.contains(":: :_hali_commands"))
                    .expect("zsh: no top-level command list");
                for l in &lines[..end] {
                    if let Some(rest) = l.strip_prefix('\'') {
                        let flag: String = rest
                            .chars()
                            .take_while(|c| *c != '[' && *c != '=' && *c != '\'')
                            .collect();
                        if flag.starts_with('-') {
                            flags.insert(flag);
                        }
                    }
                }
                let start = lines
                    .iter()
                    .position(|l| l.starts_with("_hali_commands() {"))
                    .expect("zsh: no _hali_commands");
                for l in &lines[start + 1..] {
                    if l.starts_with('}') {
                        break;
                    }
                    if let Some(rest) = l.trim().strip_prefix('\'') {
                        if let Some((name, _)) = rest.split_once(':') {
                            subs.insert(name.to_string());
                        }
                    }
                }
            }
            _ => {
                for l in lines
                    .iter()
                    .filter(|l| l.starts_with("complete -c hali -n \"__fish_hali_needs_command\""))
                {
                    let toks: Vec<&str> = l.split_whitespace().collect();
                    let mut i = 0;
                    while i + 1 < toks.len() {
                        match toks[i] {
                            "-d" => break,
                            "-s" => flags.insert(format!("-{}", toks[i + 1])),
                            "-l" => flags.insert(format!("--{}", toks[i + 1])),
                            "-a" => subs.insert(toks[i + 1].trim_matches('"').to_string()),
                            _ => false,
                        };
                        i += 1;
                    }
                }
            }
        }
        (flags, subs)
    }

    #[test]
    fn completions_match_help() {
        let env = Env::new();
        let help = ok(&env.run(&["--help"]), "--help");
        let (want_flags, want_subs) = help_surface(&help);
        assert!(
            want_flags.contains("-V") && want_flags.contains("--version"),
            "{want_flags:?}"
        );
        assert!(
            want_subs.contains("help") && !want_subs.contains("generate"),
            "{want_subs:?}"
        );
        for shell in SHELLS {
            let text = ok(
                &generate(&env, &["completions", shell]),
                &format!("completions {shell}"),
            );
            let (flags, subs) = script_surface(shell, &text);
            assert_eq!(
                flags, want_flags,
                "{shell}: top-level flags differ from --help"
            );
            assert_eq!(
                subs, want_subs,
                "{shell}: top-level subcommands differ from --help"
            );
            for marker in [
                "__subcmd__generate",
                "'generate:",
                "-a \"generate\"",
                "_subcommand generate",
            ] {
                assert!(!text.contains(marker), "{shell}: the script holds {marker}");
            }
        }
    }

    #[test]
    fn man_header_carries_the_version() {
        let env = Env::new();
        let text = ok(&generate(&env, &["man"]), "generate man");
        let want = format!("hali {}", env!("CARGO_PKG_VERSION"));
        let th = text
            .lines()
            .find(|l| l.starts_with(".TH hali 1"))
            .expect("no .TH line");
        assert!(th.contains(&want), "the .TH line lacks `{want}`: {th}");
    }

    #[test]
    #[ignore = "needs bash on PATH (make test-external)"]
    fn completions_parse() {
        let env = Env::new();
        let text = ok(
            &generate(&env, &["completions", "bash"]),
            "completions bash",
        );
        let file = env.root.join("hali.bash");
        std::fs::write(&file, text).unwrap();
        let status = std::process::Command::new("bash")
            .arg("-n")
            .arg(&file)
            .status()
            .unwrap();
        assert!(status.success(), "bash -n rejects the bash completions");
    }

    #[test]
    fn usage_errors() {
        let env = Env::new();
        for shell in ["elvish", "powershell", "zz", "", "BASH", H] {
            let out = env.run(&["generate", "completions", shell]);
            let label = format!("completions {shell:?}");
            assert_eq!(out.code, Some(2), "{label}: exit; stderr: {}", out.err());
            assert!(out.stdout.is_empty(), "{label}: stdout not empty");
            assert!(
                out.err().contains("bash, zsh, fish"),
                "{label}: stderr does not list the shells: {}",
                out.err()
            );
            assert_no_escapes(&label, &out.err());
        }
        let out = env.run(&["generate", "completions"]);
        assert_eq!(out.code, Some(2), "missing shell: {}", out.err());
        assert!(out.stdout.is_empty(), "missing shell: stdout not empty");
        assert!(
            out.err().to_lowercase().contains("shell"),
            "missing shell: {}",
            out.err()
        );
        let out = env.run(&["generate", "man", "extra"]);
        assert_eq!(out.code, Some(2), "man extra: {}", out.err());
        assert!(out.stdout.is_empty());
        let out = env.run(&["generate"]);
        assert_eq!(out.code, Some(2), "bare generate: {}", out.err());
        assert!(out.stdout.is_empty(), "bare generate: stdout not empty");
        assert!(
            out.err().contains("Usage: hali generate"),
            "bare generate prints no help on stderr: {}",
            out.err()
        );
        assert!(out.err().contains("completions") && out.err().contains("man"));
    }

    #[test]
    fn hidden() {
        let cmd = hali::cli::command();
        let generate = cmd
            .get_subcommands()
            .find(|c| c.get_name() == "generate")
            .expect("no `generate` subcommand in the clap tree");
        assert!(generate.is_hide_set(), "`generate` is not hidden");
        for leaf in ["man", "completions"] {
            assert!(
                generate.get_subcommands().any(|c| c.get_name() == leaf),
                "`generate {leaf}` is missing from the clap tree"
            );
        }
        let env = Env::new();
        for args in [vec!["--help"], vec!["help"], vec!["-h"]] {
            let out = env.run(&args);
            assert_eq!(out.code, Some(0), "{args:?}: {}", out.err());
            assert!(
                !out.out().to_lowercase().contains("generate"),
                "{args:?} mentions generate"
            );
        }
        for args in [vec!["generate", "--help"], vec!["help", "generate"]] {
            let out = env.run(&args);
            assert_eq!(out.code, Some(0), "{args:?}: {}", out.err());
            assert!(
                out.out().contains("man") && out.out().contains("completions"),
                "{args:?}: {}",
                out.out()
            );
        }
    }

    #[test]
    fn argv0_independent() {
        let env = Env::new();
        let names = [
            "\n".to_string(),
            "x\nerror: forged".to_string(),
            "\r".to_string(),
            "\t".to_string(),
            "\u{1b}[2J".to_string(),
            "\u{1b}]0;PWN\u{7}".to_string(),
            H.to_string(),
            "evil".to_string(),
            "/some/dir/evil".to_string(),
        ];
        let links = env.root.join("links");
        std::fs::create_dir_all(&links).unwrap();
        let plain = links.join("hali-link");
        symlink(BIN, &plain).unwrap();
        let hostile_link = links.join("x\nerror: forged");
        symlink(BIN, &hostile_link).unwrap();
        for leaf in leaves() {
            let mut args = vec!["generate"];
            args.extend_from_slice(&leaf);
            let want = env.run_as(Path::new(BIN), Some(OsStr::new("hali")), &args);
            let want_text = ok(&want, &format!("{leaf:?} as hali"));
            for name in &names {
                let got = env.run_as(Path::new(BIN), Some(OsStr::new(name)), &args);
                assert_eq!(got.code, Some(0), "{leaf:?} as {name:?}: {}", got.err());
                assert_eq!(got.stdout, want.stdout, "{leaf:?} as {name:?}: stdout");
                assert_eq!(got.stderr, want.stderr, "{leaf:?} as {name:?}: stderr");
            }
            for link in [&plain, &hostile_link] {
                let got = env.run_as(link, None, &args);
                assert_eq!(got.code, Some(0), "{leaf:?} via {link:?}: {}", got.err());
                assert_eq!(got.stdout, want.stdout, "{leaf:?} via {link:?}: stdout");
            }
            for banned in ["evil", "hali-link", "forged"] {
                assert!(!want_text.contains(banned), "{leaf:?} mentions {banned}");
            }
        }
    }

    #[test]
    fn deterministic_and_storeless() {
        let env = Env::new();
        for leaf in leaves() {
            let label = format!("{leaf:?}");
            let base = ok(&generate(&env, &leaf), &label);
            assert_eq!(base, ok(&generate(&env, &leaf), &label), "{label}: rerun");
            // No temp, home or binary path leaks into the output.
            let root = env.root.display().to_string();
            assert!(!base.contains(&root), "{label}: holds the temp dir");
            assert!(!base.contains(BIN), "{label}: holds the binary path");
            assert!(!base.contains("/tmp/"), "{label}: holds a temp path");

            // Other HOME, LANG, TZ, SOURCE_DATE_EPOCH and cwd.
            let other = Env::new();
            let mut cmd = other.command(Path::new(BIN), None);
            cmd.env("LANG", "de_DE.UTF-8")
                .env("TZ", "Pacific/Auckland")
                .env("SOURCE_DATE_EPOCH", "86400")
                .env("HOME", "/nonexistent-home")
                .current_dir("/");
            let mut args = vec!["generate"];
            args.extend_from_slice(&leaf);
            let got = Env::finish(cmd, &args);
            assert_eq!(ok(&got, &label), base, "{label}: environment changed it");

            // No environment variable at all.
            let mut cmd = std::process::Command::new(BIN);
            cmd.env_clear().stdin(Stdio::null());
            let got = Env::finish(cmd, &args);
            assert_eq!(ok(&got, &label), base, "{label}: empty environment");
        }
    }

    #[test]
    fn broken_store_and_config_are_ignored() {
        let env = Env::new();
        let file = env.root.join("not-a-dir");
        std::fs::write(&file, "x").unwrap();
        std::fs::create_dir_all(env.root.join("cfg/hali")).unwrap();
        std::fs::write(env.root.join("cfg/hali/config.toml"), "[[[ not toml").unwrap();
        std::fs::write(env.root.join("state/hali"), "a file, not a directory").unwrap();
        let before = env.tree();
        for leaf in leaves() {
            let label = format!("{leaf:?}");
            let mut args = vec!["generate"];
            args.extend_from_slice(&leaf);
            let mut cmd = env.command(Path::new(BIN), None);
            cmd.env("HALI_DATA_DIR", &file);
            let got = ok(&Env::finish(cmd, &args), &label);
            let clean = Env::new();
            assert_eq!(got, ok(&generate(&clean, &leaf), &label), "{label}");
            let mut cmd = env.command(Path::new(BIN), None);
            cmd.env("HALI_DATA_DIR", env.root.join("missing/deeper"));
            ok(&Env::finish(cmd, &args), &label);
        }
        assert_eq!(env.tree(), before, "generate created or changed files");
        assert!(
            !env.root.join("missing").exists(),
            "generate created a data dir"
        );
    }

    #[test]
    fn global_flags_ignored() {
        let env = Env::new();
        let flag_sets: [&[&str]; 5] = [
            &["--json"],
            &["--dry-run"],
            &["--yes"],
            &["--json", "--dry-run", "--yes"],
            &["--yes", "--dry-run"],
        ];
        for leaf in leaves() {
            let label = format!("{leaf:?}");
            let want = ok(&generate(&env, &leaf), &label);
            for flags in flag_sets {
                let mut before = flags.to_vec();
                before.push("generate");
                before.extend_from_slice(&leaf);
                let got = ok(&env.run(&before), &format!("{label} {flags:?} before"));
                assert_eq!(got, want, "{label} {flags:?} before the subcommand");
                let mut after = vec!["generate"];
                after.extend_from_slice(&leaf);
                after.extend_from_slice(flags);
                let got = ok(&env.run(&after), &format!("{label} {flags:?} after"));
                assert_eq!(got, want, "{label} {flags:?} after the leaf");
            }
        }
    }
}
