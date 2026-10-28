//! Goal tests for tagging Resources with Concepts through the real binary (#16, spec §2.3,
//! §9.2, §9.4, §9.7). Locked golden snapshots sit in tests/snapshots/ (hand-written from the
//! spec and the issue's contract): `cli_tag__add_tag_json`, `cli_tag__edit_add_remove_tag_json`
//! and the stderr envelope `cli_tag__unknown_concept_json_envelope`.
//!
//! Every run sets HALI_DATA_DIR / XDG_CONFIG_HOME / HOME to temp dirs and clears the rest of
//! the environment, so the real store and config are never read. The temp root is
//! canonicalised and shown as `/TMP` in captured output. Files get a pinned mtime of
//! 2026-10-20T14:02:11Z plus a sub-second part, as in tests/cli_find.rs. Runs default to
//! `HALI_TEST_ID_SEED=1`, so the first Resource `add` mints is R1; a test that mints ids in
//! several processes passes a distinct seed to each (`run_seeded`), since every process
//! restarts the seeded sequence.
//!
//! Wording the contract leaves open and the golden fixes (change the golden, with the human,
//! if the wording should differ): the UNKNOWN_CONCEPT message is
//! `no Concept matches <ref>` and its suggested_fix is
//! `Use the slug or a label of an existing Concept, or add the Concept first.`

mod goal {
    use assert_cmd::Command;
    use serde_json::Value;
    use std::collections::BTreeMap;
    use std::ffi::OsStr;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};
    use tempfile::TempDir;

    const R1: &str = "018bcfe5-6801-710a-adec-89025cc1beeb";
    const NOW: &str = "2026-10-24T09:30:00Z";
    const LATER: &str = "2026-10-25T10:00:00Z";
    const MTIME_SECS: u64 = 1_792_504_931; // 2026-10-20T14:02:11Z
    const FIX: &str = "Use the slug or a label of an existing Concept, or add the Concept first.";

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
        /// An environment with two Concepts: `machine-learning` and `semantic-web`.
        fn new() -> Env {
            let tmp = tempfile::tempdir().unwrap();
            let root = std::fs::canonicalize(tmp.path()).unwrap();
            for d in ["cfg", "home", "files"] {
                std::fs::create_dir_all(root.join(d)).unwrap();
            }
            let env = Env { _root: tmp, root };
            for label in ["machine learning", "semantic web"] {
                ok(&env.run(["--json", "concept", "add", label]));
            }
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

        fn run_with<I, S>(&self, now: &str, args: I) -> Out
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            self.run_seeded("1", now, args)
        }

        /// Each process restarts the seeded id sequence, so a test that mints ids in several
        /// processes gives each its own seed (as tests/cli_find.rs does).
        fn run_seeded<I, S>(&self, seed: &str, now: &str, args: I) -> Out
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
                .env("HALI_TEST_NOW", now)
                .env("HALI_TEST_ID_SEED", seed)
                .timeout(Duration::from_secs(30))
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

        fn run<I, S>(&self, args: I) -> Out
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            self.run_with(NOW, args)
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

        /// Catalogs `name` (id R1) with the two Concepts.
        fn notes_with_both_tags(&self) -> String {
            let path = self.file("notes.txt");
            ok(&self.run([
                "--json",
                "add",
                &path,
                "--tag",
                "semantic web",
                "--tag",
                "machine-learning",
            ]));
            path
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

    fn snapshot_stdout(name: &str, out: &Out) {
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        // The locked snapshots are named without the `goal__` module segment.
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!(name, out.stdout);
        });
    }

    fn tag_ids(v: &Value) -> Vec<&str> {
        v.as_array()
            .expect("tags is an array")
            .iter()
            .map(|t| t["id"].as_str().expect("tag id"))
            .collect()
    }

    fn usage(out: &Out, what: &str) {
        assert_eq!(out.code, Some(2), "{what}: stderr: {}", out.stderr);
        assert_eq!(out.stdout, "", "{what}");
        assert!(
            !out.stderr.is_empty(),
            "{what}: the error says what is wrong"
        );
    }

    // ---- criterion 1: tags on add and edit ---------------------------------------------------

    #[test]
    fn add_tag_json() {
        let env = Env::new();
        let path = env.file("notes.txt");
        let out = env.run([
            "--json",
            "add",
            &path,
            "--tag",
            "semantic web",
            "--tag",
            "machine-learning",
        ]);
        snapshot_stdout("cli_tag__add_tag_json", &out);
    }

    #[test]
    fn edit_add_remove_tag_json() {
        let env = Env::new();
        let path = env.notes_with_both_tags();
        ok(&env.run([
            "--json",
            "concept",
            "add",
            "deep learning",
            "--broader",
            "machine learning",
        ]));
        let out = env.run_with(
            LATER,
            [
                "--json",
                "edit",
                &path,
                "--add-tag",
                "deep-learning",
                "--remove-tag",
                "Semantic Web",
            ],
        );
        snapshot_stdout("cli_tag__edit_add_remove_tag_json", &out);

        // The catalog record's modified moved to now; issued did not.
        let v = ok(&env.run_with(LATER, ["--json", "show", R1]));
        assert_eq!(v["resource"]["catalog_record"]["modified"], LATER);
        assert_eq!(v["resource"]["catalog_record"]["issued"], NOW);
        assert_eq!(
            tag_ids(&v["resource"]["tags"]),
            ["deep-learning", "machine-learning"]
        );
    }

    #[test]
    fn add_tag_is_ignored_for_an_already_cataloged_path() {
        let env = Env::new();
        let path = env.file("notes.txt");
        ok(&env.run(["--json", "add", &path]));
        let before = env.state();
        let v = ok(&env.run(["--json", "add", &path, "--tag", "machine-learning"]));
        assert_eq!(v["added"], serde_json::json!([]));
        assert_eq!(v["existing"][0]["tags"], serde_json::json!([]));
        assert_eq!(env.state(), before, "nothing was written");
    }

    #[test]
    fn edit_text_summaries() {
        let env = Env::new();
        let path = env.notes_with_both_tags();

        let out = env.run_with(
            LATER,
            ["--dry-run", "edit", &path, "--remove-tag", "semantic-web"],
        );
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "note: dry run; nothing was written\n");
        assert_eq!(
            out.stdout.lines().next().unwrap(),
            format!("would edit resource {R1} \"notes\"")
        );

        let out = env.run_with(LATER, ["edit", &path, "--remove-tag", "semantic-web"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stdout, format!("edited resource {R1} \"notes\"\n"));

        // Removing an absent tag is a silent no-op.
        let before = env.state();
        let out = env.run_with(LATER, ["edit", &path, "--remove-tag", "semantic-web"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stdout, format!("unchanged resource {R1} \"notes\"\n"));
        assert_eq!(out.stderr, "");
        assert_eq!(env.state(), before);

        let out = env.run_with(LATER, ["edit", &path, "--title", "Notes"]);
        assert_eq!(out.stdout, format!("edited resource {R1} \"Notes\"\n"));
    }

    #[test]
    fn edit_noops_leave_the_store_and_modified_alone() {
        let env = Env::new();
        let path = env.notes_with_both_tags();
        // A Concept the Resource is not tagged with.
        ok(&env.run(["--json", "concept", "add", "deep learning"]));
        let before = env.state();
        let v = ok(&env.run_with(
            LATER,
            [
                "--json",
                "edit",
                &path,
                "--add-tag",
                "Machine  Learning",
                "--remove-tag",
                "deep-learning",
            ],
        ));
        assert_eq!(
            tag_ids(&v["resource"]["tags"]),
            ["machine-learning", "semantic-web"]
        );
        assert_eq!(env.state(), before, "no write, so modified is not bumped");
    }

    #[test]
    fn edit_requires_a_change_and_refuses_a_tag_in_both_lists() {
        let env = Env::new();
        let path = env.notes_with_both_tags();
        let before = env.state();
        usage(&env.run(["edit", &path]), "no flag");
        usage(
            &env.run([
                "edit",
                &path,
                "--add-tag",
                "semantic-web",
                "--remove-tag",
                "semantic-web",
            ]),
            "same slug",
        );
        usage(
            &env.run([
                "--json",
                "edit",
                &path,
                "--add-tag",
                "semantic-web",
                "--remove-tag",
                "Semantic Web",
            ]),
            "slug and label",
        );
        assert_eq!(env.state(), before);
    }

    // ---- criterion 2: UNKNOWN_CONCEPT ---------------------------------------------------------

    #[test]
    fn unknown_concept_from_add_edit_find_concept_add() {
        let env = Env::new();
        let path = env.file("notes.txt");
        ok(&env.run(["--json", "add", &path]));
        let ideas = env.file("ideas.txt");
        let before = env.state();

        let out = env.run([
            "--json",
            "add",
            &ideas,
            "--tag",
            "semantic-web",
            "--tag",
            "machin learning",
        ]);
        assert_eq!(out.code, Some(1), "{}", out.stderr);
        assert_eq!(out.stdout, "");
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!("cli_tag__unknown_concept_json_envelope", out.stderr);
        });

        // The same Concept reference is the same error from every command that takes one.
        let first = out.stderr.clone();
        assert_eq!(json(&first)["error"]["rule"], "UNKNOWN_CONCEPT");
        for args in [
            vec!["--json", "edit", R1, "--add-tag", "machin learning"],
            vec!["--json", "edit", R1, "--remove-tag", "machin learning"],
            vec![
                "--json",
                "--dry-run",
                "edit",
                R1,
                "--add-tag",
                "machin learning",
            ],
            vec!["--json", "find", "--tag", "machin learning"],
            vec!["--json", "find", "--exact", "--tag", "machin learning"],
            vec![
                "--json",
                "concept",
                "add",
                "deep learning",
                "--broader",
                "machin learning",
            ],
            vec![
                "--json",
                "--dry-run",
                "concept",
                "add",
                "deep learning",
                "--broader",
                "machin learning",
            ],
        ] {
            let out = env.run(&args);
            assert_eq!(out.code, Some(1), "{args:?}: {}", out.stderr);
            assert_eq!(out.stdout, "", "{args:?}");
            assert_eq!(out.stderr, first, "{args:?}");
        }
        assert_eq!(env.state(), before, "nothing was written");
    }

    #[test]
    fn unknown_concept_reports_the_first_in_argument_order_and_quotes_the_command() {
        let env = Env::new();
        let path = env.file("notes.txt");
        let out = env.run(["--json", "add", &path, "--tag", "it's", "--tag", "nope"]);
        assert_eq!(out.code, Some(1), "{}", out.stderr);
        let e = json(&out.stderr);
        assert_eq!(e["error"]["details"]["concept"], "it's");
        assert_eq!(
            e["error"]["suggested_command"],
            "hali --dry-run concept add 'it'\\''s'"
        );
        assert!(
            !env.data().join("resources.ttl").exists(),
            "nothing was cataloged"
        );
    }

    #[test]
    fn leading_dash_suggestion_parses_and_empty_reference_is_usage() {
        let env = Env::new();
        let out = env.run(["--json", "find", "--tag=-x"]);
        assert_eq!(out.code, Some(1), "{}", out.stderr);
        let e = json(&out.stderr);
        let cmd = e["error"]["suggested_command"].as_str().unwrap();
        assert!(cmd.ends_with(" -- -x"), "{cmd}");
        // The suggested command is accepted by clap (a dry run adds nothing).
        let words: Vec<&str> = cmd.split(' ').skip(1).collect();
        let out = env.run(words);
        assert_eq!(out.code, Some(0), "{}", out.stderr);

        let path = env.file("notes.txt");
        ok(&env.run(["--json", "add", &path]));
        for args in [
            vec!["find", "--tag="],
            vec!["find", "--tag", "  "],
            vec!["add", &path, "--tag="],
            vec!["concept", "add", "X", "--broader="],
            vec!["edit", &path, "--add-tag="],
            vec!["edit", &path, "--remove-tag="],
        ] {
            usage(&env.run(&args), &format!("{args:?}"));
        }
    }

    #[test]
    fn edit_without_a_change_is_a_usage_error_before_the_store_is_read() {
        let env = Env::new();
        usage(&env.run(["edit", "/nonexistent"]), "edit, no flags");
    }

    #[test]
    fn unknown_concept_text() {
        let env = Env::new();
        let out = env.run(["find", "--tag", "machin learning"]);
        assert_eq!(out.code, Some(1), "{}", out.stderr);
        assert_eq!(out.stdout, "");
        assert_eq!(
            out.stderr,
            format!(
                "error: no Concept matches machin learning\nhint: {FIX} Try: hali --dry-run concept add 'machin learning'\n"
            )
        );
    }

    // ---- criterion 3: find --tag over a hierarchy ---------------------------------------------

    #[test]
    fn find_tag_matches_narrower_concepts_end_to_end() {
        let env = Env::new();
        ok(&env.run_seeded(
            "11",
            NOW,
            [
                "--json",
                "concept",
                "add",
                "deep learning",
                "--broader",
                "machine learning",
            ],
        ));
        ok(&env.run_seeded(
            "12",
            NOW,
            [
                "--json",
                "concept",
                "add",
                "transformers",
                "--broader",
                "Deep Learning",
            ],
        ));
        let notes = env.file("notes.txt");
        let ideas = env.file("ideas.txt");
        let other = env.file("other.txt");
        ok(&env.run_seeded(
            "13",
            NOW,
            [
                "--json",
                "add",
                &notes,
                "--title",
                "notes",
                "--tag",
                "transformers",
            ],
        ));
        ok(&env.run_seeded(
            "14",
            NOW,
            [
                "--json",
                "add",
                &ideas,
                "--title",
                "ideas",
                "--tag",
                "machine learning",
            ],
        ));
        ok(&env.run_seeded(
            "15",
            NOW,
            [
                "--json",
                "add",
                &other,
                "--title",
                "other",
                "--tag",
                "semantic web",
            ],
        ));

        let titles = |args: &[&str]| -> Vec<String> {
            let mut full = vec!["--json", "find", "--sort", "title"];
            full.extend_from_slice(args);
            let v = ok(&env.run(&full));
            v["results"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r["title"].as_str().unwrap().to_string())
                .collect()
        };
        assert_eq!(titles(&["--tag", "machine learning"]), ["ideas", "notes"]);
        assert_eq!(titles(&["--tag", "machine-learning", "--exact"]), ["ideas"]);
        assert_eq!(titles(&["--tag", "deep-learning"]), ["notes"]);
        assert_eq!(titles(&["--tag", "transformers", "--exact"]), ["notes"]);
        assert_eq!(titles(&["--tag", "semantic-web"]), ["other"]);
        // Repeated --tag ANDs.
        assert_eq!(
            titles(&["--tag", "machine-learning", "--tag", "transformers"]),
            ["notes"]
        );
        assert!(titles(&["--tag", "machine-learning", "--tag", "semantic-web"]).is_empty());
        // --exact applies to every --tag.
        assert!(
            titles(&[
                "--tag",
                "machine-learning",
                "--tag",
                "transformers",
                "--exact"
            ])
            .is_empty()
        );
        // --exact without --tag is accepted and does nothing.
        assert_eq!(titles(&["--exact"]), ["ideas", "notes", "other"]);

        // Text mode lists the same Resources.
        let out = env.run(["find", "--tag", "machine learning", "--sort", "title"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stdout.lines().count(), 2, "{}", out.stdout);

        // An unknown Concept is an error, not an empty result.
        let out = env.run(["--json", "find", "--tag", "nope"]);
        assert_eq!(out.code, Some(1), "{}", out.stderr);
        assert_eq!(out.stdout, "");
    }
}
