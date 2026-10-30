//! Goal tests for `hali forget` and the confirmation of destructive commands through the real
//! binary (#14, spec §2.6, §9.1, §9.4, §9.5, §9.8, §14.2). Locked golden snapshots sit in
//! tests/snapshots/ (hand-written from the spec and the issue's contract):
//! `cli_forget__forget_json`, `cli_dry_run__forget_dry_run_json` (stdout) and the stderr
//! envelope `cli_forget__confirmation_required_json_envelope`.
//!
//! Every run sets HALI_DATA_DIR / XDG_CONFIG_HOME / HOME to temp dirs and clears the rest of
//! the environment, so the real store and config are never read. The store is
//! tests/fixtures/forget with its `file:///fixtures/forget/` URLs pointed at real files (six
//! bytes, mtime 2026-10-20T14:02:11Z plus nanoseconds) under the temp root. The temp root is
//! canonicalised and shown as `/TMP` in captured output. `forget` mints no id, so every run
//! uses `HALI_TEST_ID_SEED=1`.
//!
//! The terminal seam is `HALI_TEST_TTY=1|0` (test-hooks builds only, spec §14.2): `1` makes
//! `forget` treat stdin and stderr as terminals, `0` (or unset, with piped stdio) as not. The
//! prompt is fixed in spec §9.8: `forget resource <id> "<title>" <path>? [y/N] ` on stderr,
//! no trailing newline.

mod goal {
    use assert_cmd::Command;
    use serde_json::Value;
    use std::collections::BTreeMap;
    use std::ffi::OsStr;
    use std::io::Write;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant, SystemTime};
    use tempfile::TempDir;

    const R1: &str = "0192f0c4-0000-7000-8000-000000000201";
    const R2: &str = "0192f0c4-0000-7000-8000-000000000202";
    const R3: &str = "0192f0c4-0000-7000-8000-000000000203";
    const NOW: &str = "2026-10-24T09:30:00Z";
    const MTIME_SECS: u64 = 1_792_504_931; // 2026-10-20T14:02:11Z
    const FIX: &str =
        "Run it again with --yes to confirm, or with --dry-run to see what it would change.";
    const NOT_A_TTY: &str =
        "forget needs confirmation, but stdin or stderr is not a terminal; nothing was written";
    const DECLINED: &str = "forget was not confirmed; nothing was written";

    struct Env {
        _root: TempDir,
        root: PathBuf,
    }

    struct Out {
        code: Option<i32>,
        stdout: String,
        stderr: String,
    }

    impl Env {
        /// The `forget` fixture, with real files one.txt, two.txt and three.txt under files/.
        fn new() -> Env {
            let tmp = tempfile::tempdir().unwrap();
            let root = std::fs::canonicalize(tmp.path()).unwrap();
            for d in ["cfg", "home", "files", "data"] {
                std::fs::create_dir_all(root.join(d)).unwrap();
            }
            let env = Env { _root: tmp, root };
            let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/forget");
            for entry in std::fs::read_dir(&fixture).unwrap() {
                let entry = entry.unwrap();
                let text = std::fs::read_to_string(entry.path()).unwrap().replace(
                    "file:///fixtures/forget/",
                    &format!("file://{}/files/", env.root.display()),
                );
                std::fs::write(env.data().join(entry.file_name()), text).unwrap();
            }
            for name in ["one.txt", "two.txt", "three.txt"] {
                let path = env.root.join("files").join(name);
                std::fs::write(&path, b"hello\n").unwrap();
                let t = SystemTime::UNIX_EPOCH + Duration::new(MTIME_SECS, 987_654_321);
                std::fs::File::open(&path).unwrap().set_modified(t).unwrap();
            }
            env
        }

        fn data(&self) -> PathBuf {
            self.root.join("data")
        }

        /// The real path of a file under files/ (output shows it as `/TMP/files/<name>`).
        fn file(&self, name: &str) -> String {
            self.root
                .join("files")
                .join(name)
                .to_str()
                .unwrap()
                .to_string()
        }

        fn cmd(&self, extra_env: &[(&str, &str)]) -> Command {
            let mut cmd = Command::cargo_bin("hali").unwrap();
            cmd.env_clear()
                .env("XDG_CONFIG_HOME", self.root.join("cfg"))
                .env("HOME", self.root.join("home"))
                .env("HALI_DATA_DIR", self.data())
                .env("HALI_TEST_NOW", NOW)
                .env("HALI_TEST_ID_SEED", "1")
                .timeout(Duration::from_secs(30));
            for (k, v) in extra_env {
                cmd.env(k, v);
            }
            cmd
        }

        /// Runs with the given extra environment and stdin bytes.
        fn exec<I, S>(&self, extra_env: &[(&str, &str)], stdin: &[u8], args: I) -> Out
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            let out = self
                .cmd(extra_env)
                .write_stdin(stdin.to_vec())
                .args(args)
                .output()
                .unwrap();
            let root = self.root.to_str().unwrap();
            let clean = |bytes: Vec<u8>| String::from_utf8(bytes).unwrap().replace(root, "/TMP");
            Out {
                code: out.status.code(),
                stdout: clean(out.stdout),
                stderr: clean(out.stderr),
            }
        }

        /// No terminal: piped stdin holding `y`, which must never be read as an answer.
        fn run<I, S>(&self, args: I) -> Out
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            self.exec(&[], b"y\n", args)
        }

        /// A terminal (`HALI_TEST_TTY=1`) whose user types `answer`.
        fn tty<I, S>(&self, answer: &[u8], args: I) -> Out
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            self.exec(&[("HALI_TEST_TTY", "1")], answer, args)
        }

        /// The names and bytes of every file in the data dir.
        fn state(&self) -> BTreeMap<String, Vec<u8>> {
            std::fs::read_dir(self.data())
                .unwrap()
                .map(|e| {
                    let e = e.unwrap();
                    (
                        e.file_name().into_string().unwrap(),
                        std::fs::read(e.path()).unwrap(),
                    )
                })
                .collect()
        }

        fn resources_ttl(&self) -> String {
            std::fs::read_to_string(self.data().join("resources.ttl")).unwrap()
        }
    }

    fn json(s: &str) -> Value {
        assert_eq!(
            s.matches('\n').count(),
            1,
            "one compact line expected: {s:?}"
        );
        serde_json::from_str(s).unwrap_or_else(|e| panic!("not JSON ({e}): {s:?}"))
    }

    fn ok(out: &Out) -> Value {
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        json(&out.stdout)
    }

    /// The error envelope of a failed run: exit code `code`, nothing on stdout.
    fn failed(out: &Out, code: i32, rule: &str) -> Value {
        assert_eq!(out.code, Some(code), "stderr: {}", out.stderr);
        assert_eq!(out.stdout, "");
        let v = json(&out.stderr);
        assert_eq!(v["error"]["rule"], rule, "{}", out.stderr);
        v
    }

    /// The prompt (spec §9.8) for forgetting R<n> by its fixture title and file.
    fn prompt(id: &str, title: &str, file: &str) -> String {
        format!("forget resource {id} \"{title}\" /TMP/files/{file}? [y/N] ")
    }

    fn retry(id: &str) -> String {
        format!("hali forget {id} --yes")
    }

    // ---- criterion 5 (§14.1): golden snapshots --------------------------------------------------

    #[test]
    fn forget_json() {
        let env = Env::new();
        let out = env.run(["--json", "forget", R1, "--yes"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!("cli_forget__forget_json", out.stdout);
        });
        assert!(!env.resources_ttl().contains(R1));
    }

    #[test]
    fn forget_dry_run_json() {
        // R2: a format node R3 still uses, an inbound relation from R3, and a catalog record.
        let env = Env::new();
        let before = env.state();
        let out = env.run(["--json", "--dry-run", "forget", R2, "--force"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!("cli_dry_run__forget_dry_run_json", out.stdout);
        });
        assert_eq!(env.state(), before, "a dry run writes nothing");
    }

    #[test]
    fn an_overlong_answer_declines() {
        let env = Env::new();
        let before = env.state();
        // Trimmed it would read `y`, but an answer is capped at 64 bytes.
        let answer = format!("y{}\n", " ".repeat(100));
        let out = env.tty(answer.as_bytes(), ["--json", "forget", R1]);
        assert_eq!(out.code, Some(1), "{}", out.stderr);
        assert!(out.stderr.contains(DECLINED), "{}", out.stderr);
        assert_eq!(env.state(), before);
        let out = env.tty(b"y\n", ["--json", "forget", R1]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
    }

    #[test]
    fn confirmation_required_json_envelope() {
        let env = Env::new();
        let before = env.state();
        let out = env.run(["--json", "forget", R1]);
        assert_eq!(out.code, Some(1), "{}", out.stderr);
        assert_eq!(out.stdout, "");
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!(
                "cli_forget__confirmation_required_json_envelope",
                out.stderr
            );
        });
        assert_eq!(env.state(), before);

        // The suggested command names the full UUID, whatever the reference was; the
        // `--json` flag may come after the subcommand.
        let by_path = env.run(["forget", &env.file("one.txt"), "--json"]);
        assert_eq!(by_path.stderr, out.stderr);
        assert_eq!(by_path.code, Some(1));
    }

    // ---- criterion 3: the confirmation rule -----------------------------------------------------

    #[test]
    fn no_tty_requires_confirmation() {
        let env = Env::new();
        let before = env.state();

        // Text mode: one `error:` line and one `hint:` line; the `y` on piped stdin is not an
        // answer.
        let out = env.run(["forget", R1]);
        assert_eq!(out.code, Some(1));
        assert_eq!(out.stdout, "");
        assert_eq!(
            out.stderr,
            format!("error: {NOT_A_TTY}\nhint: {FIX} Try: {}\n", retry(R1))
        );
        assert_eq!(env.state(), before, "nothing was written");

        // JSON details; an explicit "not a terminal" behaves the same.
        for (extra, args) in [
            (&[][..], vec!["--json", "forget", R1]),
            (&[("HALI_TEST_TTY", "0")][..], vec!["--json", "forget", R1]),
            (&[("HALI_TEST_TTY", "0")][..], vec!["forget", R1, "--json"]),
        ] {
            let out = env.exec(extra, b"y\n", args);
            let v = failed(&out, 1, "CONFIRMATION_REQUIRED");
            assert_eq!(v["error"]["message"], NOT_A_TTY);
            assert_eq!(
                v["error"]["details"],
                serde_json::json!({"command": "forget", "declined": false})
            );
            assert_eq!(v["error"]["suggested_fix"], FIX);
            assert_eq!(v["error"]["suggested_command"], retry(R1));
            assert_eq!(env.state(), before);
        }

        // With `--yes` it goes ahead, still with no terminal.
        let out = env.run(["forget", R1, "--yes"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_ne!(env.state(), before);
    }

    #[test]
    fn yes_is_global() {
        // Before or after the subcommand, text or JSON; each on a fresh store.
        for args in [
            vec!["forget", R1, "--yes"],
            vec!["--yes", "forget", R1],
            vec!["forget", "--yes", R1],
            vec!["--json", "--yes", "forget", R1],
            vec!["forget", R1, "--yes", "--json"],
        ] {
            let env = Env::new();
            let out = env.run(args.clone());
            assert_eq!(out.code, Some(0), "{args:?}: {}", out.stderr);
            assert!(!env.resources_ttl().contains(R1), "{args:?}");
        }

        // Commands that are not destructive accept it and ignore it.
        let env = Env::new();
        for args in [
            vec!["--yes", "find"],
            vec!["find", "--yes"],
            vec!["--yes", "validate"],
            vec!["concept", "list", "--yes"],
            vec!["show", R2, "--yes"],
            vec!["--yes", "schema"],
            vec!["--yes", "--dry-run", "edit", R2, "--title", "x"],
        ] {
            let with = env.tty(b"n\n", args.clone());
            let without: Vec<&str> = args.iter().copied().filter(|a| *a != "--yes").collect();
            let without = env.tty(b"n\n", without);
            assert_eq!(with.code, Some(0), "{args:?}: {}", with.stderr);
            assert_eq!(with.stdout, without.stdout, "{args:?}");
            assert_eq!(with.stderr, without.stderr, "{args:?}");
        }
        // An edit changes something, and still never asks, on a terminal or not.
        let out = env.tty(b"n\n", ["edit", R2, "--title", "renamed", "--yes"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        let out = env.tty(b"n\n", ["edit", R2, "--title", "renamed again"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "", "edit does not ask");
    }

    #[test]
    fn tty_prompt_yes_no_eof() {
        let title = "one doc";
        let ask = prompt(R1, title, "one.txt");

        // y and yes, trimmed, in any case, confirm. The prompt is the only thing on stderr and
        // the summary is the only thing on stdout (spec §9.8).
        for answer in [
            "y\n",
            "yes\n",
            "Y\n",
            "YES\n",
            "Yes\n",
            "  y  \n",
            "\tyes\r\n",
        ] {
            let env = Env::new();
            let out = env.tty(answer.as_bytes(), ["forget", R1]);
            assert_eq!(out.code, Some(0), "{answer:?}: {}", out.stderr);
            assert_eq!(out.stderr, ask, "{answer:?}");
            assert_eq!(
                out.stdout,
                format!("forgot resource {R1} \"{title}\" /TMP/files/one.txt\n"),
                "{answer:?}"
            );
            assert!(!env.resources_ttl().contains(R1), "{answer:?}");
        }

        // Anything else, end of input, or an unreadable line declines, with the same rule.
        let declines: [&[u8]; 9] = [
            b"n\n",
            b"N\n",
            b"no\n",
            b"\n",
            b"",
            b"maybe\n",
            b"yess\n",
            b"y e s\n",
            b"\xff\xfe\n",
        ];
        for answer in declines {
            let env = Env::new();
            let before = env.state();
            let out = env.tty(answer, ["forget", R1]);
            assert_eq!(out.code, Some(1), "{answer:?}: {}", out.stderr);
            assert_eq!(out.stdout, "", "{answer:?}");
            assert!(out.stderr.starts_with(&ask), "{answer:?}: {}", out.stderr);
            assert!(
                out.stderr.contains(&format!("error: {DECLINED}")),
                "{answer:?}: {}",
                out.stderr
            );
            assert_eq!(env.state(), before, "{answer:?}: nothing was written");

            // `--json` changes nothing about the prompt; the envelope follows it on stderr.
            let out = env.tty(answer, ["--json", "forget", R1]);
            assert_eq!(out.stdout, "", "{answer:?}");
            let rest = out
                .stderr
                .strip_prefix(&ask)
                .unwrap_or_else(|| panic!("{answer:?}: no prompt first: {}", out.stderr));
            let v = json(rest.trim_start_matches('\n'));
            assert_eq!(v["error"]["rule"], "CONFIRMATION_REQUIRED");
            assert_eq!(v["error"]["message"], DECLINED);
            assert_eq!(
                v["error"]["details"],
                serde_json::json!({"command": "forget", "declined": true})
            );
            assert_eq!(v["error"]["suggested_command"], retry(R1));
            assert_eq!(out.code, Some(1));
            assert_eq!(env.state(), before);
        }

        // `--json` and a yes: stdout is the clean payload, the prompt is on stderr.
        let env = Env::new();
        let out = env.tty(b"y\n", ["--json", "forget", R1]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, ask);
        assert_eq!(json(&out.stdout)["resource"]["id"], R1);
    }

    #[test]
    fn yes_skips_prompt_on_tty() {
        let env = Env::new();
        // The answer on stdin is `n`: with --yes it is never asked for.
        let out = env.tty(b"n\n", ["forget", R1, "--yes"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        assert!(!env.resources_ttl().contains(R1));
    }

    #[test]
    fn dry_run_never_asks() {
        let env = Env::new();
        let before = env.state();
        for (extra, stdin) in [
            (&[][..], &b"n\n"[..]),
            (&[("HALI_TEST_TTY", "0")][..], &b""[..]),
            (&[("HALI_TEST_TTY", "1")][..], &b"n\n"[..]),
            (&[("HALI_TEST_TTY", "1")][..], &b""[..]),
        ] {
            // Text: the note on stderr and nothing else; no prompt, no CONFIRMATION_REQUIRED.
            let out = env.exec(extra, stdin, ["--dry-run", "forget", R1]);
            assert_eq!(out.code, Some(0), "{extra:?}: {}", out.stderr);
            assert_eq!(
                out.stderr, "note: dry run; nothing was written\n",
                "{extra:?}"
            );
            let first = out.stdout.lines().next().unwrap();
            assert_eq!(
                first,
                format!("would forget resource {R1} \"one doc\" /TMP/files/one.txt")
            );
            // JSON: nothing on stderr.
            let out = env.exec(extra, stdin, ["forget", R1, "--json", "--dry-run"]);
            assert_eq!(out.code, Some(0), "{extra:?}: {}", out.stderr);
            assert_eq!(out.stderr, "", "{extra:?}");
            let v = json(&out.stdout);
            assert_eq!(v["dry_run"], true);
            assert_eq!(env.state(), before, "{extra:?}");
        }
    }

    #[test]
    fn never_waits_on_stdin() {
        // stdin is a pipe that stays open and is never written: a command that read it would
        // hang. Each run must exit on its own, with the code of its own path, well inside 10 s.
        let runs: [(&[&str], i32); 3] = [
            (&["forget", R1], 1),
            (&["forget", R1, "--yes"], 0),
            (&["--dry-run", "forget", R1], 0),
        ];
        for (args, want) in runs {
            let env = Env::new();
            let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_hali"))
                .env_clear()
                .env("XDG_CONFIG_HOME", env.root.join("cfg"))
                .env("HOME", env.root.join("home"))
                .env("HALI_DATA_DIR", env.data())
                .env("HALI_TEST_NOW", NOW)
                .env("HALI_TEST_ID_SEED", "1")
                .args(args)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            let held_open = child.stdin.take().expect("piped stdin");
            let deadline = Instant::now() + Duration::from_secs(10);
            let status = loop {
                if let Some(status) = child.try_wait().unwrap() {
                    break status;
                }
                if Instant::now() > deadline {
                    child.kill().ok();
                    child.wait().ok();
                    panic!("`hali {args:?}` was still waiting after 10 s: it reads stdin");
                }
                std::thread::sleep(Duration::from_millis(20));
            };
            assert_eq!(status.code(), Some(want), "`hali {args:?}`");
            drop(held_open);
        }
    }

    // ---- criterion 4: dry run -------------------------------------------------------------------

    #[test]
    fn forget_text_summaries() {
        let env = Env::new();
        let out = env.run(["--dry-run", "forget", R2, "--force"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "note: dry run; nothing was written\n");
        let lines: Vec<&str> = out.stdout.lines().collect();
        assert_eq!(
            lines[0],
            format!("would forget resource {R2} \"two doc\" /TMP/files/two.txt")
        );
        // The shared change lines: every one a removal, the whole triple in byte order.
        assert_eq!(lines.len(), 16, "{}", out.stdout);
        assert!(lines[1..].iter().all(|l| l.starts_with("- <")), "{lines:?}");
        assert!(lines[1..].windows(2).all(|w| w[0] < w[1]), "{lines:?}");

        let out = env.run(["forget", R2, "--force", "--yes"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(
            out.stdout,
            format!("forgot resource {R2} \"two doc\" /TMP/files/two.txt\n")
        );
        assert_eq!(out.stderr, "");
    }

    // ---- criterion 1: what is removed -----------------------------------------------------------

    #[test]
    fn forget_twice_is_unknown_resource() {
        let env = Env::new();
        ok(&env.run(["--json", "forget", R1, "--yes"]));
        let before = env.state();
        let v = failed(
            &env.run(["--json", "forget", R1, "--yes"]),
            1,
            "UNKNOWN_RESOURCE",
        );
        assert_eq!(v["error"]["details"]["ref"], R1);
        // By path too: it is no longer cataloged. And domain errors come before the
        // confirmation: without --yes it is the same error, not CONFIRMATION_REQUIRED.
        let path = env.file("one.txt");
        failed(&env.run(["--json", "forget", &path]), 1, "UNKNOWN_RESOURCE");
        failed(
            &env.tty(b"y\n", ["--json", "forget", &path]),
            1,
            "UNKNOWN_RESOURCE",
        );
        // A UUID that was never cataloged.
        failed(
            &env.run(["--json", "forget", "0192f0c4-0000-7000-8000-00000000ffff"]),
            1,
            "UNKNOWN_RESOURCE",
        );
        assert_eq!(env.state(), before);
    }

    #[test]
    fn forget_leaves_other_resources_and_only_resources_ttl_changes() {
        let env = Env::new();
        // A hand-written extensions.ttl that must survive byte for byte.
        std::fs::write(
            env.data().join("extensions.ttl"),
            "# mine\n@prefix hali: <https://w3id.org/hali#> .\nhali:bird a hali:Thing .\n",
        )
        .unwrap();
        let before = env.state();
        ok(&env.run(["--json", "forget", R1, "--yes"]));
        let after = env.state();
        assert_eq!(
            before.keys().collect::<Vec<_>>(),
            after.keys().collect::<Vec<_>>(),
            "no file was added or removed in the data dir"
        );
        for name in ["vocabulary.ttl", "extensions.ttl"] {
            assert_eq!(before[name], after[name], "{name} is never written");
        }
        assert_ne!(before["resources.ttl"], after["resources.ttl"]);

        let v = ok(&env.run(["--json", "find"]));
        let mut ids: Vec<&str> = v["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["id"].as_str().unwrap())
            .collect();
        ids.sort();
        assert_eq!(ids, [R2, R3]);
        failed(&env.run(["--json", "show", R1]), 1, "UNKNOWN_RESOURCE");
        ok(&env.run(["--json", "show", R3]));
    }

    #[test]
    fn last_user_of_a_format_drops_its_node() {
        let env = Env::new();
        ok(&env.run(["--json", "forget", R2, "--force", "--yes"]));
        assert!(
            env.resources_ttl().contains("x-forget"),
            "R3 still uses the format"
        );
        ok(&env.run(["--json", "forget", R3, "--yes"]));
        assert!(
            !env.resources_ttl().contains("x-forget"),
            "the last user took the format node with it:\n{}",
            env.resources_ttl()
        );
    }

    // ---- order of checks ------------------------------------------------------------------------

    #[test]
    fn domain_errors_come_before_confirmation() {
        // usage (exit 2) before the store is read.
        let env = Env::new();
        let before = env.state();
        let out = env.run(["forget"]);
        assert_eq!(out.code, Some(2), "{}", out.stderr);
        assert_eq!(out.stdout, "");

        // store problem (exit 3) before everything else.
        std::fs::write(env.data().join("resources.ttl"), "this is not turtle\n").unwrap();
        let broken = env.state();
        failed(&env.run(["--json", "forget", R1]), 3, "STORE_UNPARSEABLE");
        assert_eq!(env.state(), broken);
        std::fs::write(env.data().join("resources.ttl"), &before["resources.ttl"]).unwrap();

        // SHACL (exit 1) before the confirmation: extensions.ttl keeps typing R1 as a
        // hali:Resource, so the result would not conform.
        std::fs::write(
            env.data().join("extensions.ttl"),
            format!(
                "@prefix hali: <https://w3id.org/hali#> .\n<urn:uuid:{R1}> a hali:Resource .\n"
            ),
        )
        .unwrap();
        let held = env.state();
        for out in [
            env.run(["--json", "forget", R1]),
            env.tty(b"y\n", ["--json", "forget", R1]),
            env.run(["--json", "forget", R1, "--yes"]),
        ] {
            failed(&out, 1, "SHACL_VIOLATION");
        }
        assert_eq!(env.state(), held, "nothing was written");
    }

    // ---- criterion 2: the file is untouched -----------------------------------------------------

    #[derive(Debug, PartialEq)]
    struct Stat {
        bytes: Option<Vec<u8>>,
        dev: u64,
        ino: u64,
        size: u64,
        mode: u32,
        nlink: u64,
        mtime: (i64, i64),
        ctime: (i64, i64),
    }

    fn stat(path: &Path) -> Stat {
        let m = std::fs::symlink_metadata(path).unwrap();
        Stat {
            bytes: m.is_file().then(|| std::fs::read(path).unwrap()),
            dev: m.dev(),
            ino: m.ino(),
            size: m.size(),
            mode: m.mode(),
            nlink: m.nlink(),
            mtime: (m.mtime(), m.mtime_nsec()),
            ctime: (m.ctime(), m.ctime_nsec()),
        }
    }

    /// Puts a directory's permissions back, so the temp dir can be removed.
    struct Restore(PathBuf);
    impl Drop for Restore {
        fn drop(&mut self) {
            std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755)).ok();
        }
    }

    fn snapshot_files(env: &Env) -> Vec<(String, Stat)> {
        let dir = env.root.join("files");
        let mut all = vec![("files/".to_string(), stat(&dir))];
        for name in ["one.txt", "two.txt", "three.txt"] {
            all.push((name.to_string(), stat(&dir.join(name))));
        }
        all
    }

    #[test]
    fn forget_leaves_file_untouched() {
        let env = Env::new();
        let before = snapshot_files(&env);
        for id in [R1, R2, R3] {
            ok(&env.run(["--json", "forget", id, "--force", "--yes"]));
        }
        assert_eq!(snapshot_files(&env), before);
        // Four entries: the directory and the three files (bytes, size, mtime to the
        // nanosecond, mode, inode, link count, ctime).
        assert_eq!(before.len(), 4);
    }

    #[test]
    fn forget_leaves_a_read_only_file_in_a_read_only_dir_untouched() {
        let env = Env::new();
        let dir = env.root.join("files");
        for name in ["one.txt", "two.txt", "three.txt"] {
            std::fs::set_permissions(dir.join(name), std::fs::Permissions::from_mode(0o444))
                .unwrap();
        }
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let _restore = Restore(dir.clone());
        let before = snapshot_files(&env);
        for id in [R1, R2, R3] {
            let v = ok(&env.run(["--json", "forget", id, "--force", "--yes"]));
            assert_eq!(v["resource"]["id"], id);
        }
        assert_eq!(snapshot_files(&env), before);
    }

    #[test]
    fn forget_missing_file_by_path_and_uuid() {
        let env = Env::new();
        std::fs::remove_file(env.root.join("files/one.txt")).unwrap();
        std::fs::remove_file(env.root.join("files/two.txt")).unwrap();

        // By UUID: the record shows what `stat` sees, a missing file.
        let v = ok(&env.run(["--json", "forget", R1, "--yes"]));
        assert_eq!(v["resource"]["id"], R1);
        assert_eq!(v["resource"]["path"], "/TMP/files/one.txt");
        assert_eq!(v["resource"]["missing"], true);
        assert_eq!(v["resource"]["stale"], false);

        // By path, though nothing exists there any more.
        let v = ok(&env.run(["--json", "forget", &env.file("two.txt"), "--force", "--yes"]));
        assert_eq!(v["resource"]["id"], R2);
        assert_eq!(v["resource"]["missing"], true);

        // Even with its whole directory gone.
        std::fs::remove_dir_all(env.root.join("files")).unwrap();
        let v = ok(&env.run(["--json", "forget", &env.file("three.txt"), "--yes"]));
        assert_eq!(v["resource"]["id"], R3);
        assert_eq!(v["resource"]["missing"], true);
        assert!(
            !env.resources_ttl()
                .contains("0192f0c4-0000-7000-8000-0000000002")
        );
    }

    // ---- criterion 6: schemas -------------------------------------------------------------------

    #[test]
    fn schema_lists_forget() {
        let env = Env::new();
        let out = env.run(["schema"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert!(out.stdout.lines().any(|l| l == "forget"), "{}", out.stdout);
        let v = ok(&env.run(["schema", "--json"]));
        assert!(
            v["schemas"]
                .as_array()
                .unwrap()
                .iter()
                .any(|n| n == "forget"),
            "{v}"
        );
        let doc = json(&env.run(["schema", "forget"]).stdout);
        assert_eq!(
            doc["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
        assert!(
            doc["oneOf"].as_array().is_some_and(|a| a.len() == 2),
            "{doc}"
        );
    }

    // ---- an interactive session on a real pseudo-terminal -------------------------------------

    #[test]
    #[ignore = "needs script(1) from util-linux"]
    fn real_pty_prompts() {
        for (answer, forgotten) in [("y\n", true), ("n\n", false)] {
            let env = Env::new();
            let command = format!("{} forget {R1}", env!("CARGO_BIN_EXE_hali"));
            let mut child = std::process::Command::new("/usr/bin/script")
                .args(["-qec", &command, "/dev/null"])
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("XDG_CONFIG_HOME", env.root.join("cfg"))
                .env("HOME", env.root.join("home"))
                .env("HALI_DATA_DIR", env.data())
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            let mut stdin = child.stdin.take().unwrap();
            std::thread::sleep(Duration::from_millis(500));
            stdin.write_all(answer.as_bytes()).unwrap();
            stdin.flush().unwrap();
            std::thread::sleep(Duration::from_millis(500));
            drop(stdin);
            let out = child.wait_with_output().unwrap();
            let text = String::from_utf8_lossy(&out.stdout);
            assert!(text.contains("[y/N]"), "{answer:?}: no prompt: {text}");
            assert_eq!(
                env.resources_ttl().contains(R1),
                !forgotten,
                "{answer:?}: {text}"
            );
            if forgotten {
                assert!(text.contains("forgot resource"), "{text}");
            }
        }
    }
}

mod hand_edited_iri {
    use assert_cmd::Command;

    /// A Resource whose IRI is not a UUID, found by path: the retry command is shell-safe.
    #[test]
    fn suggested_command_quotes_a_non_uuid_id() {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        for d in ["cfg", "home", "files", "data"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        let fixture =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/forget");
        for name in ["resources.ttl", "vocabulary.ttl"] {
            let text = std::fs::read_to_string(fixture.join(name))
                .unwrap()
                .replace(
                    "file:///fixtures/forget/",
                    &format!("file://{}/files/", root.display()),
                )
                .replace("0192f0c4-0000-7000-8000-000000000201", "x;id");
            std::fs::write(root.join("data").join(name), text).unwrap();
        }
        let file = root.join("files/one.txt");
        std::fs::write(&file, b"hello\n").unwrap();
        let out = Command::cargo_bin("hali")
            .unwrap()
            .env_clear()
            .env("XDG_CONFIG_HOME", root.join("cfg"))
            .env("HOME", root.join("home"))
            .env("HALI_DATA_DIR", root.join("data"))
            .env("HALI_TEST_ID_SEED", "1")
            .args(["--json", "forget"])
            .arg(&file)
            .write_stdin("y\n")
            .output()
            .unwrap();
        let stderr = String::from_utf8(out.stderr).unwrap();
        assert_eq!(out.status.code(), Some(1), "{stderr}");
        let v: serde_json::Value = serde_json::from_str(&stderr).unwrap();
        assert_eq!(v["error"]["rule"], "CONFIRMATION_REQUIRED", "{stderr}");
        assert_eq!(
            v["error"]["suggested_command"], "hali forget 'x;id' --yes",
            "{stderr}"
        );
    }
}
