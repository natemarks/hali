//! Goal tests for managing Concepts through the real binary (#17, spec §2.3, §9.1, §9.4, §9.5,
//! §9.8, §14.2): `concept show`, `concept rename`, `concept edit` and `concept remove`. Locked
//! golden snapshots sit in tests/snapshots/ (hand-written from the spec and the issue's
//! contract): `cli_concept_manage__concept_{show,rename,edit,remove}_json` (stdout) and the
//! stderr envelopes `cli_concept_manage__in_use_json_envelope` and
//! `cli_concept_manage__remove_confirmation_required_json_envelope`. The dry-run goldens
//! `cli_dry_run__concept_{rename,edit,remove}_dry_run_json` are checked in tests/cli_dry_run.rs.
//!
//! Every run sets HALI_DATA_DIR / XDG_CONFIG_HOME / HOME to temp dirs and clears the rest of
//! the environment, so the real store and config are never read. The store is
//! tests/fixtures/concepts (see tests/core_concept_manage.rs), with a `# hand-edited` comment
//! line put in front of resources.ttl so that a rewrite of it shows up as a byte difference.
//! Concept commands mint no id, so every run uses `HALI_TEST_ID_SEED=1`.
//!
//! The terminal seam is `HALI_TEST_TTY=1|0` (test-hooks builds only, spec §14.2). The prompt of
//! `concept remove` is `remove concept <id> "<label>"? [y/N] ` on stderr (spec §9.8).
//!
//! Wording the contract leaves open and the goldens and tests fix (change them, with the human,
//! if the wording should differ):
//! - the text of `concept show` joins list values with `, `, and an empty list is an empty
//!   value (`narrower<TAB>`), as `hali show` does;
//! - the `IN_USE` suggested_fix is `Untag the Resources (hali edit <ref> --remove-tag <id>) and
//!   unlink the narrower Concepts (hali concept edit <C> --remove-broader <id>), then retry.`;
//!   the message keeps "Concepts" whatever the count;
//! - the `CONFIRMATION_REQUIRED` message names the command as `concept remove`.

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
    const FIX: &str =
        "Run it again with --yes to confirm, or with --dry-run to see what it would change.";
    const NOT_A_TTY: &str = "concept remove needs confirmation, but stdin or stderr is not a \
                             terminal; nothing was written";
    const DECLINED: &str = "concept remove was not confirmed; nothing was written";
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
        /// The `concepts` fixture in the data dir.
        fn new() -> Env {
            let tmp = tempfile::tempdir().unwrap();
            let root = std::fs::canonicalize(tmp.path()).unwrap();
            for d in ["cfg", "home", "data"] {
                std::fs::create_dir_all(root.join(d)).unwrap();
            }
            let env = Env { _root: tmp, root };
            let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/concepts");
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

        fn file(&self, name: &str) -> String {
            std::fs::read_to_string(self.data().join(name)).unwrap()
        }

        fn vocabulary(&self) -> String {
            self.file("vocabulary.ttl")
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
        format!("remove concept {id} \"{label}\"? [y/N] ")
    }

    fn retry(id: &str) -> String {
        format!("hali concept remove {id} --yes")
    }

    // ---- golden snapshots (§14.1) -------------------------------------------------------------

    #[test]
    fn concept_show_json() {
        let env = Env::new();
        let out = env.run(["--json", "concept", "show", "dog"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!("cli_concept_manage__concept_show_json", out.stdout);
        });
    }

    #[test]
    fn concept_rename_json() {
        let env = Env::new();
        let out = env.run(["--json", "concept", "rename", "cat", "feline"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!("cli_concept_manage__concept_rename_json", out.stdout);
        });
        let vocabulary = env.vocabulary();
        assert!(vocabulary.contains("\"feline\"@en"), "{vocabulary}");
        assert!(vocabulary.contains("hali:cat "), "the IRI is unchanged");
    }

    #[test]
    fn concept_edit_json() {
        let env = Env::new();
        let out = env.run([
            "--json",
            "concept",
            "edit",
            "cat",
            "--add-alt",
            "Moggy",
            "--remove-alt",
            "kitty",
            "--add-broader",
            "pet",
        ]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!("cli_concept_manage__concept_edit_json", out.stdout);
        });
    }

    #[test]
    fn concept_remove_json() {
        let env = Env::new();
        let out = env.run(["--json", "concept", "remove", "cat", "--yes"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!("cli_concept_manage__concept_remove_json", out.stdout);
        });
        assert!(!env.vocabulary().contains("hali:cat "));
    }

    #[test]
    fn in_use_json_envelope() {
        let env = Env::new();
        let before = env.state();
        let out = env.run(["--json", "concept", "remove", "fish", "--yes"]);
        assert_eq!(out.code, Some(1), "{}", out.stderr);
        assert_eq!(out.stdout, "");
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!("cli_concept_manage__in_use_json_envelope", out.stderr);
        });
        assert_eq!(env.state(), before);

        // The other reasons, each with its own counts: a parent (mammal: two Concepts under
        // it, one a link only extensions.ttl holds for bird) and a tag only extensions.ttl holds.
        for (id, tagged, narrower) in [("mammal", 0, 2), ("bird", 0, 1), ("plant", 1, 0)] {
            let v = failed(
                &env.run(["--json", "concept", "remove", id, "--yes"]),
                1,
                "IN_USE",
            );
            assert_eq!(
                v["error"]["details"],
                serde_json::json!({"concept": id, "tagged": tagged, "narrower": narrower})
            );
        }
        // Text mode: one `error:` line and one `hint:` line.
        let out = env.run(["concept", "remove", "mammal", "--yes"]);
        assert_eq!(out.code, Some(1));
        assert_eq!(
            out.stderr,
            "error: Concept mammal is in use: 0 tagged, broader of 2 Concepts; nothing was written\n\
             hint: Untag the Resources (hali edit <ref> --remove-tag mammal) and unlink the \
             narrower Concepts (hali concept edit <C> --remove-broader mammal), then retry. \
             Try: hali concept show mammal\n"
        );
        assert_eq!(env.state(), before);
    }

    #[test]
    fn remove_in_use_before_confirmation() {
        // IN_USE comes before the confirmation: no prompt, with or without a terminal, and the
        // vocabulary is as it was.
        let env = Env::new();
        let before = env.state();
        for out in [
            env.run(["--json", "concept", "remove", "fish"]),
            env.tty(b"y\n", ["--json", "concept", "remove", "fish"]),
            env.tty(b"y\n", ["--json", "concept", "remove", "mammal"]),
        ] {
            assert!(!out.stderr.contains("[y/N]"), "{}", out.stderr);
            failed(&out, 1, "IN_USE");
        }
        assert_eq!(env.state(), before);
    }

    #[test]
    fn remove_confirmation_required_json_envelope() {
        let env = Env::new();
        let before = env.state();
        let out = env.run(["--json", "concept", "remove", "cat"]);
        assert_eq!(out.code, Some(1), "{}", out.stderr);
        assert_eq!(out.stdout, "");
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!(
                "cli_concept_manage__remove_confirmation_required_json_envelope",
                out.stderr
            );
        });
        assert_eq!(env.state(), before);

        // The suggested command names the id, whatever the reference was; `--json` may come
        // after the subcommand.
        let by_label = env.run(["concept", "remove", "Cat", "--json"]);
        assert_eq!(by_label.stderr, out.stderr);
        assert_eq!(by_label.code, Some(1));
    }

    // ---- show ---------------------------------------------------------------------------------

    #[test]
    fn show_ignores_dry_run_and_writes_nothing() {
        let env = Env::new();
        let before = env.state();
        for json_flag in [false, true] {
            let base: Vec<&str> = if json_flag {
                vec!["--json", "concept", "show", "dog"]
            } else {
                vec!["concept", "show", "dog"]
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

        let v = ok(&env.run(["--json", "concept", "show", "Hound"]));
        assert_eq!(v["concept"]["id"], "dog");
        let v = failed(
            &env.run(["--json", "concept", "show", "doge"]),
            1,
            "UNKNOWN_CONCEPT",
        );
        assert_eq!(v["error"]["details"]["concept"], "doge");
        let v = ok(&env.run(["--json", "concept", "show", "bird"]));
        assert_eq!(v["concept"]["narrower"], serde_json::json!(["parrot"]));
        assert_eq!(v["concept"]["tagged"], 0);
        let v = ok(&env.run(["--json", "concept", "show", "plant"]));
        assert_eq!(v["concept"]["tagged"], 1);
    }

    #[test]
    fn text_summaries() {
        let env = Env::new();

        // show: one `name<TAB>value` line per field.
        let out = env.run(["concept", "show", "dog"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(
            out.stdout,
            "id\tdog\nlabel\tdog\nalt_labels\thound\nbroader\tmammal, pet\nnarrower\t\ntagged\t1\n"
        );
        assert_eq!(out.stderr, "");
        let out = env.run(["concept", "show", "mammal"]);
        assert_eq!(
            out.stdout,
            "id\tmammal\nlabel\tmammal\nalt_labels\t\nbroader\tanimal\nnarrower\tcat, dog\ntagged\t0\n"
        );

        // rename.
        let out = env.run(["--dry-run", "concept", "rename", "dog", "pooch"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "note: dry run; nothing was written\n");
        let lines: Vec<&str> = out.stdout.lines().collect();
        assert_eq!(lines[0], "would rename concept dog \"pooch\"");
        assert_eq!(lines.len(), 4, "{}", out.stdout);
        assert!(lines[1].starts_with("+ <"), "{lines:?}");
        assert!(lines[2].starts_with("+ <"), "{lines:?}");
        assert!(lines[3].starts_with("- <"), "{lines:?}");
        let out = env.run(["concept", "rename", "dog", "pooch"]);
        assert_eq!(out.stdout, "renamed concept dog \"pooch\"\n");
        assert_eq!(out.stderr, "");
        let out = env.run(["concept", "rename", "dog", "pooch"]);
        assert_eq!(out.stdout, "unchanged concept dog \"pooch\"\n");

        // edit.
        let out = env.run(["--dry-run", "concept", "edit", "cat", "--add-alt", "puss"]);
        assert_eq!(
            out.stdout.lines().next(),
            Some("would edit concept cat \"cat\"")
        );
        let out = env.run(["concept", "edit", "cat", "--add-alt", "puss"]);
        assert_eq!(out.stdout, "edited concept cat \"cat\"\n");
        assert_eq!(out.stderr, "");
        let out = env.run(["concept", "edit", "cat", "--add-alt", "puss"]);
        assert_eq!(out.stdout, "unchanged concept cat \"cat\"\n");
        assert_eq!(out.code, Some(0));

        // remove: the dry run lists the removals, one `- ` line each.
        let out = env.run(["--dry-run", "concept", "remove", "cat"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        let lines: Vec<&str> = out.stdout.lines().collect();
        assert_eq!(lines[0], "would remove concept cat \"cat\"");
        assert_eq!(lines.len(), 1 + 8, "{}", out.stdout); // puss was added above
        assert!(lines[1..].iter().all(|l| l.starts_with("- <")), "{lines:?}");
        assert!(lines[1..].windows(2).all(|w| w[0] < w[1]), "{lines:?}");
        let out = env.run(["concept", "remove", "cat", "--yes"]);
        assert_eq!(out.stdout, "removed concept cat \"cat\"\n");
        assert_eq!(out.stderr, "");

        // A Concept that exists only in extensions.ttl: nothing to remove, nothing to ask.
        let before = env.state();
        let out = env.tty(b"n\n", ["concept", "remove", "parrot"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stdout, "unchanged concept parrot \"parrot\"\n");
        assert_eq!(out.stderr, "", "no prompt");
        let out = env.run(["--json", "concept", "remove", "parrot"]);
        let v = ok(&out);
        assert_eq!(v["concept"]["id"], "parrot");
        assert_eq!(env.state(), before);
    }

    #[test]
    fn labels_are_printable() {
        // A hand-edited label with an escape sequence is shown escaped, in the text of show,
        // in the summaries and in the prompt.
        let env = Env::with_vocabulary(&format!(
            "@prefix skos: <http://www.w3.org/2004/02/skos/core#> .\n\
             <{HALI}topics> a skos:ConceptScheme .\n\
             <{HALI}esc> a skos:Concept ; skos:inScheme <{HALI}topics> ; \
             skos:prefLabel \"esc\\u001B[31mred\"@en .\n"
        ));
        let out = env.run(["concept", "show", "esc"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert!(
            out.stdout.contains("label\tesc\\u{1b}[31mred\n"),
            "{:?}",
            out.stdout
        );
        assert!(!out.stdout.contains('\u{1b}'));
        let out = env.tty(b"n\n", ["concept", "remove", "esc"]);
        assert_eq!(out.code, Some(1));
        assert!(
            out.stderr
                .starts_with("remove concept esc \"esc\\u{1b}[31mred\"? [y/N] "),
            "{:?}",
            out.stderr
        );
        assert!(!out.stderr.contains('\u{1b}'));
        let out = env.run(["concept", "remove", "esc", "--yes"]);
        assert_eq!(out.stdout, "removed concept esc \"esc\\u{1b}[31mred\"\n");
    }

    // ---- rename and edit: errors write nothing ------------------------------------------------

    #[test]
    fn rename_label_taken_writes_nothing() {
        let env = Env::new();
        let before = env.state();
        let v = failed(
            &env.run(["--json", "concept", "rename", "cat", "Dog"]),
            1,
            "LABEL_TAKEN",
        );
        assert_eq!(
            v["error"]["details"],
            serde_json::json!({"label": "Dog", "language": "en", "concept": "dog"})
        );
        // An altLabel another Concept holds.
        failed(
            &env.run(["--json", "concept", "rename", "cat", "hound"]),
            1,
            "LABEL_TAKEN",
        );
        // edit --add-alt: the same rule.
        let v = failed(
            &env.run(["--json", "concept", "edit", "cat", "--add-alt", "HOUND"]),
            1,
            "LABEL_TAKEN",
        );
        assert_eq!(v["error"]["details"]["concept"], "dog");
        assert_eq!(env.state(), before);
        // A label that is free in English: another Concept's French one.
        ok(&env.run(["--json", "concept", "rename", "cat", "chien"]));
    }

    #[test]
    fn rename_and_edit_unknown_and_usage() {
        let env = Env::new();
        let before = env.state();
        for args in [
            vec!["--json", "concept", "rename", "dogg", "x"],
            vec!["--json", "concept", "edit", "dogg", "--add-alt", "x"],
            vec!["--json", "concept", "edit", "cat", "--add-broader", "ghost"],
            vec![
                "--json",
                "concept",
                "edit",
                "cat",
                "--remove-broader",
                "ghost",
            ],
        ] {
            failed(&env.run(args.clone()), 1, "UNKNOWN_CONCEPT");
        }
        // The first unknown one in order: --add-broader before --remove-broader.
        let v = failed(
            &env.run([
                "--json",
                "concept",
                "edit",
                "cat",
                "--remove-broader",
                "ghost-b",
                "--add-broader",
                "ghost-a",
            ]),
            1,
            "UNKNOWN_CONCEPT",
        );
        assert_eq!(v["error"]["details"]["concept"], "ghost-a");
        // Usage errors, exit 2: itself as a parent; one Concept in both lists.
        for args in [
            vec!["concept", "edit", "cat", "--add-broader", "Cat"],
            vec![
                "concept",
                "edit",
                "cat",
                "--add-broader",
                "pet",
                "--remove-broader",
                "pet",
            ],
        ] {
            let out = env.run(args.clone());
            assert_eq!(out.code, Some(2), "{args:?}: {}", out.stderr);
            assert_eq!(out.stdout, "");
        }
        assert_eq!(env.state(), before);
    }

    #[test]
    fn edit_without_flags_exits_2() {
        let env = Env::new();
        let before = env.state();
        let out = env.run(["concept", "edit", "cat"]);
        assert_eq!(out.code, Some(2), "{}", out.stderr);
        assert_eq!(out.stdout, "");
        assert_eq!(env.state(), before);

        // Usage errors come before the store is read: with a broken store, exit 2, not 3.
        std::fs::write(env.data().join("vocabulary.ttl"), "this is not turtle\n").unwrap();
        let broken = env.state();
        for args in [
            vec!["concept", "edit", "cat"],
            vec![
                "concept",
                "edit",
                "cat",
                "--add-alt",
                "Foo",
                "--remove-alt",
                " foo",
            ],
            vec![
                "--json",
                "concept",
                "edit",
                "cat",
                "--add-alt",
                "Foo",
                "--remove-alt",
                "FOO",
            ],
        ] {
            let out = env.run(args.clone());
            assert_eq!(out.code, Some(2), "{args:?}: {}", out.stderr);
            assert_eq!(out.stdout, "");
        }
        failed(
            &env.run(["--json", "concept", "edit", "cat", "--add-alt", "x"]),
            3,
            "STORE_UNPARSEABLE",
        );
        assert_eq!(env.state(), broken);
    }

    // ---- criterion 7: only vocabulary.ttl changes ---------------------------------------------

    #[test]
    fn only_vocabulary_changes() {
        let env = Env::new();
        let before = env.state();
        assert!(before["resources.ttl"].starts_with(b"# hand-edited\n"));
        for args in [
            vec!["--json", "concept", "rename", "dog", "pooch"],
            vec![
                "--json",
                "concept",
                "edit",
                "dog",
                "--add-alt",
                "pup",
                "--remove-broader",
                "pet",
            ],
            vec!["--json", "concept", "remove", "cat", "--yes"],
        ] {
            let out = env.run(args.clone());
            assert_eq!(out.code, Some(0), "{args:?}: {}", out.stderr);
            let after = env.state();
            assert_eq!(
                before.keys().collect::<Vec<_>>(),
                after.keys().collect::<Vec<_>>(),
                "{args:?}: no file was added or removed"
            );
            for name in ["resources.ttl", "extensions.ttl"] {
                assert_eq!(before[name], after[name], "{args:?}: {name} was written");
            }
            assert_ne!(
                before["vocabulary.ttl"], after["vocabulary.ttl"],
                "{args:?}"
            );
        }
        // The Concepts that are tagged or hang under another are still there, and the Resource
        // still finds its tags.
        let v = ok(&env.run(["--json", "concept", "show", "pooch"]));
        assert_eq!(v["concept"]["id"], "dog");
        assert_eq!(
            v["concept"]["alt_labels"],
            serde_json::json!(["dog", "hound", "pup"])
        );
        assert_eq!(v["concept"]["broader"], serde_json::json!(["mammal"]));
        assert_eq!(v["concept"]["tagged"], 1);
        let v = ok(&env.run(["--json", "validate"]));
        assert_eq!(v["conforms"], true);
    }

    #[test]
    fn remove_checks_come_before_confirmation() {
        let env = Env::new();
        let before = env.state();
        // usage (exit 2) before the store is read.
        let out = env.run(["concept", "remove"]);
        assert_eq!(out.code, Some(2), "{}", out.stderr);
        // domain errors before the confirmation, with no --yes and no terminal.
        failed(
            &env.run(["--json", "concept", "remove", "ghost"]),
            1,
            "UNKNOWN_CONCEPT",
        );
        failed(
            &env.tty(b"y\n", ["--json", "concept", "remove", "ghost"]),
            1,
            "UNKNOWN_CONCEPT",
        );
        assert_eq!(env.state(), before);

        // SHACL (exit 1) before the confirmation: extensions.ttl keeps typing cat as a
        // Concept, so what is left of it (no scheme, no label) would not conform.
        std::fs::write(
            env.data().join("extensions.ttl"),
            format!(
                "@prefix skos: <http://www.w3.org/2004/02/skos/core#> .\n<{HALI}cat> a skos:Concept .\n"
            ),
        )
        .unwrap();
        let held = env.state();
        for out in [
            env.run(["--json", "concept", "remove", "cat"]),
            env.tty(b"y\n", ["--json", "concept", "remove", "cat"]),
            env.run(["--json", "concept", "remove", "cat", "--yes"]),
        ] {
            failed(&out, 1, "SHACL_VIOLATION");
            assert!(!out.stderr.contains("[y/N]"));
        }
        assert_eq!(env.state(), held, "nothing was written");

        // store problem (exit 3) before everything else.
        std::fs::write(env.data().join("vocabulary.ttl"), "this is not turtle\n").unwrap();
        let broken = env.state();
        failed(
            &env.run(["--json", "concept", "remove", "cat"]),
            3,
            "STORE_UNPARSEABLE",
        );
        assert_eq!(env.state(), broken);
    }

    // ---- criterion 8: remove confirms like forget ---------------------------------------------

    #[test]
    fn remove_no_tty_requires_confirmation() {
        let env = Env::new();
        let before = env.state();

        // Text mode: one `error:` line and one `hint:` line; the `y` on piped stdin is not an
        // answer.
        let out = env.run(["concept", "remove", "cat"]);
        assert_eq!(out.code, Some(1));
        assert_eq!(out.stdout, "");
        assert_eq!(
            out.stderr,
            format!("error: {NOT_A_TTY}\nhint: {FIX} Try: {}\n", retry("cat"))
        );
        assert_eq!(env.state(), before, "nothing was written");

        // JSON details; an explicit "not a terminal" behaves the same, and so does a label.
        for (extra, args) in [
            (&[][..], vec!["--json", "concept", "remove", "cat"]),
            (
                &[("HALI_TEST_TTY", "0")][..],
                vec!["--json", "concept", "remove", "Cat"],
            ),
            (
                &[("HALI_TEST_TTY", "0")][..],
                vec!["concept", "remove", "kitty", "--json"],
            ),
        ] {
            let out = env.exec(extra, b"y\n", args);
            let v = failed(&out, 1, "CONFIRMATION_REQUIRED");
            assert_eq!(v["error"]["message"], NOT_A_TTY);
            assert_eq!(
                v["error"]["details"],
                serde_json::json!({"command": "concept remove", "declined": false})
            );
            assert_eq!(v["error"]["suggested_fix"], FIX);
            assert_eq!(v["error"]["suggested_command"], retry("cat"));
            assert_eq!(env.state(), before);
        }

        // With `--yes`, anywhere, it goes ahead, still with no terminal.
        for args in [
            vec!["concept", "remove", "cat", "--yes"],
            vec!["--yes", "concept", "remove", "cat"],
            vec!["concept", "--yes", "remove", "cat"],
        ] {
            let env = Env::new();
            let out = env.run(args.clone());
            assert_eq!(out.code, Some(0), "{args:?}: {}", out.stderr);
            assert_eq!(out.stdout, "removed concept cat \"cat\"\n");
            assert!(!env.vocabulary().contains("hali:cat "));
        }
    }

    #[test]
    fn remove_retry_command_is_shell_safe() {
        // Ids from a hand-edited store: one that starts with `-` and one that needs quoting.
        let env = Env::with_vocabulary(&format!(
            "@prefix skos: <http://www.w3.org/2004/02/skos/core#> .\n\
             <{HALI}topics> a skos:ConceptScheme .\n\
             <{HALI}-x> a skos:Concept ; skos:inScheme <{HALI}topics> ; skos:prefLabel \"dash x\"@en .\n\
             <{HALI}a;b> a skos:Concept ; skos:inScheme <{HALI}topics> ; skos:prefLabel \"semi\"@en .\n"
        ));
        let v = failed(
            &env.run(["--json", "concept", "remove", "dash x"]),
            1,
            "CONFIRMATION_REQUIRED",
        );
        assert_eq!(
            v["error"]["suggested_command"],
            "hali --yes concept remove -- -x"
        );
        let v = failed(
            &env.run(["--json", "concept", "remove", "semi"]),
            1,
            "CONFIRMATION_REQUIRED",
        );
        assert_eq!(
            v["error"]["suggested_command"],
            "hali concept remove 'a;b' --yes"
        );
    }

    #[test]
    fn remove_tty_prompt_yes_no_eof() {
        let ask = prompt("cat", "cat");

        // y and yes, trimmed, in any case, confirm. The prompt is the only thing on stderr and
        // the summary is the only thing on stdout (spec §9.8).
        for answer in ["y\n", "yes\n", "Y\n", "YES\n", "  y  \n", "\tyes\r\n"] {
            let env = Env::new();
            let out = env.tty(answer.as_bytes(), ["concept", "remove", "cat"]);
            assert_eq!(out.code, Some(0), "{answer:?}: {}", out.stderr);
            assert_eq!(out.stderr, ask, "{answer:?}");
            assert_eq!(out.stdout, "removed concept cat \"cat\"\n", "{answer:?}");
            assert!(!env.vocabulary().contains("hali:cat "), "{answer:?}");
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
            let out = env.tty(answer, ["concept", "remove", "cat"]);
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
            let out = env.tty(answer, ["--json", "concept", "remove", "cat"]);
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
                serde_json::json!({"command": "concept remove", "declined": true})
            );
            assert_eq!(v["error"]["suggested_command"], retry("cat"));
            assert_eq!(out.code, Some(1));
            assert_eq!(env.state(), before);
        }

        // `--json` and a yes: stdout is the clean payload, the prompt is on stderr.
        let env = Env::new();
        let out = env.tty(b"y\n", ["--json", "concept", "remove", "cat"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, ask);
        assert_eq!(json(&out.stdout)["concept"]["id"], "cat");

        // `--yes` skips the prompt; the answer on stdin is `n` and is never read.
        let env = Env::new();
        let out = env.tty(b"n\n", ["concept", "remove", "cat", "--yes"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        assert!(!env.vocabulary().contains("hali:cat "));
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
            let out = env.exec(extra, stdin, ["--dry-run", "concept", "remove", "cat"]);
            assert_eq!(out.code, Some(0), "{extra:?}: {}", out.stderr);
            assert_eq!(
                out.stderr, "note: dry run; nothing was written\n",
                "{extra:?}"
            );
            assert_eq!(
                out.stdout.lines().next(),
                Some("would remove concept cat \"cat\"")
            );
            // JSON: nothing on stderr.
            let out = env.exec(
                extra,
                stdin,
                ["concept", "remove", "cat", "--json", "--dry-run"],
            );
            assert_eq!(out.code, Some(0), "{extra:?}: {}", out.stderr);
            assert_eq!(out.stderr, "", "{extra:?}");
            let v = json(&out.stdout);
            assert_eq!(v["dry_run"], true);
            assert_eq!(env.state(), before, "{extra:?}");
        }
        // A dry run reports the errors of a real run, with identical stderr.
        let dry = env.run(["--json", "--dry-run", "concept", "remove", "fish"]);
        let real = env.run(["--json", "concept", "remove", "fish", "--yes"]);
        assert_eq!(dry.code, Some(1));
        assert_eq!(dry.stderr, real.stderr);
        assert!(!dry.stderr.contains("dry_run"));
    }

    #[test]
    fn remove_never_waits_on_stdin() {
        // stdin is a pipe that stays open and is never written: a command that read it would
        // hang. Each run must exit on its own, with the code of its own path, well inside 10 s.
        let runs: [(&[&str], i32); 5] = [
            (&["concept", "remove", "cat"], 1),
            (&["concept", "remove", "cat", "--yes"], 0),
            (&["--dry-run", "concept", "remove", "cat"], 0),
            (&["concept", "remove", "fish"], 1),
            (&["concept", "rename", "cat", "feline"], 0),
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
    fn rename_and_edit_never_ask() {
        // Neither is destructive: no prompt on a terminal, no CONFIRMATION_REQUIRED without one.
        for tty in ["0", "1"] {
            let env = Env::new();
            let out = env.exec(
                &[("HALI_TEST_TTY", tty)],
                b"n\n",
                ["concept", "rename", "cat", "feline"],
            );
            assert_eq!(out.code, Some(0), "tty {tty}: {}", out.stderr);
            assert_eq!(out.stderr, "", "tty {tty}");
            assert_eq!(out.stdout, "renamed concept cat \"feline\"\n");
            let out = env.exec(
                &[("HALI_TEST_TTY", tty)],
                b"n\n",
                ["concept", "edit", "cat", "--add-alt", "puss"],
            );
            assert_eq!(out.code, Some(0), "tty {tty}: {}", out.stderr);
            assert_eq!(out.stderr, "", "tty {tty}");
            assert_eq!(out.stdout, "edited concept cat \"feline\"\n");
        }
        // Without a terminal and without --yes, the same.
        let env = Env::new();
        let out = env.run(["concept", "rename", "cat", "feline"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
    }

    // ---- schemas ------------------------------------------------------------------------------

    #[test]
    fn schema_lists_new_names() {
        let env = Env::new();
        let out = env.run(["schema"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        for name in [
            "concept edit",
            "concept remove",
            "concept rename",
            "concept show",
        ] {
            assert!(
                out.stdout.lines().any(|l| l == name),
                "{name}: {}",
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
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted);
        for name in [
            "concept edit",
            "concept remove",
            "concept rename",
            "concept show",
        ] {
            assert!(names.contains(&name), "{name}: {names:?}");
        }
        for name in ["concept edit", "concept remove", "concept rename"] {
            let words: Vec<&str> = name.split(' ').collect();
            let mut args = vec!["schema"];
            args.extend(words);
            let doc = json(&env.run(args).stdout);
            assert_eq!(
                doc["$schema"],
                "https://json-schema.org/draft/2020-12/schema"
            );
            assert!(
                doc["oneOf"].as_array().is_some_and(|a| a.len() == 2),
                "{name}: {doc}"
            );
        }
        let doc = json(&env.run(["schema", "concept", "show"]).stdout);
        assert_eq!(
            doc["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
        assert!(
            doc.get("oneOf").is_none(),
            "show has no dry-run form: {doc}"
        );
    }
}
