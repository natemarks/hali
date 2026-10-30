//! Goal tests for relations between Resources through the real binary (#18, spec §2.4, §9.1,
//! §9.2, §9.3, §9.4, §9.5, §9.7, §9.8, §14.1, §14.2): `relate`, `unrelate`,
//! `relation add|list|remove`, `show` relations, `find --related-to/--via` and
//! `forget --force`. Locked golden snapshots sit in tests/snapshots/ (hand-written from the
//! spec and the issue's contract): `cli_relation__{relate,unrelate,relation_add,relation_list,
//! relation_remove,find_related_to,find_related_via,show_relations}_json` (stdout), the stderr
//! envelopes `cli_relation__{relation_label_taken,relation_in_use,
//! relation_remove_confirmation_required,forget_blocked,unknown_relation}_json_envelope`, and
//! the dry-run goldens `cli_dry_run__{relate,unrelate,relation_add,relation_remove}_dry_run_json`
//! (written from this file, in the `dry_run` module).
//!
//! Every run sets HALI_DATA_DIR / XDG_CONFIG_HOME / HOME to temp dirs and clears the rest of
//! the environment, so the real store and config are never read. The store is
//! tests/fixtures/relations (see tests/core_relation.rs), with a `# hand-edited` comment line put
//! in front of resources.ttl so that a rewrite of it shows up as a byte difference. The
//! Resources' files do not exist (their paths are `/fixtures/relations/rN.txt`), so every record
//! is `missing`. No command here mints an id, so every run uses `HALI_TEST_ID_SEED=1`.
//!
//! The terminal seam is `HALI_TEST_TTY=1|0` (test-hooks builds only, spec §14.2). The prompt of
//! `relation remove` is `remove relation <id> "<label>"? [y/N] ` on stderr (spec §9.8).
//!
//! Wording the contract leaves open and the goldens and tests fix (change them, with the human,
//! if the wording should differ):
//! - the `LABEL_TAKEN` suggested_fix for a relation kind is `Use the existing relation kind, or
//!   choose a different name.`;
//! - the labels of the standard relations in core.ttl are `relation`, `is part of`,
//!   `references` and `was derived from`, all `@en`;
//! - the text of `relation list` is one `<id><TAB><label><TAB><parents joined with ", ">` line
//!   per kind; an empty parent list leaves nothing after the last tab;
//! - the `CONFIRMATION_REQUIRED` message names the command as `relation remove`; a declined
//!   confirmation says `relation remove was not confirmed; nothing was written`;
//! - the confirmation retry of `forget` carries `--force` exactly when `--force` was given:
//!   `hali forget <id> --force --yes`;
//! - the `IN_USE` suggested_fix names the kind id POSIX-quoted (`'skos:related'`), as the Concept
//!   and Type ones do; the `UNKNOWN_RELATION` suggested_command writes `dcterms:relation`
//!   unquoted, as the contract has it;
//! - text summaries: `related|would relate|unchanged <A> <relation id> <B>` and
//!   `unrelated|would unrelate|unchanged …`, ids in full.

#[cfg(feature = "goal")]
mod support;

#[cfg(feature = "goal")]
mod goal {
    use assert_cmd::Command;
    use serde_json::Value;
    use std::collections::BTreeMap;
    use std::ffi::OsStr;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};
    use tempfile::TempDir;

    const NOW: &str = "2026-10-24T09:30:00Z";
    const R1: &str = "0192f0c4-0000-7000-8000-000000000701";
    const R2: &str = "0192f0c4-0000-7000-8000-000000000702";
    const R3: &str = "0192f0c4-0000-7000-8000-000000000703";
    const R4: &str = "0192f0c4-0000-7000-8000-000000000704";
    const R5: &str = "0192f0c4-0000-7000-8000-000000000705";
    const GONE: &str = "0192f0c4-0000-7000-8000-000000000799";
    const GONE2: &str = "0192f0c4-0000-7000-8000-000000000798";
    const FIX: &str =
        "Run it again with --yes to confirm, or with --dry-run to see what it would change.";
    const NOT_A_TTY: &str = "relation remove needs confirmation, but stdin or stderr is not a \
                             terminal; nothing was written";
    const DECLINED: &str = "relation remove was not confirmed; nothing was written";
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
        /// The `relations` fixture in the data dir.
        fn new() -> Env {
            let tmp = tempfile::tempdir().unwrap();
            let root = std::fs::canonicalize(tmp.path()).unwrap();
            for d in ["cfg", "home", "data"] {
                std::fs::create_dir_all(root.join(d)).unwrap();
            }
            let env = Env { _root: tmp, root };
            let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/relations");
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

        /// Appends Turtle text to a file of the data dir (the file's prefixes apply).
        fn append(&self, file: &str, text: &str) {
            let path = self.data().join(file);
            let mut old = std::fs::read_to_string(&path).unwrap();
            old.push_str(text);
            std::fs::write(path, old).unwrap();
        }

        fn data(&self) -> PathBuf {
            self.root.join("data")
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

        /// Runs with extra environment and stdin bytes.
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
            Out {
                code: out.status.code(),
                stdout: String::from_utf8(out.stdout).unwrap(),
                stderr: String::from_utf8(out.stderr).unwrap(),
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

        fn file_bytes(&self, name: &str) -> Vec<u8> {
            std::fs::read(self.data().join(name)).unwrap()
        }

        fn vocabulary(&self) -> String {
            String::from_utf8(self.file_bytes("vocabulary.ttl")).unwrap()
        }

        fn resources(&self) -> String {
            String::from_utf8(self.file_bytes("resources.ttl")).unwrap()
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
        format!("remove relation {id} \"{label}\"? [y/N] ")
    }

    fn retry(id: &str) -> String {
        format!("hali relation remove {id} --yes")
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

    // ---- golden snapshots (§14.1) -------------------------------------------------------------

    #[test]
    fn relate_json() {
        let env = Env::new();
        let out = env.run(["--json", "relate", R5, "annotates", R2]);
        snapshot_stdout("cli_relation__relate_json", &out);
        let resources = env.resources();
        assert!(resources.contains("hali:annotates"), "{resources}");
    }

    #[test]
    fn unrelate_json() {
        let env = Env::new();
        let out = env.run(["--json", "unrelate", R3, "annotates", R1]);
        snapshot_stdout("cli_relation__unrelate_json", &out);
        // Only that triple went: r3 is still `dcterms:relation` r1.
        let resources = env.resources();
        assert!(!resources.contains("hali:annotates"), "{resources}");
        assert!(resources.contains("dcterms:relation"), "{resources}");
    }

    #[test]
    fn relation_add_json() {
        let env = Env::new();
        let out = env.run([
            "--json",
            "relation",
            "add",
            "cites",
            "--parent",
            "dcterms:references",
        ]);
        snapshot_stdout("cli_relation__relation_add_json", &out);
        let vocabulary = env.vocabulary();
        assert!(
            vocabulary.contains("hali:cites a owl:ObjectProperty"),
            "{vocabulary}"
        );
    }

    #[test]
    fn relation_label_taken_json_envelope() {
        let env = Env::new();
        let before = env.state();
        let out = env.run([
            "--json",
            "relation",
            "add",
            "references",
            "--parent",
            "dcterms:relation",
        ]);
        snapshot_stderr("cli_relation__relation_label_taken_json_envelope", 1, &out);
        assert_eq!(env.state(), before);
        // Text mode: one `error:` line and one `hint:` line.
        let out = env.run([
            "relation",
            "add",
            "references",
            "--parent",
            "dcterms:relation",
        ]);
        assert_eq!(out.code, Some(1));
        assert_eq!(
            out.stderr,
            "error: label \"references\"@en is already used by relation kind dcterms:references\n\
             hint: Use the existing relation kind, or choose a different name. Try: hali relation list\n"
        );
        // A folded id of a kind that only extensions.ttl holds, and a name already used in the
        // store.
        failed(
            &env.run([
                "--json",
                "relation",
                "add",
                "quotes",
                "--parent",
                "dcterms:relation",
            ]),
            1,
            "LABEL_TAKEN",
        );
        let v = failed(
            &env.run([
                "--json",
                "relation",
                "add",
                "ml",
                "--parent",
                "dcterms:relation",
            ]),
            2,
            "USAGE",
        );
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
    fn relation_list_json() {
        let env = Env::new();
        let before = env.state();
        let out = env.run(["--json", "relation", "list"]);
        snapshot_stdout("cli_relation__relation_list_json", &out);
        assert_eq!(env.state(), before);
    }

    #[test]
    fn relation_in_use_json_envelope() {
        let env = Env::new();
        let before = env.state();
        let out = env.run(["--json", "relation", "remove", "annotates", "--yes"]);
        snapshot_stderr("cli_relation__relation_in_use_json_envelope", 1, &out);
        assert_eq!(env.state(), before);
        // Used, parent of others, used only in extensions.ttl, a blank-node sub-property.
        for (id, used, subproperties) in [
            ("annotates", 1, 1),
            ("summarises", 1, 0),
            ("quotes", 1, 0),
            ("digestOf", 0, 1),
        ] {
            let v = failed(
                &env.run(["--json", "relation", "remove", id, "--yes"]),
                1,
                "IN_USE",
            );
            assert_eq!(
                v["error"]["details"],
                serde_json::json!({"relation": id, "used": used, "subproperties": subproperties})
            );
        }
        // Text mode: one `error:` line and one `hint:` line.
        let out = env.run(["relation", "remove", "digestOf", "--yes"]);
        assert_eq!(out.code, Some(1));
        assert_eq!(
            out.stderr,
            "error: relation kind digestOf is in use: 0 relations, parent of 1 relation kinds; nothing was written\n\
             hint: Remove the relations (hali unrelate <ref> digestOf <ref>) and remove its sub-relations first (hali relation remove <S>), then retry. Try: hali relation list\n"
        );
        assert_eq!(env.state(), before);
    }

    #[test]
    fn relation_remove_json() {
        let env = Env::new();
        let out = env.run(["--json", "relation", "remove", "excerptOf", "--yes"]);
        snapshot_stdout("cli_relation__relation_remove_json", &out);
        assert!(!env.vocabulary().contains("excerptOf"));
    }

    #[test]
    fn relation_remove_confirmation_required_json_envelope() {
        let env = Env::new();
        let before = env.state();
        let out = env.run(["--json", "relation", "remove", "excerptOf"]);
        snapshot_stderr(
            "cli_relation__relation_remove_confirmation_required_json_envelope",
            1,
            &out,
        );
        assert_eq!(env.state(), before);
        // The suggested command names the id, whatever the reference was; `--json` may come
        // after the subcommand.
        let by_case = env.run(["relation", "remove", "EXCERPTOF", "--json"]);
        assert_eq!(by_case.stderr, out.stderr);
        assert_eq!(by_case.code, Some(1));
    }

    #[test]
    fn find_related_to_json() {
        let env = Env::new();
        let out = env.run(["--json", "find", "--related-to", R1, "--sort", "title"]);
        snapshot_stdout("cli_relation__find_related_to_json", &out);
    }

    #[test]
    fn find_related_via_json() {
        let env = Env::new();
        let out = env.run([
            "--json",
            "find",
            "--related-to",
            R1,
            "--via",
            "dcterms:relation",
            "--sort",
            "title",
        ]);
        snapshot_stdout("cli_relation__find_related_via_json", &out);
    }

    #[test]
    fn forget_blocked_json_envelope() {
        let env = Env::new();
        let before = env.state();
        let out = env.run(["--json", "forget", R1]);
        snapshot_stderr("cli_relation__forget_blocked_json_envelope", 1, &out);
        assert_eq!(env.state(), before);
        // Text mode: one `error:` line and one `hint:` line.
        let out = env.run(["forget", R1]);
        assert_eq!(out.code, Some(1));
        assert_eq!(
            out.stderr,
            format!(
                "error: Resource {R1} has 3 inbound relations from 2 other Resources; nothing was written\n\
                 hint: Remove them (hali unrelate <ref> <relation> {R1}), or forget with --force to remove them too, then retry. Try: hali show {R1}\n"
            )
        );
        assert_eq!(env.state(), before);
    }

    #[test]
    fn unknown_relation_json_envelope() {
        let env = Env::new();
        let before = env.state();
        let out = env.run(["--json", "relate", R1, "annotate", R2]);
        snapshot_stderr("cli_relation__unknown_relation_json_envelope", 1, &out);
        assert_eq!(env.state(), before);

        // The same rule from every command that takes a relation reference, each writing nothing.
        for args in [
            vec!["--json", "unrelate", R1, "annotate", R2],
            vec!["--json", "find", "--related-to", R1, "--via", "annotate"],
            vec!["--json", "relation", "remove", "annotate", "--yes"],
            vec!["--json", "relation", "add", "cites", "--parent", "annotate"],
        ] {
            let v = failed(&env.run(args.clone()), 1, "UNKNOWN_RELATION");
            assert_eq!(v["error"]["details"]["relation"], "annotate", "{args:?}");
            assert_eq!(
                v["error"]["suggested_command"],
                "hali --dry-run relation add annotate --parent dcterms:relation",
                "{args:?}"
            );
        }
        assert_eq!(env.state(), before);

        // A property that is no relation kind; a reference that is no valid name.
        let v = failed(
            &env.run(["--json", "relate", R1, "dcterms:title", R2]),
            1,
            "UNKNOWN_RELATION",
        );
        assert_eq!(v["error"]["suggested_command"], "hali relation list");
        // A bare name never names a standard kind; it is suggested.
        let v = failed(
            &env.run(["--json", "relate", R1, "isPartOf", R2]),
            1,
            "UNKNOWN_RELATION",
        );
        assert_eq!(
            v["error"]["details"]["suggestions"][0],
            serde_json::json!({"id": "dcterms:isPartOf", "label": "is part of"})
        );
        // Text mode.
        let out = env.run(["relate", R1, "annotate", R2]);
        assert_eq!(out.code, Some(1));
        assert_eq!(
            out.stderr,
            "error: no relation kind matches annotate\n\
             hint: Use an existing relation kind (hali relation list), or add one first. \
             Try: hali --dry-run relation add annotate --parent dcterms:relation\n"
        );
        assert_eq!(env.state(), before);
    }

    #[test]
    fn show_relations_json() {
        let env = Env::new();
        let before = env.state();
        let out = env.run(["--json", "show", R1]);
        snapshot_stdout("cli_relation__show_relations_json", &out);
        assert_eq!(env.state(), before);
    }

    // ---- relate and unrelate ------------------------------------------------------------------

    #[test]
    fn relate_text() {
        let env = Env::new();
        let out = env.run(["relate", R5, "annotates", R2]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stdout, format!("related {R5} annotates {R2}\n"));
        assert_eq!(out.stderr, "");
        // Again: nothing changes, and the text says so. The relation is reported by its id.
        let after = env.state();
        let out = env.run(["relate", R5, "ANNOTATES", R2]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stdout, format!("unchanged {R5} annotates {R2}\n"));
        assert_eq!(env.state(), after);
        // A dry run: the summary, the three change lines (sorted), the note on stderr.
        let fresh = Env::new();
        let out = fresh.run(["--dry-run", "relate", R5, "dcterms:references", R1]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "note: dry run; nothing was written\n");
        let lines: Vec<&str> = out.stdout.lines().collect();
        assert_eq!(
            lines[0],
            format!("would relate {R5} dcterms:references {R1}")
        );
        assert_eq!(lines.len(), 4, "{}", out.stdout);
        assert!(
            lines[1..]
                .iter()
                .all(|l| l.starts_with("+ <") || l.starts_with("- <"))
        );
        assert_eq!(env.state(), after);
        // By path, the way a Resource is referred to elsewhere.
        let out = env.run([
            "relate",
            "/fixtures/relations/r4.txt",
            "dcterms:isPartOf",
            "/fixtures/relations/r5.txt",
        ]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stdout, format!("related {R4} dcterms:isPartOf {R5}\n"));
    }

    #[test]
    fn unrelate_text() {
        let env = Env::new();
        let out = env.run(["unrelate", R3, "annotates", R1]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stdout, format!("unrelated {R3} annotates {R1}\n"));
        assert_eq!(out.stderr, "");
        let after = env.state();
        let out = env.run(["unrelate", R3, "annotates", R1]);
        assert_eq!(out.stdout, format!("unchanged {R3} annotates {R1}\n"));
        assert_eq!(env.state(), after);
        // The sub-property and the reverse are not removed: nothing changes.
        let out = env.run(["unrelate", R3, "dcterms:references", R1]);
        assert_eq!(
            out.stdout,
            format!("unchanged {R3} dcterms:references {R1}\n")
        );
        let out = env.run(["--dry-run", "unrelate", R3, "dcterms:relation", R1]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        let lines: Vec<&str> = out.stdout.lines().collect();
        assert_eq!(
            lines[0],
            format!("would unrelate {R3} dcterms:relation {R1}")
        );
        assert_eq!(env.state(), after);
    }

    #[test]
    fn relate_never_asks() {
        // Not destructive: no prompt on a terminal, no CONFIRMATION_REQUIRED without one.
        for tty in ["0", "1"] {
            for args in [
                vec!["relate", R5, "annotates", R2],
                vec!["unrelate", R3, "annotates", R1],
                vec!["relation", "add", "cites", "--parent", "dcterms:relation"],
            ] {
                let env = Env::new();
                let out = env.exec(&[("HALI_TEST_TTY", tty)], b"n\n", args.clone());
                assert_eq!(out.code, Some(0), "tty {tty} {args:?}: {}", out.stderr);
                assert_eq!(out.stderr, "", "tty {tty} {args:?}");
            }
        }
        // Without a terminal and without --yes, the same.
        let env = Env::new();
        let out = env.run(["relate", R5, "annotates", R2]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
    }

    #[test]
    fn only_resources_changes() {
        let env = Env::new();
        let before = env.state();
        ok(&env.run(["--json", "relate", R5, "annotates", R2]));
        let after = env.state();
        assert_ne!(after["resources.ttl"], before["resources.ttl"]);
        assert_eq!(after["vocabulary.ttl"], before["vocabulary.ttl"]);
        assert_eq!(after["extensions.ttl"], before["extensions.ttl"]);
        ok(&env.run(["--json", "unrelate", R5, "annotates", R2]));
        let last = env.state();
        assert_ne!(last["resources.ttl"], after["resources.ttl"]);
        assert_eq!(last["vocabulary.ttl"], before["vocabulary.ttl"]);
        assert_eq!(last["extensions.ttl"], before["extensions.ttl"]);
        // The no-ops rewrite nothing at all: the hand-edit comment stays.
        let env = Env::new();
        let before = env.state();
        ok(&env.run(["--json", "relate", R1, "dcterms:isPartOf", R2]));
        ok(&env.run(["--json", "unrelate", R2, "dcterms:isPartOf", R1]));
        ok(&env.run(["--json", "unrelate", R5, "quotes", R3]));
        assert_eq!(env.state(), before);
    }

    #[test]
    fn relate_checks_run_in_order() {
        let env = Env::new();
        let before = env.state();
        // A first, then the relation, then B.
        let v = failed(
            &env.run(["--json", "relate", GONE, "nope", GONE2]),
            1,
            "UNKNOWN_RESOURCE",
        );
        assert_eq!(v["error"]["details"]["ref"], GONE);
        failed(
            &env.run(["--json", "relate", R1, "nope", GONE2]),
            1,
            "UNKNOWN_RELATION",
        );
        let v = failed(
            &env.run(["--json", "relate", R1, "annotates", GONE2]),
            1,
            "UNKNOWN_RESOURCE",
        );
        assert_eq!(v["error"]["details"]["ref"], GONE2);
        // The same for unrelate, except that B may be a UUID that is not cataloged (no relation
        // is held to it, so nothing changes).
        failed(
            &env.run(["--json", "unrelate", GONE, "nope", GONE2]),
            1,
            "UNKNOWN_RESOURCE",
        );
        failed(
            &env.run(["--json", "unrelate", R1, "nope", GONE2]),
            1,
            "UNKNOWN_RELATION",
        );
        let v = ok(&env.run(["--json", "unrelate", R1, "annotates", GONE2]));
        assert_eq!(v["relation"]["to"], GONE2);
        // A Resource cannot be related to itself: a usage error, exit 2.
        let v = failed(
            &env.run(["--json", "relate", R1, "annotates", R1]),
            2,
            "USAGE",
        );
        assert!(
            v["error"]["message"]
                .as_str()
                .unwrap()
                .contains("a Resource cannot be related to itself"),
            "{v}"
        );
        assert_eq!(env.state(), before);
        // A missing argument is a clap usage error.
        for args in [
            vec!["relate", R1, "annotates"],
            vec!["relate", R1],
            vec!["unrelate", R1, "annotates"],
            vec!["relate"],
        ] {
            let out = env.run(args.clone());
            assert_eq!(out.code, Some(2), "{args:?}: {}", out.stderr);
            assert_eq!(out.stdout, "");
        }
    }

    #[test]
    fn unrelate_dangling_full_uuid() {
        // A hand-edited relation to a Resource that is no longer cataloged breaks the target
        // shape: every mutation is refused until `unrelate` removes it, by the full UUID.
        let env = Env::new();
        env.append(
            "resources.ttl",
            &format!("<urn:uuid:{R1}> dcterms:relation <urn:uuid:{GONE}> .\n"),
        );
        let before = env.state();
        failed(
            &env.run(["--json", "relate", R2, "annotates", R3]),
            1,
            "SHACL_VIOLATION",
        );
        assert_eq!(env.state(), before);
        let out = env.run(["unrelate", R1, "dcterms:relation", GONE]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(
            out.stdout,
            format!("unrelated {R1} dcterms:relation {GONE}\n")
        );
        let out = env.run(["validate"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        // ...but `relate` needs both Resources cataloged.
        failed(
            &env.run(["--json", "relate", R1, "dcterms:relation", GONE]),
            1,
            "UNKNOWN_RESOURCE",
        );
    }

    // ---- relation add, list, remove -----------------------------------------------------------

    #[test]
    fn relation_add_text() {
        let env = Env::new();
        let out = env.run(["relation", "add", "cites", "--parent", "dcterms:references"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stdout, "added relation cites \"cites\"\n");
        assert_eq!(out.stderr, "");
        // It is a kind now: it can be related with and listed.
        ok(&env.run(["--json", "relate", R5, "cites", R2]));
        let v = ok(&env.run(["--json", "relation", "list"]));
        assert_eq!(v["total"], 11);
        let out = env.run([
            "--dry-run",
            "relation",
            "add",
            "other",
            "--parent",
            "prov:wasDerivedFrom",
        ]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "note: dry run; nothing was written\n");
        let lines: Vec<&str> = out.stdout.lines().collect();
        assert_eq!(lines[0], "would add relation other \"other\"");
        assert_eq!(lines.len(), 4, "{}", out.stdout);
        assert!(lines[1..].iter().all(|l| l.starts_with("+ <")), "{lines:?}");
        assert!(lines[1..].windows(2).all(|w| w[0] < w[1]), "{lines:?}");
    }

    #[test]
    fn relation_add_bad_name_exits_2() {
        let env = Env::new();
        let before = env.state();
        let long = "a".repeat(65);
        for name in [
            "Annotates",
            "1x",
            "a b",
            "",
            "-x",
            "a.b",
            "a:b",
            "résumé",
            long.as_str(),
        ] {
            let out = env.run(["relation", "add", name, "--parent", "dcterms:relation"]);
            assert_eq!(out.code, Some(2), "{name:?}: {}", out.stderr);
            assert_eq!(out.stdout, "", "{name:?}");
        }
        // `--parent` is required and is not repeatable; a missing name or value is a usage
        // error too.
        for args in [
            vec!["relation", "add", "cites"],
            vec![
                "relation",
                "add",
                "cites",
                "--parent",
                "dcterms:relation",
                "--parent",
                "dcterms:isPartOf",
            ],
            vec!["relation", "add", "--parent", "dcterms:relation"],
            vec!["relation", "add", "cites", "--parent"],
        ] {
            let out = env.run(args.clone());
            assert_eq!(out.code, Some(2), "{args:?}: {}", out.stderr);
        }
        assert_eq!(env.state(), before);
        // Under `--json` a usage error that only core can see (a custom parent) is the
        // envelope, rule USAGE, on stderr.
        let v = failed(
            &env.run([
                "--json",
                "relation",
                "add",
                "cites",
                "--parent",
                "annotates",
            ]),
            2,
            "USAGE",
        );
        assert!(
            v["error"]["message"]
                .as_str()
                .unwrap()
                .contains("--parent must be a standard relation"),
            "{v}"
        );
        assert_eq!(env.state(), before);
    }

    #[test]
    fn relation_add_checks_run_in_order() {
        let env = Env::new();
        let before = env.state();
        // The parent is resolved first, then it must be standard, then the name must be free.
        failed(
            &env.run(["--json", "relation", "add", "quotes", "--parent", "nope"]),
            1,
            "UNKNOWN_RELATION",
        );
        failed(
            &env.run([
                "--json",
                "relation",
                "add",
                "quotes",
                "--parent",
                "annotates",
            ]),
            2,
            "USAGE",
        );
        failed(
            &env.run([
                "--json",
                "relation",
                "add",
                "quotes",
                "--parent",
                "dcterms:relation",
            ]),
            1,
            "LABEL_TAKEN",
        );
        assert_eq!(env.state(), before);
    }

    #[test]
    fn relation_list_text() {
        let env = Env::new();
        let out = env.run(["relation", "list"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        assert_eq!(
            out.stdout,
            "annotates\tannotates\tdcterms:references\n\
             digestOf\tdigestOf\tdcterms:relation\n\
             excerptOf\texcerpt of\tprov:wasDerivedFrom\n\
             dcterms:isPartOf\tis part of\tdcterms:relation\n\
             quotes\tquotes\tdcterms:references\n\
             dcterms:references\treferences\tdcterms:relation\n\
             dcterms:relation\trelation\t\n\
             sketch\tsketch\tdcterms:isPartOf\n\
             summarises\tsummarises\tannotates\n\
             prov:wasDerivedFrom\twas derived from\t\n"
        );
        // Paging: `--limit` and `--offset`.
        let out = env.run([
            "--json", "relation", "list", "--limit", "2", "--offset", "1",
        ]);
        let v = ok(&out);
        let ids: Vec<&str> = v["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["digestOf", "excerptOf"]);
        assert_eq!(v["total"], 10);
        assert_eq!(v["truncated"], true);
        assert_eq!(v["next_offset"], 3);
        let out = env.run(["relation", "list", "--limit", "2", "--offset", "1"]);
        assert_eq!(out.stderr, "note: showing 2 of 10; next: --offset 3\n");
        // A store with no custom kinds lists the four standard ones.
        let env = Env::with_vocabulary("");
        let v = ok(&env.run(["--json", "relation", "list"]));
        assert_eq!(v["total"], 4);
        let ids: Vec<&str> = v["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["id"].as_str().unwrap())
            .collect();
        assert_eq!(
            ids,
            [
                "dcterms:isPartOf",
                "dcterms:references",
                "dcterms:relation",
                "prov:wasDerivedFrom"
            ]
        );
        assert!(
            v["results"]
                .as_array()
                .unwrap()
                .iter()
                .all(|k| k["standard"] == true)
        );
    }

    #[test]
    fn relation_list_ignores_dry_run() {
        let env = Env::new();
        let before = env.state();
        for json_flag in [false, true] {
            let base: Vec<&str> = if json_flag {
                vec!["--json", "relation", "list"]
            } else {
                vec!["relation", "list"]
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
            "<{HALI}esc> a <http://www.w3.org/2002/07/owl#ObjectProperty> ; \
             <http://www.w3.org/2000/01/rdf-schema#subPropertyOf> <http://purl.org/dc/terms/relation> ; \
             <http://www.w3.org/2000/01/rdf-schema#label> \"esc\\u001B[31mred\"@en .\n"
        ));
        let out = env.run(["relation", "list"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert!(
            out.stdout
                .contains("esc\tesc\\u{1b}[31mred\tdcterms:relation\n"),
            "{:?}",
            out.stdout
        );
        assert!(!out.stdout.contains('\u{1b}'));
        let out = env.tty(b"n\n", ["relation", "remove", "esc"]);
        assert_eq!(out.code, Some(1));
        assert!(
            out.stderr
                .starts_with("remove relation esc \"esc\\u{1b}[31mred\"? [y/N] "),
            "{:?}",
            out.stderr
        );
        assert!(!out.stderr.contains('\u{1b}'));
        let out = env.run(["relation", "remove", "esc", "--yes"]);
        assert_eq!(out.stdout, "removed relation esc \"esc\\u{1b}[31mred\"\n");
    }

    #[test]
    fn only_vocabulary_changes() {
        let env = Env::new();
        let before = env.state();
        ok(&env.run([
            "--json",
            "relation",
            "add",
            "cites",
            "--parent",
            "dcterms:references",
        ]));
        let after = env.state();
        assert_ne!(after["vocabulary.ttl"], before["vocabulary.ttl"]);
        assert_eq!(after["resources.ttl"], before["resources.ttl"]);
        assert_eq!(after["extensions.ttl"], before["extensions.ttl"]);
        ok(&env.run(["--json", "relation", "remove", "excerptOf", "--yes"]));
        let last = env.state();
        assert_ne!(last["vocabulary.ttl"], after["vocabulary.ttl"]);
        assert_eq!(last["resources.ttl"], before["resources.ttl"]);
        assert_eq!(last["extensions.ttl"], before["extensions.ttl"]);
        // Only excerptOf's triples left the vocabulary; extensions.ttl keeps its comment.
        assert!(!env.vocabulary().contains("excerptOf"));
        assert!(env.vocabulary().contains("hali:annotates"));
        assert!(
            String::from_utf8(last["extensions.ttl"].clone())
                .unwrap()
                .contains("kept in extensions")
        );
    }

    // ---- show ---------------------------------------------------------------------------------

    #[test]
    fn show_relations_text() {
        let env = Env::new();
        let out = env.run(["show", R1]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        assert_eq!(
            out.stdout,
            format!(
                "id\t{R1}\n\
                 title\tr1\n\
                 path\t/fixtures/relations/r1.txt\n\
                 media_type\ttext/plain\n\
                 editable\ttrue\n\
                 missing\ttrue\n\
                 stale\tfalse\n\
                 types\tNote\n\
                 tags\t\n\
                 byte_size\t6\n\
                 modified\t2026-10-20T14:02:11Z\n\
                 relation\t-> dcterms:isPartOf {R2} \"r2\"\n\
                 relation\t<- annotates {R3} \"r3\"\n\
                 relation\t<- dcterms:relation {R3} \"r3\"\n\
                 relation\t<- prov:wasDerivedFrom {R4} \"r4\"\n\
                 catalog_record\t0192f0c4-0000-7000-8000-000000000711\n\
                 issued\t2026-10-21T08:00:00Z\n\
                 record_modified\t2026-10-21T08:00:00Z\n"
            )
        );
        // A Resource nothing relates to has no relation lines.
        let out = env.run(["show", R5]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(
            out.stdout
                .lines()
                .filter(|l| l.starts_with("relation\t"))
                .collect::<Vec<_>>(),
            [format!("relation\t-> quotes {R3} \"r3\"")]
        );
        // A title with an escape sequence is escaped in the relation line.
        let env = Env::new();
        let resources = env
            .resources()
            .replace("dcterms:title \"r2\"", "dcterms:title \"r2\\u001B[31m\"");
        std::fs::write(env.data().join("resources.ttl"), resources).unwrap();
        let out = env.run(["show", R1]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert!(
            out.stdout.contains(&format!(
                "relation\t-> dcterms:isPartOf {R2} \"r2\\u{{1b}}[31m\"\n"
            )),
            "{:?}",
            out.stdout
        );
        assert!(!out.stdout.contains('\u{1b}'));
    }

    // ---- find --related-to and --via ----------------------------------------------------------

    #[test]
    fn find_related_filters() {
        let env = Env::new();
        let find = |args: &[&str]| {
            let mut argv = vec!["--json", "find"];
            argv.extend_from_slice(args);
            ok(&env.run(argv))
        };
        assert_eq!(titles(&find(&["--related-to", R1])), ["r2", "r3", "r4"]);
        // Either direction, and a kind that only extensions.ttl declares.
        assert_eq!(titles(&find(&["--related-to", R3])), ["r1", "r5"]);
        assert_eq!(
            titles(&find(&["--related-to", R3, "--via", "quotes"])),
            ["r5"]
        );
        assert_eq!(
            titles(&find(&["--related-to", R5, "--via", "quotes"])),
            ["r3"]
        );
        // --via includes sub-properties; a custom kind and the kinds under it.
        assert_eq!(
            titles(&find(&["--related-to", R2, "--via", "annotates"])),
            ["r4"]
        );
        assert!(titles(&find(&["--related-to", R1, "--via", "summarises"])).is_empty());
        assert_eq!(
            titles(&find(&["--related-to", R1, "--via", "dcterms:references"])),
            ["r3"]
        );
        assert_eq!(
            titles(&find(&["--related-to", R1, "--via", "prov:wasDerivedFrom"])),
            ["r4"]
        );
        // ANDed with the other filters.
        assert_eq!(
            titles(&find(&["--related-to", R1, "--tag", "ml"])),
            ["r3", "r4"]
        );
        assert_eq!(
            titles(&find(&["--related-to", R1, "--type", "Note"])),
            ["r4"]
        );
        assert_eq!(
            titles(&find(&["--related-to", R1, "--tag", "ml", "--title", "r3"])),
            ["r3"]
        );
        // Nothing related: an empty result, exit 0.
        let v = find(&["--related-to", R1, "--via", "excerptOf"]);
        assert_eq!(v["results"], serde_json::json!([]));
        assert_eq!(v["total"], 0);
    }

    #[test]
    fn find_related_to_by_path_and_text() {
        let env = Env::new();
        let out = env.run(["find", "--related-to", "/fixtures/relations/r5.txt"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(
            out.stdout,
            format!("{R3}\tr3\t/fixtures/relations/r3.txt\tmissing\n")
        );
        let out = env.run(["find", "--related-to", R5, "--path-only"]);
        assert_eq!(out.stdout, "/fixtures/relations/r3.txt\n");
    }

    #[test]
    fn via_without_related_to_exits_2() {
        let env = Env::new();
        // `--related-to` alone, and with `--via`, are accepted...
        ok(&env.run(["--json", "find", "--related-to", R1]));
        ok(&env.run(["--json", "find", "--related-to", R1, "--via", "annotates"]));
        // ...`--via` alone, and a repeated or valueless flag, are usage errors.
        for args in [
            vec!["find", "--via", "annotates"],
            vec!["--json", "find", "--via", "annotates", "--tag", "ml"],
            vec!["find", "--related-to"],
            vec!["find", "--related-to", R1, "--related-to", R2],
            vec!["find", "--related-to", R1, "--via"],
        ] {
            let out = env.run(args.clone());
            assert_eq!(out.code, Some(2), "{args:?}: {}", out.stderr);
            assert_eq!(out.stdout, "");
        }
    }

    #[test]
    fn find_related_unknown_references() {
        let env = Env::new();
        let before = env.state();
        let v = failed(
            &env.run(["--json", "find", "--related-to", GONE]),
            1,
            "UNKNOWN_RESOURCE",
        );
        assert_eq!(v["error"]["details"]["ref"], GONE);
        let v = failed(
            &env.run(["--json", "find", "--related-to", R1, "--via", "nope"]),
            1,
            "UNKNOWN_RELATION",
        );
        assert_eq!(v["error"]["details"]["relation"], "nope");
        // Resolution order: the Resource, the tags, the Types, then the relation.
        failed(
            &env.run([
                "--json",
                "find",
                "--related-to",
                R1,
                "--via",
                "nope",
                "--type",
                "Nope",
                "--tag",
                "nope-tag",
            ]),
            1,
            "UNKNOWN_CONCEPT",
        );
        failed(
            &env.run([
                "--json",
                "find",
                "--related-to",
                R1,
                "--via",
                "nope",
                "--type",
                "Nope",
            ]),
            1,
            "UNKNOWN_TYPE",
        );
        failed(
            &env.run([
                "--json",
                "find",
                "--related-to",
                GONE,
                "--via",
                "nope",
                "--tag",
                "nope-tag",
            ]),
            1,
            "UNKNOWN_RESOURCE",
        );
        assert_eq!(env.state(), before);
    }

    // ---- relation remove: confirmation (spec §9.8) -----------------------------------------------

    #[test]
    fn remove_in_use_before_confirmation() {
        // IN_USE comes before the confirmation: no prompt, with or without a terminal, and the
        // vocabulary is as it was.
        let env = Env::new();
        let before = env.state();
        for out in [
            env.run(["--json", "relation", "remove", "annotates"]),
            env.tty(b"y\n", ["--json", "relation", "remove", "annotates"]),
            env.tty(b"y\n", ["--json", "relation", "remove", "digestOf"]),
        ] {
            assert!(!out.stderr.contains("[y/N]"), "{}", out.stderr);
            failed(&out, 1, "IN_USE");
        }
        assert_eq!(env.state(), before);
    }

    #[test]
    fn remove_standard_is_usage() {
        let env = Env::new();
        let before = env.state();
        for id in [
            "dcterms:relation",
            "dcterms:isPartOf",
            "prov:wasDerivedFrom",
        ] {
            let v = failed(
                &env.run(["--json", "relation", "remove", id, "--yes"]),
                2,
                "USAGE",
            );
            assert_eq!(
                v["error"]["message"],
                format!("{id} is a standard relation and cannot be removed")
            );
        }
        assert_eq!(env.state(), before);
    }

    #[test]
    fn remove_without_a_terminal_and_with_yes() {
        let env = Env::new();
        let before = env.state();
        for (extra, args) in [
            (&[][..], vec!["--json", "relation", "remove", "excerptOf"]),
            (
                &[("HALI_TEST_TTY", "0")][..],
                vec!["--json", "relation", "remove", "excerptOf"],
            ),
            (
                &[("HALI_TEST_TTY", "0")][..],
                vec!["relation", "remove", "EXCERPTOF", "--json"],
            ),
        ] {
            let out = env.exec(extra, b"y\n", args);
            let v = failed(&out, 1, "CONFIRMATION_REQUIRED");
            assert_eq!(v["error"]["message"], NOT_A_TTY);
            assert_eq!(
                v["error"]["details"],
                serde_json::json!({"command": "relation remove", "declined": false})
            );
            assert_eq!(v["error"]["suggested_fix"], FIX);
            assert_eq!(v["error"]["suggested_command"], retry("excerptOf"));
            assert_eq!(env.state(), before);
        }
        // With `--yes`, anywhere, it goes ahead, still with no terminal.
        for args in [
            vec!["relation", "remove", "excerptOf", "--yes"],
            vec!["--yes", "relation", "remove", "excerptOf"],
            vec!["relation", "--yes", "remove", "excerptOf"],
        ] {
            let env = Env::new();
            let out = env.run(args.clone());
            assert_eq!(out.code, Some(0), "{args:?}: {}", out.stderr);
            assert_eq!(out.stdout, "removed relation excerptOf \"excerpt of\"\n");
            assert!(!env.vocabulary().contains("excerptOf"));
        }
    }

    #[test]
    fn remove_retry_command_is_shell_safe() {
        // Ids from a hand-edited store: one that starts with `-`, one that needs quoting and
        // one from the prefix table, whose `:` needs quoting too.
        let owl = "<http://www.w3.org/2002/07/owl#ObjectProperty>";
        let sub = "<http://www.w3.org/2000/01/rdf-schema#subPropertyOf>";
        let lab = "<http://www.w3.org/2000/01/rdf-schema#label>";
        let parent = "<http://purl.org/dc/terms/relation>";
        let env = Env::with_vocabulary(&format!(
            "<{HALI}-x> a {owl} ; {sub} {parent} ; {lab} \"dash x\"@en .\n\
             <{HALI}a;b> a {owl} ; {sub} {parent} ; {lab} \"semi\"@en .\n\
             <http://www.w3.org/2004/02/skos/core#related> a {owl} ; {sub} {parent} ; {lab} \"related\"@en .\n"
        ));
        let v = failed(
            &env.run(["--json", "relation", "remove", "--", "-x"]),
            1,
            "CONFIRMATION_REQUIRED",
        );
        assert_eq!(
            v["error"]["suggested_command"],
            "hali --yes relation remove -- -x"
        );
        let v = failed(
            &env.run(["--json", "relation", "remove", "a;b"]),
            1,
            "CONFIRMATION_REQUIRED",
        );
        assert_eq!(
            v["error"]["suggested_command"],
            "hali relation remove 'a;b' --yes"
        );
        let v = failed(
            &env.run(["--json", "relation", "remove", "skos:related"]),
            1,
            "CONFIRMATION_REQUIRED",
        );
        assert_eq!(
            v["error"]["suggested_command"],
            "hali relation remove 'skos:related' --yes"
        );
        // The reference may itself start with `-`, after `--`.
        let out = env.run(["relation", "remove", "--yes", "--", "-x"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stdout, "removed relation -x \"dash x\"\n");
    }

    #[test]
    fn remove_tty_prompt_yes_no_eof() {
        let ask = prompt("excerptOf", "excerpt of");

        // y and yes, trimmed, in any case, confirm. The prompt is the only thing on stderr and
        // the summary is the only thing on stdout (spec §9.8).
        for answer in ["y\n", "yes\n", "Y\n", "YES\n", "  y  \n", "\tyes\r\n"] {
            let env = Env::new();
            let out = env.tty(answer.as_bytes(), ["relation", "remove", "excerptOf"]);
            assert_eq!(out.code, Some(0), "{answer:?}: {}", out.stderr);
            assert_eq!(out.stderr, ask, "{answer:?}");
            assert_eq!(
                out.stdout, "removed relation excerptOf \"excerpt of\"\n",
                "{answer:?}"
            );
            assert!(!env.vocabulary().contains("excerptOf"), "{answer:?}");
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
            let out = env.tty(answer, ["relation", "remove", "excerptOf"]);
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
            let out = env.tty(answer, ["--json", "relation", "remove", "excerptOf"]);
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
                serde_json::json!({"command": "relation remove", "declined": true})
            );
            assert_eq!(v["error"]["suggested_command"], retry("excerptOf"));
            assert_eq!(out.code, Some(1));
            assert_eq!(env.state(), before);
        }

        // `--json` and a yes: stdout is the clean payload, the prompt is on stderr.
        let env = Env::new();
        let out = env.tty(b"y\n", ["--json", "relation", "remove", "excerptOf"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, ask);
        assert_eq!(json(&out.stdout)["relation_kind"]["id"], "excerptOf");

        // `--yes` skips the prompt; the answer on stdin is `n` and is never read.
        let env = Env::new();
        let out = env.tty(b"n\n", ["relation", "remove", "excerptOf", "--yes"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        assert!(!env.vocabulary().contains("excerptOf"));
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
            let out = env.exec(
                extra,
                stdin,
                ["--dry-run", "relation", "remove", "excerptOf"],
            );
            assert_eq!(out.code, Some(0), "{extra:?}: {}", out.stderr);
            assert_eq!(
                out.stderr, "note: dry run; nothing was written\n",
                "{extra:?}"
            );
            let lines: Vec<&str> = out.stdout.lines().collect();
            assert_eq!(lines[0], "would remove relation excerptOf \"excerpt of\"");
            assert_eq!(lines.len(), 1 + 3, "{}", out.stdout);
            assert!(lines[1..].iter().all(|l| l.starts_with("- <")), "{lines:?}");
            // JSON: nothing on stderr.
            let out = env.exec(
                extra,
                stdin,
                ["relation", "remove", "excerptOf", "--json", "--dry-run"],
            );
            assert_eq!(out.code, Some(0), "{extra:?}: {}", out.stderr);
            assert_eq!(out.stderr, "", "{extra:?}");
            assert_eq!(json(&out.stdout)["dry_run"], true);
            assert_eq!(env.state(), before, "{extra:?}");
        }
        // A dry run reports the errors of a real run, with identical stderr.
        let dry = env.run(["--json", "--dry-run", "relation", "remove", "annotates"]);
        let real = env.run(["--json", "relation", "remove", "annotates", "--yes"]);
        assert_eq!(dry.code, Some(1));
        assert_eq!(dry.stderr, real.stderr);
        assert!(!dry.stderr.contains("dry_run"));
    }

    #[test]
    fn remove_extensions_only_kind_is_unchanged() {
        // `sketch` exists only in extensions.ttl and nothing uses it: nothing to remove,
        // nothing to ask, with or without a terminal.
        let env = Env::new();
        let before = env.state();
        let out = env.tty(b"n\n", ["relation", "remove", "sketch"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stdout, "unchanged relation sketch \"sketch\"\n");
        assert_eq!(out.stderr, "", "no prompt");
        let out = env.run(["--json", "relation", "remove", "sketch"]);
        let v = ok(&out);
        assert_eq!(v["relation_kind"]["id"], "sketch");
        assert_eq!(env.state(), before);
    }

    #[test]
    fn remove_never_waits_on_stdin() {
        // stdin is a pipe that stays open and is never written: a command that read it would
        // hang. Each run must exit on its own, with the code of its own path, well inside 10 s.
        let runs: [(&[&str], i32); 12] = [
            (&["relation", "remove", "excerptOf"], 1),
            (&["relation", "remove", "excerptOf", "--yes"], 0),
            (&["--dry-run", "relation", "remove", "excerptOf"], 0),
            (&["relation", "remove", "annotates"], 1),
            (&["relation", "remove", "sketch"], 0),
            (
                &["relation", "add", "cites", "--parent", "dcterms:relation"],
                0,
            ),
            (&["relation", "list"], 0),
            (&["relate", R5, "annotates", R2], 0),
            (&["unrelate", R3, "annotates", R1], 0),
            (&["forget", R5], 1),
            (&["forget", R1], 1),
            (&["forget", R5, "--yes"], 0),
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

    // ---- forget and --force ------------------------------------------------------------------------

    #[test]
    fn forget_blocked_before_confirmation() {
        // FORGET_BLOCKED_BY_RELATIONS comes before the confirmation: no prompt, with or without
        // a terminal, and `--yes` does not get around it.
        let env = Env::new();
        let before = env.state();
        for out in [
            env.run(["--json", "forget", R1]),
            env.tty(b"y\n", ["--json", "forget", R1]),
            env.run(["--json", "forget", R1, "--yes"]),
            env.tty(b"y\n", ["--json", "forget", R2]),
        ] {
            assert!(!out.stderr.contains("[y/N]"), "{}", out.stderr);
            failed(&out, 1, "FORGET_BLOCKED_BY_RELATIONS");
        }
        // A dry run reports the same error, with identical stderr.
        let dry = env.run(["--json", "--dry-run", "forget", R1]);
        let real = env.run(["--json", "forget", R1]);
        assert_eq!(dry.code, Some(1));
        assert_eq!(dry.stderr, real.stderr);
        // Counted from every graph: r3's only inbound relation is the one extensions.ttl holds.
        let v = failed(
            &env.run(["--json", "forget", R3]),
            1,
            "FORGET_BLOCKED_BY_RELATIONS",
        );
        assert_eq!(
            v["error"]["details"],
            serde_json::json!({"resource": R3, "relations": 1, "from": 1})
        );
        assert_eq!(env.state(), before);
        // Outbound relations do not block: r5 only relates to r3.
        let out = env.run(["forget", R5, "--yes"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
    }

    #[test]
    fn forget_force_removes_the_relations_too() {
        let env = Env::new();
        let before = env.state();
        let out = env.run(["forget", R1, "--force", "--yes"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(
            out.stdout,
            format!("forgot resource {R1} \"r1\" /fixtures/relations/r1.txt\n")
        );
        let after = env.state();
        assert_ne!(after["resources.ttl"], before["resources.ttl"]);
        assert_eq!(after["vocabulary.ttl"], before["vocabulary.ttl"]);
        assert_eq!(after["extensions.ttl"], before["extensions.ttl"]);
        assert!(!env.resources().contains(R1), "{}", env.resources());
        // r3 keeps only the relation extensions.ttl holds.
        let v = ok(&env.run(["--json", "show", R3]));
        let relations = v["resource"]["relations"].as_array().unwrap();
        assert_eq!(relations.len(), 1, "{v}");
        assert_eq!(relations[0]["direction"], "in");
        assert_eq!(relations[0]["relation"], "quotes");
        // The flag may come before the reference; with nothing to block, it changes nothing.
        let env = Env::new();
        let out = env.run(["forget", "--force", R5, "--yes"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert!(out.stdout.starts_with("forgot resource "));
    }

    #[test]
    fn forget_force_retry_has_force() {
        let env = Env::new();
        let before = env.state();
        // The retry of a confirmation carries `--force` when `--force` was given, and not
        // otherwise.
        let v = failed(
            &env.run(["--json", "forget", R1, "--force"]),
            1,
            "CONFIRMATION_REQUIRED",
        );
        assert_eq!(
            v["error"]["message"],
            "forget needs confirmation, but stdin or stderr is not a terminal; nothing was written"
        );
        assert_eq!(
            v["error"]["details"],
            serde_json::json!({"command": "forget", "declined": false})
        );
        assert_eq!(
            v["error"]["suggested_command"],
            format!("hali forget {R1} --force --yes")
        );
        let v = failed(
            &env.run(["--json", "forget", "--force", R5]),
            1,
            "CONFIRMATION_REQUIRED",
        );
        assert_eq!(
            v["error"]["suggested_command"],
            format!("hali forget {R5} --force --yes")
        );
        let v = failed(
            &env.run(["--json", "forget", R5]),
            1,
            "CONFIRMATION_REQUIRED",
        );
        assert_eq!(
            v["error"]["suggested_command"],
            format!("hali forget {R5} --yes")
        );
        assert_eq!(env.state(), before);
        // On a terminal: the prompt is unchanged, and a no declines with the same retry.
        let out = env.tty(b"n\n", ["--json", "forget", R1, "--force"]);
        let ask = format!("forget resource {R1} \"r1\" /fixtures/relations/r1.txt? [y/N] ");
        let rest = out
            .stderr
            .strip_prefix(&ask)
            .unwrap_or_else(|| panic!("no prompt first: {}", out.stderr));
        let v = json(rest.trim_start_matches('\n'));
        assert_eq!(v["error"]["details"]["declined"], true);
        assert_eq!(
            v["error"]["suggested_command"],
            format!("hali forget {R1} --force --yes")
        );
        assert_eq!(env.state(), before);
        // A yes forgets it and its relations.
        let out = env.tty(b"y\n", ["forget", R1, "--force"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, ask);
        assert!(!env.resources().contains(R1));
    }

    #[test]
    fn forget_force_extensions_inbound_is_shacl() {
        // `r5 quotes r3` is in extensions.ttl, which hali never writes: forgetting r3, even
        // with --force, would leave a relation to a node that is no Resource.
        let env = Env::new();
        let before = env.state();
        let v = failed(
            &env.run(["--json", "forget", R3, "--force", "--yes"]),
            1,
            "SHACL_VIOLATION",
        );
        assert_eq!(
            v["error"]["details"]["violations"][0]["focus_node"],
            format!("<urn:uuid:{R5}>")
        );
        assert_eq!(env.state(), before);
    }

    // ---- schemas ----------------------------------------------------------------------------------

    #[test]
    fn schema_lists_relation_names() {
        let env = Env::without_store();
        let out = env.run(["schema"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        for name in [
            "relate",
            "unrelate",
            "relation add",
            "relation list",
            "relation remove",
        ] {
            assert!(
                out.stdout.lines().any(|l| l == name),
                "`hali schema` should list `{name}`: {}",
                out.stdout
            );
        }
        let v = ok(&env.run(["schema", "--json"]));
        let names: Vec<&str> = v["schemas"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_str().unwrap())
            .collect();
        assert_eq!(names.len(), 22, "{names:?}");
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted, "the names are sorted");
        // Each is served as a bare JSON Schema document.
        for name in [
            "relate",
            "unrelate",
            "relation add",
            "relation list",
            "relation remove",
        ] {
            let mut args = vec!["schema"];
            args.extend(name.split(' '));
            let out = env.run(args);
            assert_eq!(out.code, Some(0), "{name}: {}", out.stderr);
            let doc = json(&out.stdout);
            assert_eq!(
                doc["$schema"], "https://json-schema.org/draft/2020-12/schema",
                "{name}"
            );
        }
    }

    // ---- --dry-run (spec §9.5, §14.3) ---------------------------------------------------------
    //
    // The new mutating commands under the dry-run equality helper over tests/fixtures/relations,
    // and the hand-written dry-run goldens
    // tests/snapshots/cli_dry_run__{relate,unrelate,relation_add,relation_remove}_dry_run_json.snap.
    // The real run of `relation remove` and `forget` gets `--yes`, as there is no terminal; a
    // dry run ignores it.

    mod dry_run {
        use super::{Env, R1, R2, R3, R5};
        use crate::support;
        use serde_json::{Value, json};
        use std::path::{Path, PathBuf};

        fn fixture() -> PathBuf {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/relations")
        }

        fn ops(dry: &Value) -> Vec<(&str, &str)> {
            dry["changes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| (c["op"].as_str().unwrap(), c["triple"].as_str().unwrap()))
                .collect()
        }

        /// The dry run through the real binary, as stdout bytes, for the golden.
        fn dry_stdout(args: &[&str]) -> String {
            let env = Env::new();
            let before = env.state();
            let mut argv = vec!["--json", "--dry-run"];
            argv.extend_from_slice(args);
            let out = env.run(argv);
            assert_eq!(out.code, Some(0), "{}", out.stderr);
            assert_eq!(out.stderr, "");
            assert_eq!(env.state(), before, "a dry run writes nothing");
            out.stdout
        }

        #[test]
        fn relate_matches() {
            let store = fixture();
            let args = ["relate", R5, "annotates", R2];
            let dry = support::assert_dry_run_matches_real_run(Some(&store), &args);
            assert_eq!(
                dry["relation"],
                json!({"from": R5, "relation": "annotates", "to": R2})
            );
            // The triple, the old `modified` out and the new one in.
            assert_eq!(ops(&dry).len(), 3, "{dry}");
            assert_eq!(ops(&dry).iter().filter(|(op, _)| *op == "add").count(), 2);
            insta::with_settings!({prepend_module_to_snapshot => false}, {
                insta::assert_snapshot!("cli_dry_run__relate_dry_run_json", dry_stdout(&args));
            });

            // No-ops: a triple resources.ttl holds; the same relation by another reference.
            for noop in [
                vec!["relate", R1, "dcterms:isPartOf", R2],
                vec!["relate", R1, "DCTERMS:ISPARTOF", R2],
            ] {
                let dry = support::assert_dry_run_matches_real_run(Some(&store), &noop);
                assert_eq!(dry["changes"], json!([]), "{noop:?}");
            }
            // A triple only extensions.ttl holds is still written to resources.ttl.
            let dry = support::assert_dry_run_matches_real_run(
                Some(&store),
                &["relate", R5, "quotes", R3],
            );
            assert_eq!(ops(&dry).len(), 3, "{dry}");
        }

        #[test]
        fn unrelate_matches() {
            let store = fixture();
            let args = ["unrelate", R3, "annotates", R1];
            let dry = support::assert_dry_run_matches_real_run(Some(&store), &args);
            assert_eq!(
                dry["relation"],
                json!({"from": R3, "relation": "annotates", "to": R1})
            );
            assert_eq!(ops(&dry).len(), 3, "{dry}");
            assert_eq!(
                ops(&dry).iter().filter(|(op, _)| *op == "remove").count(),
                2
            );
            insta::with_settings!({prepend_module_to_snapshot => false}, {
                insta::assert_snapshot!("cli_dry_run__unrelate_dry_run_json", dry_stdout(&args));
            });

            // No-ops: held only in extensions.ttl; absent; a super-property of the one held.
            for noop in [
                vec!["unrelate", R5, "quotes", R3],
                vec!["unrelate", R2, "dcterms:relation", R1],
                vec!["unrelate", R3, "dcterms:references", R1],
            ] {
                let dry = support::assert_dry_run_matches_real_run(Some(&store), &noop);
                assert_eq!(dry["changes"], json!([]), "{noop:?}");
            }
        }

        #[test]
        fn relation_add_matches() {
            let store = fixture();
            let args = ["relation", "add", "cites", "--parent", "dcterms:references"];
            let dry = support::assert_dry_run_matches_real_run(Some(&store), &args);
            assert_eq!(dry["relation_kind"]["id"], "cites");
            assert_eq!(
                dry["relation_kind"]["parents"],
                json!(["dcterms:references"])
            );
            assert_eq!(ops(&dry).len(), 3, "{dry}");
            assert!(ops(&dry).iter().all(|(op, _)| *op == "add"), "{dry}");
            insta::with_settings!({prepend_module_to_snapshot => false}, {
                insta::assert_snapshot!(
                    "cli_dry_run__relation_add_dry_run_json",
                    dry_stdout(&args)
                );
            });
        }

        #[test]
        fn relation_remove_matches() {
            let store = fixture();
            let args = ["relation", "remove", "excerptOf"];
            let mut real_args = args.to_vec();
            real_args.push("--yes");
            let dry = support::assert_dry_run_matches_real_run(Some(&store), &real_args);
            assert_eq!(dry["relation_kind"]["id"], "excerptOf");
            let listed = ops(&dry);
            assert_eq!(listed.len(), 3, "{dry}");
            assert!(listed.iter().all(|(op, _)| *op == "remove"), "{dry}");
            insta::with_settings!({prepend_module_to_snapshot => false}, {
                insta::assert_snapshot!(
                    "cli_dry_run__relation_remove_dry_run_json",
                    dry_stdout(&args)
                );
            });

            // A kind that exists only in extensions.ttl: `changes` is `[]`.
            let dry = support::assert_dry_run_matches_real_run(
                Some(&store),
                &["relation", "remove", "sketch", "--yes"],
            );
            assert_eq!(dry["changes"], json!([]));
        }

        #[test]
        fn forget_force_matches() {
            // r1 with its three inbound relations: every triple of r1, its checksum node, its
            // catalog record, and the relations that point at it (r3 twice, r4 once).
            let store = fixture();
            let dry = support::assert_dry_run_matches_real_run(
                Some(&store),
                &["forget", R1, "--force", "--yes"],
            );
            assert_eq!(dry["resource"]["id"], R1);
            let listed = ops(&dry);
            assert!(listed.iter().all(|(op, _)| *op == "remove"), "{dry}");
            let inbound = listed
                .iter()
                .filter(|(_, t)| t.ends_with(&format!("<urn:uuid:{R1}> .")))
                .count();
            // The three relations, and the catalog record's primary topic.
            assert_eq!(inbound, 4, "{dry}");
        }
    }
}
