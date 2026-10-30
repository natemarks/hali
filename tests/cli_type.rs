//! Goal tests for Types through the real binary (#15, spec §2.2, §9.1, §9.2, §9.4, §9.5, §9.7,
//! §9.8, §14.1, §14.2): `type add|list|remove`, `add --type`, `edit --add-type/--remove-type`
//! and `find --type`. Locked golden snapshots sit in tests/snapshots/ (hand-written from the
//! spec and the issue's contract): `cli_type__{type_add,type_list,add_type,edit_add_remove_type,
//! type_remove,find_type}_json` (stdout) and the stderr envelopes
//! `cli_type__{type_label_taken,unknown_type,type_in_use,type_remove_confirmation_required}_json_envelope`.
//! The dry-run goldens `cli_dry_run__type_{add,remove}_dry_run_json` are checked in
//! tests/cli_dry_run.rs.
//!
//! Every run sets HALI_DATA_DIR / XDG_CONFIG_HOME / HOME to temp dirs and clears the rest of
//! the environment, so the real store and config are never read. The store is
//! tests/fixtures/types (see tests/core_type.rs), with a `# hand-edited` comment line put in
//! front of resources.ttl so that a rewrite of it shows up as a byte difference. The fixture
//! has no user Person, so the first `add` mints the same ids as in tests/cli_tag.rs.
//!
//! The terminal seam is `HALI_TEST_TTY=1|0` (test-hooks builds only, spec §14.2). The prompt of
//! `type remove` is `remove type <id> "<label>"? [y/N] ` on stderr (spec §9.8).
//!
//! Wording the contract leaves open and the goldens and tests fix (change them, with the human,
//! if the wording should differ):
//! - the `LABEL_TAKEN` suggested_fix for a Type is `Use the existing Type, or choose a
//!   different name.` (spec §9.1, Types);
//! - the text of `type list` is one `<id><TAB><label><TAB><parents joined with ", ">` line per
//!   Type; an empty parent list leaves nothing after the last tab;
//! - the `CONFIRMATION_REQUIRED` message names the command as `type remove`; a declined
//!   confirmation says `type remove was not confirmed; nothing was written`.

mod goal {
    use assert_cmd::Command;
    use serde_json::Value;
    use std::collections::BTreeMap;
    use std::ffi::OsStr;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant, SystemTime};
    use tempfile::TempDir;

    const NOW: &str = "2026-10-24T09:30:00Z";
    const MTIME_SECS: u64 = 1_792_504_931; // 2026-10-20T14:02:11Z
    const R1: &str = "0192f0c4-0000-7000-8000-000000000501";
    const FIX: &str =
        "Run it again with --yes to confirm, or with --dry-run to see what it would change.";
    const NOT_A_TTY: &str = "type remove needs confirmation, but stdin or stderr is not a \
                             terminal; nothing was written";
    const DECLINED: &str = "type remove was not confirmed; nothing was written";
    const HALI: &str = "https://w3id.org/hali#";

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
        /// The `types` fixture in the data dir.
        fn new() -> Env {
            let tmp = tempfile::tempdir().unwrap();
            let root = std::fs::canonicalize(tmp.path()).unwrap();
            for d in ["cfg", "home", "data", "files"] {
                std::fs::create_dir_all(root.join(d)).unwrap();
            }
            let env = Env { _root: tmp, root };
            let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/types");
            for entry in std::fs::read_dir(&fixture).unwrap() {
                let entry = entry.unwrap();
                std::fs::copy(entry.path(), env.data().join(entry.file_name())).unwrap();
            }
            let resources = std::fs::read_to_string(env.data().join("resources.ttl")).unwrap();
            std::fs::write(
                env.data().join("resources.ttl"),
                format!("# hand-edited\n{resources}"),
            )
            .unwrap();
            env
        }

        /// A store of this vocabulary alone.
        fn with_vocabulary(vocabulary: &str) -> Env {
            let env = Env::new();
            std::fs::remove_file(env.data().join("resources.ttl")).unwrap();
            std::fs::remove_file(env.data().join("extensions.ttl")).unwrap();
            std::fs::write(env.data().join("vocabulary.ttl"), vocabulary).unwrap();
            env
        }

        /// No data dir at all.
        fn without_store() -> Env {
            let env = Env::new();
            std::fs::remove_dir_all(env.data()).unwrap();
            env
        }

        fn data(&self) -> PathBuf {
            self.root.join("data")
        }

        /// A file under files/ with the pinned mtime (plus nanoseconds).
        fn file(&self, name: &str) -> String {
            let path = self.root.join("files").join(name);
            std::fs::write(&path, b"hello\n").unwrap();
            let t = SystemTime::UNIX_EPOCH + Duration::new(MTIME_SECS, 987_654_321);
            std::fs::File::open(&path).unwrap().set_modified(t).unwrap();
            path.to_str().unwrap().to_string()
        }

        fn cmd(&self, seed: &str, extra_env: &[(&str, &str)]) -> Command {
            let mut cmd = Command::cargo_bin("hali").unwrap();
            cmd.env_clear()
                .env("XDG_CONFIG_HOME", self.root.join("cfg"))
                .env("HOME", self.root.join("home"))
                .env("HALI_DATA_DIR", self.data())
                .env("HALI_TEST_NOW", NOW)
                .env("HALI_TEST_ID_SEED", seed)
                .timeout(Duration::from_secs(30));
            for (k, v) in extra_env {
                cmd.env(k, v);
            }
            cmd
        }

        /// Runs with a seed, extra environment and stdin bytes. The temp root shows as `/TMP`.
        fn exec_seeded<I, S>(
            &self,
            seed: &str,
            extra_env: &[(&str, &str)],
            stdin: &[u8],
            args: I,
        ) -> Out
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            let out = self
                .cmd(seed, extra_env)
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

        fn exec<I, S>(&self, extra_env: &[(&str, &str)], stdin: &[u8], args: I) -> Out
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            self.exec_seeded("1", extra_env, stdin, args)
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

        fn file_bytes(&self, name: &str) -> Vec<u8> {
            std::fs::read(self.data().join(name)).unwrap()
        }

        fn vocabulary(&self) -> String {
            String::from_utf8(self.file_bytes("vocabulary.ttl")).unwrap()
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

    fn prompt(id: &str, label: &str) -> String {
        format!("remove type {id} \"{label}\"? [y/N] ")
    }

    fn retry(id: &str) -> String {
        format!("hali type remove {id} --yes")
    }

    fn snapshot_stdout(name: &str, out: &Out) {
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        // The locked snapshots are named without the `goal__` module segment.
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!(name, out.stdout);
        });
    }

    fn snapshot_stderr(name: &str, code: i32, out: &Out) {
        assert_eq!(out.code, Some(code), "{}", out.stderr);
        assert_eq!(out.stdout, "");
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!(name, out.stderr);
        });
    }

    // ---- golden snapshots (§14.1) -------------------------------------------------------------

    #[test]
    fn type_add_json() {
        let env = Env::new();
        let out = env.run(["--json", "type", "add", "Report", "--parent", "Document"]);
        snapshot_stdout("cli_type__type_add_json", &out);
        assert!(env.vocabulary().contains("hali:Report a owl:Class"));
    }

    #[test]
    fn type_list_json() {
        let env = Env::new();
        let before = env.state();
        let out = env.run(["--json", "type", "list"]);
        snapshot_stdout("cli_type__type_list_json", &out);
        assert_eq!(env.state(), before);
    }

    #[test]
    fn type_label_taken_json_envelope() {
        let env = Env::new();
        let before = env.state();
        let out = env.run(["--json", "type", "add", "paper"]);
        snapshot_stderr("cli_type__type_label_taken_json_envelope", 1, &out);
        assert_eq!(env.state(), before);
        // Text mode: one `error:` line and one `hint:` line.
        let out = env.run(["type", "add", "paper"]);
        assert_eq!(out.code, Some(1));
        assert_eq!(
            out.stderr,
            "error: label \"paper\"@en is already used by Type Paper\n\
             hint: Use the existing Type, or choose a different name. Try: hali type list\n"
        );
        // A folded id of an extensions-only Type, and a name already used in the store.
        failed(
            &env.run(["--json", "type", "add", "MEMO"]),
            1,
            "LABEL_TAKEN",
        );
        let v = failed(&env.run(["--json", "type", "add", "ml"]), 2, "USAGE");
        assert!(
            v["error"]["message"]
                .as_str()
                .unwrap()
                .contains("hali:ml already names something in the store; choose another name"),
            "{v}"
        );
        assert_eq!(env.state(), before);
    }

    #[test]
    fn add_type_json() {
        let env = Env::new();
        let path = env.file("notes.txt");
        let out = env.run([
            "--json", "add", &path, "--type", "paper", "--type", "Memo", "--tag", "ml",
        ]);
        snapshot_stdout("cli_type__add_type_json", &out);
        // Typed in resources.ttl, which still says it is a hali:Resource.
        let resources = String::from_utf8(env.file_bytes("resources.ttl")).unwrap();
        assert!(resources.contains("hali:Memo"), "{resources}");
        assert!(resources.contains("hali:Paper"), "{resources}");
        assert!(resources.contains("hali:Resource"), "{resources}");
    }

    #[test]
    fn edit_add_remove_type_json() {
        let env = Env::new();
        let out = env.run([
            "--json",
            "edit",
            R1,
            "--add-type",
            "Note",
            "--remove-type",
            "paper",
        ]);
        snapshot_stdout("cli_type__edit_add_remove_type_json", &out);
    }

    #[test]
    fn unknown_type_json_envelope() {
        let env = Env::new();
        let before = env.state();
        let out = env.run(["--json", "edit", R1, "--add-type", "Papr"]);
        snapshot_stderr("cli_type__unknown_type_json_envelope", 1, &out);
        assert_eq!(env.state(), before);

        // The same rule from every command that takes a Type reference, each writing nothing.
        let path = env.file("notes.txt");
        for args in [
            vec!["--json", "type", "remove", "Papr", "--yes"],
            vec!["--json", "type", "add", "Report", "--parent", "Papr"],
            vec!["--json", "find", "--type", "Papr"],
            vec!["--json", "add", &path, "--type", "Papr"],
            vec!["--json", "edit", R1, "--remove-type", "Papr"],
        ] {
            let v = failed(&env.run(args.clone()), 1, "UNKNOWN_TYPE");
            assert_eq!(v["error"]["details"]["type"], "Papr", "{args:?}");
            assert_eq!(
                v["error"]["suggested_command"], "hali --dry-run type add Papr",
                "{args:?}"
            );
        }
        assert_eq!(env.state(), before, "no Resource was cataloged");

        // With several, the first unknown one in argument order.
        let v = failed(
            &env.run([
                "--json", "find", "--type", "Paper", "--type", "Nope", "--type", "Nada",
            ]),
            1,
            "UNKNOWN_TYPE",
        );
        assert_eq!(v["error"]["details"]["type"], "Nope");
        // Text mode.
        let out = env.run(["find", "--type", "Papr"]);
        assert_eq!(out.code, Some(1));
        assert_eq!(
            out.stderr,
            "error: no Type matches Papr\n\
             hint: Use the name of an existing Type, or add the Type first. \
             Try: hali --dry-run type add Papr\n"
        );
    }

    #[test]
    fn type_in_use_json_envelope() {
        let env = Env::new();
        let before = env.state();
        let out = env.run(["--json", "type", "remove", "Paper", "--yes"]);
        snapshot_stderr("cli_type__type_in_use_json_envelope", 1, &out);
        assert_eq!(env.state(), before);

        // The other reasons, each with its own counts: typed and a parent (Document), a
        // subclass link only extensions.ttl holds (Note), a blank-node subclass (Ticket), a type
        // assertion only extensions.ttl holds (Invoice).
        for (id, typed, subtypes) in [
            ("Document", 1, 1),
            ("Note", 1, 1),
            ("Ticket", 0, 1),
            ("Invoice", 1, 0),
            ("Loop1", 0, 1),
        ] {
            let v = failed(
                &env.run(["--json", "type", "remove", id, "--yes"]),
                1,
                "IN_USE",
            );
            assert_eq!(
                v["error"]["details"],
                serde_json::json!({"type": id, "typed": typed, "subtypes": subtypes})
            );
        }
        // Text mode: one `error:` line and one `hint:` line; nothing typed lists the Types.
        let out = env.run(["type", "remove", "Ticket", "--yes"]);
        assert_eq!(out.code, Some(1));
        assert_eq!(
            out.stderr,
            "error: Type Ticket is in use: 0 typed, parent of 1 Types; nothing was written\n\
             hint: Remove the Type from the Resources (hali edit <ref> --remove-type Ticket) and \
             remove its subtypes first (hali type remove <S>), then retry. Try: hali type list\n"
        );
        assert_eq!(env.state(), before);
    }

    #[test]
    fn type_remove_json() {
        let env = Env::new();
        let out = env.run(["--json", "type", "remove", "Receipt", "--yes"]);
        snapshot_stdout("cli_type__type_remove_json", &out);
        assert!(!env.vocabulary().contains("Receipt"));
    }

    #[test]
    fn type_remove_confirmation_required_json_envelope() {
        let env = Env::new();
        let before = env.state();
        let out = env.run(["--json", "type", "remove", "Receipt"]);
        snapshot_stderr(
            "cli_type__type_remove_confirmation_required_json_envelope",
            1,
            &out,
        );
        assert_eq!(env.state(), before);
        // The suggested command names the id, whatever the reference was; `--json` may come
        // after the subcommand.
        let by_label = env.run(["type", "remove", "receipt", "--json"]);
        assert_eq!(by_label.stderr, out.stderr);
        assert_eq!(by_label.code, Some(1));
    }

    #[test]
    fn find_type_json() {
        let env = Env::new();
        let out = env.run(["--json", "find", "--type", "Document", "--sort", "title"]);
        snapshot_stdout("cli_type__find_type_json", &out);
    }

    // ---- type add and list --------------------------------------------------------------------

    #[test]
    fn type_add_text() {
        let env = Env::new();
        let out = env.run(["type", "add", "Report", "--parent", "paper"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stdout, "added type Report \"Report\"\n");
        assert_eq!(out.stderr, "");
        let out = env.run(["--json", "type", "list"]);
        let v = ok(&out);
        assert_eq!(v["total"], 11);
        let out = env.run(["--dry-run", "type", "add", "Other"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "note: dry run; nothing was written\n");
        let lines: Vec<&str> = out.stdout.lines().collect();
        assert_eq!(lines[0], "would add type Other \"Other\"");
        assert_eq!(lines.len(), 4, "{}", out.stdout);
        assert!(lines[1..].iter().all(|l| l.starts_with("+ <")), "{lines:?}");
        assert!(lines[1..].windows(2).all(|w| w[0] < w[1]), "{lines:?}");
    }

    #[test]
    fn type_add_bad_name_exits_2() {
        let env = Env::new();
        let before = env.state();
        let long = "a".repeat(65);
        for name in [
            "1x",
            "a b",
            "",
            "Resource",
            "resource",
            "a.b",
            long.as_str(),
        ] {
            let out = env.run(["type", "add", name]);
            assert_eq!(out.code, Some(2), "{name:?}: {}", out.stderr);
            assert_eq!(out.stdout, "", "{name:?}");
        }
        // `--parent` is not repeatable, and a missing name or value is a usage error too.
        for args in [
            vec![
                "type", "add", "Report", "--parent", "Paper", "--parent", "Note",
            ],
            vec!["type", "add"],
            vec!["type", "add", "Report", "--parent"],
            vec!["type", "add", "Report", "--parent", "Resource"],
            vec!["type", "add", "-x"],
        ] {
            let out = env.run(args.clone());
            assert_eq!(out.code, Some(2), "{args:?}: {}", out.stderr);
        }
        assert_eq!(env.state(), before);
        // Under `--json` a usage error that only core can see (the root class as a parent) is
        // the envelope, rule USAGE, on stderr.
        let v = failed(
            &env.run(["--json", "type", "add", "Report", "--parent", "Resource"]),
            2,
            "USAGE",
        );
        assert!(v["error"]["message"].is_string());
    }

    #[test]
    fn type_list_text() {
        let env = Env::new();
        let out = env.run(["type", "list"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        assert_eq!(
            out.stdout,
            "Document\tDocument\t\n\
             Draft\tDraft\t\n\
             Invoice\tInvoice\t\n\
             Loop1\tLoop1\tLoop2\n\
             Loop2\tLoop2\tLoop1\n\
             Memo\tMemo\tNote\n\
             Note\tNote\t\n\
             Paper\tPaper\tDocument\n\
             Receipt\tReceipt\t\n\
             Ticket\tTicket\t\n"
        );
        // Paging: `--limit` and `--offset`.
        let v = ok(&env.run(["--json", "type", "list", "--limit", "2", "--offset", "1"]));
        let ids: Vec<&str> = v["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["Draft", "Invoice"]);
        assert_eq!(v["total"], 10);
        assert_eq!(v["truncated"], true);
        assert_eq!(v["next_offset"], 3);
        // A store with no Types lists none.
        let env = Env::with_vocabulary("");
        let v = ok(&env.run(["--json", "type", "list"]));
        assert_eq!(v["results"], serde_json::json!([]));
        assert_eq!(v["total"], 0);
    }

    #[test]
    fn type_list_ignores_dry_run() {
        let env = Env::new();
        let before = env.state();
        for json_flag in [false, true] {
            let base: Vec<&str> = if json_flag {
                vec!["--json", "type", "list"]
            } else {
                vec!["type", "list"]
            };
            let plain = env.run(base.clone());
            assert_eq!(plain.code, Some(0), "{}", plain.stderr);
            assert_eq!(plain.stderr, "");
            for flag in ["--dry-run", "--yes"] {
                let mut args = base.clone();
                args.push(flag);
                let with = env.run(args);
                assert_eq!(with.code, Some(0), "{flag}: {}", with.stderr);
                assert_eq!(with.stdout, plain.stdout, "{flag}");
                assert_eq!(with.stderr, "", "{flag}: no dry-run note, no dry_run key");
            }
            assert!(!plain.stdout.contains("dry_run"));
        }
        assert_eq!(env.state(), before);
    }

    #[test]
    fn labels_are_printable() {
        // A hand-edited label with an escape sequence is shown escaped, in the text of list,
        // in the summaries and in the prompt.
        let env = Env::with_vocabulary(&format!(
            "<{HALI}esc> a <http://www.w3.org/2002/07/owl#Class> ; \
             <http://www.w3.org/2000/01/rdf-schema#subClassOf> <{HALI}Resource> ; \
             <http://www.w3.org/2000/01/rdf-schema#label> \"esc\\u001B[31mred\"@en .\n"
        ));
        let out = env.run(["type", "list"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stdout, "esc\tesc\\u{1b}[31mred\t\n");
        assert!(!out.stdout.contains('\u{1b}'));
        let out = env.tty(b"n\n", ["type", "remove", "esc"]);
        assert_eq!(out.code, Some(1));
        assert!(
            out.stderr
                .starts_with("remove type esc \"esc\\u{1b}[31mred\"? [y/N] "),
            "{:?}",
            out.stderr
        );
        assert!(!out.stderr.contains('\u{1b}'));
        let out = env.run(["type", "remove", "esc", "--yes"]);
        assert_eq!(out.stdout, "removed type esc \"esc\\u{1b}[31mred\"\n");
    }

    // ---- assignment: add --type, edit --add-type / --remove-type -------------------------------

    #[test]
    fn add_type_is_ignored_for_an_already_cataloged_path() {
        let env = Env::new();
        let path = env.file("notes.txt");
        ok(&env.run(["--json", "add", &path, "--type", "Paper"]));
        let before = env.state();
        let v = ok(&env.exec_seeded(
            "2",
            &[],
            b"",
            ["--json", "add", path.as_str(), "--type", "Note"],
        ));
        assert_eq!(v["added"], serde_json::json!([]));
        assert_eq!(v["existing"][0]["types"], serde_json::json!(["Paper"]));
        assert_eq!(env.state(), before);
    }

    #[test]
    fn edit_without_flags_exits_2_before_the_store() {
        // No change flag is a usage error, before the store is read (exit 2, not 3).
        let env = Env::without_store();
        for args in [
            vec!["edit", R1],
            vec!["--json", "edit", R1],
            vec!["edit", R1, "--add-type"],
        ] {
            let out = env.run(args.clone());
            assert_eq!(out.code, Some(2), "{args:?}: {}", out.stderr);
        }
        // Either type flag alone is a change flag.
        let env = Env::new();
        for args in [
            vec!["--json", "edit", R1, "--add-type", "Note"],
            vec!["--json", "edit", R1, "--remove-type", "Paper"],
        ] {
            let out = env.run(args.clone());
            assert_eq!(out.code, Some(0), "{args:?}: {}", out.stderr);
        }
        // One Type in both lists, however referred to, is a usage error and writes nothing.
        let before = env.state();
        let out = env.run([
            "--json",
            "edit",
            R1,
            "--add-type",
            "Paper",
            "--remove-type",
            "paper",
        ]);
        let v = failed(&out, 2, "USAGE");
        assert!(
            v["error"]["message"]
                .as_str()
                .unwrap()
                .contains("a Type cannot be both added and removed"),
            "{v}"
        );
        assert_eq!(env.state(), before);
    }

    #[test]
    fn edit_only_resources_changes() {
        let env = Env::new();
        let before = env.state();
        let out = env.run(["edit", R1, "--add-type", "Note", "--remove-type", "Paper"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(
            out.stdout,
            "edited resource 0192f0c4-0000-7000-8000-000000000501 \"r1\"\n"
        );
        let after = env.state();
        assert_ne!(after["resources.ttl"], before["resources.ttl"]);
        assert_eq!(after["vocabulary.ttl"], before["vocabulary.ttl"]);
        assert_eq!(after["extensions.ttl"], before["extensions.ttl"]);
        // hali:Resource stays, and `--remove-type` of a Type not held changes nothing.
        let resources = String::from_utf8(after["resources.ttl"].clone()).unwrap();
        assert!(resources.contains("hali:Resource"));
        let out = env.run(["edit", R1, "--remove-type", "Invoice"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(
            out.stdout,
            "unchanged resource 0192f0c4-0000-7000-8000-000000000501 \"r1\"\n"
        );
        assert_eq!(env.state(), after);
        // The root is not a Type.
        let out = env.run(["edit", R1, "--remove-type", "Resource"]);
        assert_eq!(out.code, Some(2), "{}", out.stderr);
        assert_eq!(env.state(), after);
    }

    #[test]
    fn only_vocabulary_changes() {
        let env = Env::new();
        let before = env.state();
        ok(&env.run(["--json", "type", "add", "Report", "--parent", "Memo"]));
        let after = env.state();
        assert_ne!(after["vocabulary.ttl"], before["vocabulary.ttl"]);
        assert_eq!(after["resources.ttl"], before["resources.ttl"]);
        assert_eq!(after["extensions.ttl"], before["extensions.ttl"]);
        ok(&env.run(["--json", "type", "remove", "Receipt", "--yes"]));
        let last = env.state();
        assert_ne!(last["vocabulary.ttl"], after["vocabulary.ttl"]);
        assert_eq!(last["resources.ttl"], before["resources.ttl"]);
        assert_eq!(last["extensions.ttl"], before["extensions.ttl"]);
        // Only Receipt's triples left the vocabulary.
        assert!(!env.vocabulary().contains("Receipt"));
        assert!(env.vocabulary().contains("hali:Paper"));
    }

    // ---- find --type --------------------------------------------------------------------------

    fn titles(v: &Value) -> Vec<String> {
        let mut t: Vec<String> = v["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["title"].as_str().unwrap().to_string())
            .collect();
        t.sort();
        t
    }

    #[test]
    fn find_type_filters() {
        let env = Env::new();
        let find = |args: &[&str]| {
            let mut argv = vec!["--json", "find"];
            argv.extend_from_slice(args);
            ok(&env.run(argv))
        };
        // Subclasses, a subclass link in extensions.ttl, and a cycle.
        assert_eq!(titles(&find(&["--type", "Document"])), ["r1", "r2"]);
        assert_eq!(titles(&find(&["--type", "Note"])), ["r3", "r5"]);
        assert_eq!(titles(&find(&["--type", "Loop1"])), ["r6"]);
        // By a folded id or a label; a leaf matches itself only.
        assert_eq!(titles(&find(&["--type", "paper"])), ["r1"]);
        // Repeated --type is AND, and ANDs with the other filters; --exact is for tags only.
        assert!(titles(&find(&["--type", "Document", "--type", "Note"])).is_empty());
        assert_eq!(
            titles(&find(&["--type", "Document", "--type", "Paper"])),
            ["r1"]
        );
        assert_eq!(titles(&find(&["--type", "Note", "--tag", "ml"])), ["r3"]);
        assert_eq!(
            titles(&find(&["--type", "Document", "--title", "r2"])),
            ["r2"]
        );
        assert_eq!(
            titles(&find(&["--type", "Document", "--tag", "ml", "--exact"])),
            ["r1"]
        );
        // A Type nothing is typed with: an empty result, exit 0.
        let v = find(&["--type", "Receipt"]);
        assert_eq!(v["results"], serde_json::json!([]));
        assert_eq!(v["total"], 0);
        // The root is not a Type; tags are resolved before Types.
        let out = env.run(["find", "--type", "Resource"]);
        assert_eq!(out.code, Some(2), "{}", out.stderr);
        let v = failed(
            &env.run(["--json", "find", "--type", "Nope", "--tag", "nope-tag"]),
            1,
            "UNKNOWN_CONCEPT",
        );
        assert_eq!(v["error"]["details"]["concept"], "nope-tag");
    }

    // ---- type remove: confirmation (spec §9.8) --------------------------------------------------

    #[test]
    fn remove_in_use_before_confirmation() {
        // IN_USE comes before the confirmation: no prompt, with or without a terminal, and the
        // vocabulary is as it was.
        let env = Env::new();
        let before = env.state();
        for out in [
            env.run(["--json", "type", "remove", "Paper"]),
            env.tty(b"y\n", ["--json", "type", "remove", "Paper"]),
            env.tty(b"y\n", ["--json", "type", "remove", "Document"]),
        ] {
            assert!(!out.stderr.contains("[y/N]"), "{}", out.stderr);
            failed(&out, 1, "IN_USE");
        }
        assert_eq!(env.state(), before);
    }

    #[test]
    fn remove_without_a_terminal_and_with_yes() {
        let env = Env::new();
        let before = env.state();
        for (extra, args) in [
            (&[][..], vec!["--json", "type", "remove", "Receipt"]),
            (
                &[("HALI_TEST_TTY", "0")][..],
                vec!["--json", "type", "remove", "Receipt"],
            ),
            (
                &[("HALI_TEST_TTY", "0")][..],
                vec!["type", "remove", "receipt", "--json"],
            ),
        ] {
            let out = env.exec(extra, b"y\n", args);
            let v = failed(&out, 1, "CONFIRMATION_REQUIRED");
            assert_eq!(v["error"]["message"], NOT_A_TTY);
            assert_eq!(
                v["error"]["details"],
                serde_json::json!({"command": "type remove", "declined": false})
            );
            assert_eq!(v["error"]["suggested_fix"], FIX);
            assert_eq!(v["error"]["suggested_command"], retry("Receipt"));
            assert_eq!(env.state(), before);
        }
        // With `--yes`, anywhere, it goes ahead, still with no terminal.
        for args in [
            vec!["type", "remove", "Receipt", "--yes"],
            vec!["--yes", "type", "remove", "Receipt"],
            vec!["type", "--yes", "remove", "Receipt"],
        ] {
            let env = Env::new();
            let out = env.run(args.clone());
            assert_eq!(out.code, Some(0), "{args:?}: {}", out.stderr);
            assert_eq!(out.stdout, "removed type Receipt \"Receipt\"\n");
            assert!(!env.vocabulary().contains("Receipt"));
        }
    }

    #[test]
    fn remove_retry_command_is_shell_safe() {
        // Ids from a hand-edited store: one that starts with `-` and one that needs quoting.
        let class = "<http://www.w3.org/2002/07/owl#Class>";
        let sub = "<http://www.w3.org/2000/01/rdf-schema#subClassOf>";
        let lab = "<http://www.w3.org/2000/01/rdf-schema#label>";
        let env = Env::with_vocabulary(&format!(
            "<{HALI}-x> a {class} ; {sub} <{HALI}Resource> ; {lab} \"dash x\"@en .\n\
             <{HALI}a;b> a {class} ; {sub} <{HALI}Resource> ; {lab} \"semi\"@en .\n"
        ));
        let v = failed(
            &env.run(["--json", "type", "remove", "dash x"]),
            1,
            "CONFIRMATION_REQUIRED",
        );
        assert_eq!(
            v["error"]["suggested_command"],
            "hali --yes type remove -- -x"
        );
        let v = failed(
            &env.run(["--json", "type", "remove", "semi"]),
            1,
            "CONFIRMATION_REQUIRED",
        );
        assert_eq!(
            v["error"]["suggested_command"],
            "hali type remove 'a;b' --yes"
        );
        // The reference may itself start with `-`, after `--`.
        let out = env.run(["type", "remove", "--yes", "--", "-x"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stdout, "removed type -x \"dash x\"\n");
    }

    #[test]
    fn remove_tty_prompt_yes_no_eof() {
        let ask = prompt("Receipt", "Receipt");

        // y and yes, trimmed, in any case, confirm. The prompt is the only thing on stderr and
        // the summary is the only thing on stdout (spec §9.8).
        for answer in ["y\n", "yes\n", "Y\n", "YES\n", "  y  \n", "\tyes\r\n"] {
            let env = Env::new();
            let out = env.tty(answer.as_bytes(), ["type", "remove", "Receipt"]);
            assert_eq!(out.code, Some(0), "{answer:?}: {}", out.stderr);
            assert_eq!(out.stderr, ask, "{answer:?}");
            assert_eq!(
                out.stdout, "removed type Receipt \"Receipt\"\n",
                "{answer:?}"
            );
            assert!(!env.vocabulary().contains("Receipt"), "{answer:?}");
        }

        // Anything else, end of input, or an unreadable line declines, with the same rule.
        let declines: [&[u8]; 7] = [
            b"n\n",
            b"no\n",
            b"\n",
            b"",
            b"maybe\n",
            b"y e s\n",
            b"\xff\xfe\n",
        ];
        for answer in declines {
            let env = Env::new();
            let before = env.state();
            let out = env.tty(answer, ["type", "remove", "Receipt"]);
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
            let out = env.tty(answer, ["--json", "type", "remove", "Receipt"]);
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
                serde_json::json!({"command": "type remove", "declined": true})
            );
            assert_eq!(v["error"]["suggested_command"], retry("Receipt"));
            assert_eq!(out.code, Some(1));
            assert_eq!(env.state(), before);
        }

        // `--json` and a yes: stdout is the clean payload, the prompt is on stderr.
        let env = Env::new();
        let out = env.tty(b"y\n", ["--json", "type", "remove", "Receipt"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, ask);
        assert_eq!(json(&out.stdout)["type"]["id"], "Receipt");

        // `--yes` skips the prompt; the answer on stdin is `n` and is never read.
        let env = Env::new();
        let out = env.tty(b"n\n", ["type", "remove", "Receipt", "--yes"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        assert!(!env.vocabulary().contains("Receipt"));
    }

    #[test]
    fn remove_dry_run_never_asks() {
        let env = Env::new();
        let before = env.state();
        for (extra, stdin) in [
            (&[][..], &b"n\n"[..]),
            (&[("HALI_TEST_TTY", "0")][..], &b""[..]),
            (&[("HALI_TEST_TTY", "1")][..], &b"n\n"[..]),
            (&[("HALI_TEST_TTY", "1")][..], &b""[..]),
        ] {
            // Text: the note on stderr and nothing else; no prompt, no CONFIRMATION_REQUIRED.
            let out = env.exec(extra, stdin, ["--dry-run", "type", "remove", "Receipt"]);
            assert_eq!(out.code, Some(0), "{extra:?}: {}", out.stderr);
            assert_eq!(
                out.stderr, "note: dry run; nothing was written\n",
                "{extra:?}"
            );
            let lines: Vec<&str> = out.stdout.lines().collect();
            assert_eq!(lines[0], "would remove type Receipt \"Receipt\"");
            assert_eq!(lines.len(), 1 + 4, "{}", out.stdout);
            assert!(lines[1..].iter().all(|l| l.starts_with("- <")), "{lines:?}");
            // JSON: nothing on stderr.
            let out = env.exec(
                extra,
                stdin,
                ["type", "remove", "Receipt", "--json", "--dry-run"],
            );
            assert_eq!(out.code, Some(0), "{extra:?}: {}", out.stderr);
            assert_eq!(out.stderr, "", "{extra:?}");
            assert_eq!(json(&out.stdout)["dry_run"], true);
            assert_eq!(env.state(), before, "{extra:?}");
        }
        // A dry run reports the errors of a real run, with identical stderr.
        let dry = env.run(["--json", "--dry-run", "type", "remove", "Paper"]);
        let real = env.run(["--json", "type", "remove", "Paper", "--yes"]);
        assert_eq!(dry.code, Some(1));
        assert_eq!(dry.stderr, real.stderr);
        assert!(!dry.stderr.contains("dry_run"));
    }

    #[test]
    fn remove_extensions_only_type_is_unchanged() {
        // Draft exists only in extensions.ttl and nothing uses it: nothing to remove, nothing
        // to ask, with or without a terminal.
        let env = Env::new();
        let before = env.state();
        let out = env.tty(b"n\n", ["type", "remove", "Draft"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stdout, "unchanged type Draft \"Draft\"\n");
        assert_eq!(out.stderr, "", "no prompt");
        let out = env.run(["--json", "type", "remove", "Draft"]);
        let v = ok(&out);
        assert_eq!(v["type"]["id"], "Draft");
        assert_eq!(env.state(), before);
    }

    #[test]
    fn remove_never_waits_on_stdin() {
        // stdin is a pipe that stays open and is never written: a command that read it would
        // hang. Each run must exit on its own, with the code of its own path, well inside 10 s.
        let runs: [(&[&str], i32); 7] = [
            (&["type", "remove", "Receipt"], 1),
            (&["type", "remove", "Receipt", "--yes"], 0),
            (&["--dry-run", "type", "remove", "Receipt"], 0),
            (&["type", "remove", "Paper"], 1),
            (&["type", "remove", "Draft"], 0),
            (&["type", "add", "Report"], 0),
            (&["type", "list"], 0),
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

    #[test]
    fn type_add_never_asks() {
        // Not destructive: no prompt on a terminal, no CONFIRMATION_REQUIRED without one.
        for tty in ["0", "1"] {
            let env = Env::new();
            let out = env.exec(&[("HALI_TEST_TTY", tty)], b"n\n", ["type", "add", "Report"]);
            assert_eq!(out.code, Some(0), "tty {tty}: {}", out.stderr);
            assert_eq!(out.stderr, "", "tty {tty}");
            assert_eq!(out.stdout, "added type Report \"Report\"\n");
        }
        // Without a terminal and without --yes, the same.
        let env = Env::new();
        let out = env.run(["type", "add", "Report"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
    }

    #[test]
    fn remove_text_summaries() {
        let env = Env::new();
        let out = env.run(["type", "remove", "Receipt", "--yes"]);
        assert_eq!(out.stdout, "removed type Receipt \"Receipt\"\n");
        assert_eq!(out.stderr, "");
        // The Type is gone: a second removal is UNKNOWN_TYPE.
        failed(
            &env.run(["--json", "type", "remove", "Receipt", "--yes"]),
            1,
            "UNKNOWN_TYPE",
        );
    }

    // ---- schemas ------------------------------------------------------------------------------

    #[test]
    fn schema_lists_type_names() {
        let env = Env::new();
        let out = env.run(["schema"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        for name in ["type add", "type list", "type remove"] {
            assert!(
                out.stdout.lines().any(|l| l == name),
                "`hali schema` should list `{name}`: {}",
                out.stdout
            );
        }
        let v = ok(&env.run(["schema", "--json"]));
        assert_eq!(v["schemas"].as_array().unwrap().len(), 22, "{v}");
        for name in ["type add", "type list", "type remove"] {
            let words: Vec<&str> = name.split(' ').collect();
            let mut args = vec!["schema"];
            args.extend(words);
            let out = env.run(args);
            assert_eq!(out.code, Some(0), "{name}: {}", out.stderr);
            let schema: Value = serde_json::from_str(&out.stdout).unwrap();
            assert_eq!(
                schema["$schema"], "https://json-schema.org/draft/2020-12/schema",
                "{name}"
            );
        }
    }
}
