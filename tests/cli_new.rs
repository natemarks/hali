//! Goal tests for `hali new` through the real binary (#25, spec §8, §9.1, §9.6). Locked golden
//! snapshots sit in tests/snapshots/ (hand-written from the approved contract; the ids are the
//! ones `HALI_TEST_ID_SEED=1` yields: Resource, record, Person, then the SoftwareAgent when an
//! agent acts): `cli_new__new_json`, `cli_dry_run__new_dry_run_json`,
//! `cli_dry_run__new_agent_dry_run_json` (stdout) and `cli_new__files_dir_unusable_json_envelope`
//! (stderr). The two dry-run goldens carry the `cli_dry_run__` prefix because
//! tests/schema_contract.rs maps every `*dry_run*` golden to a command from that prefix.
//!
//! Assumed CLI (the approved contract):
//!
//! - `hali new "<title>" [--type T]... [--tag C]... [--path-only]`; the global `--json`,
//!   `--dry-run`, `--agent` and `--yes` apply (`--yes` is ignored: `new` never asks).
//! - Output: `{"schema_version":1,"resource":<Resource record>}`; under `--dry-run` also
//!   `dry_run: true` and `changes`. Text: `created|would create resource <id> "<title>" <path>`,
//!   then under `--dry-run` the `+ <triple>` lines and the stderr note `note: dry run; nothing
//!   was written`. `--path-only` prints exactly `<real path>\n` and wins over `--json`.
//! - `FILES_DIR_UNUSABLE` (exit 3, `details {"path": <string|null>, "reason": ...}`) with the
//!   reasons `unresolved`, `relative`, `not_a_directory`, `own_dir` and `cannot_create`; a real
//!   `files.dir` path that is not UTF-8 is `NON_UTF8_PATH` (exit 1). A usage error (exit 2) for
//!   an empty, blank or control-character title, raised before the store is read.
//! - `hali schema new` is served, with the title `hali new --json`.
//!
//! Isolation: `HOME`, `XDG_CONFIG_HOME`, `XDG_STATE_HOME`, `HALI_DATA_DIR`, `GIT_CONFIG_*` point
//! at temp paths and the rest of the environment is cleared, so `HALI_AGENT`, `EDITOR` and the
//! rest are only set where a test sets them. Every run pins `HALI_TEST_NOW` and
//! `HALI_TEST_ID_SEED` (a second run in one store uses another seed so that ids never repeat).
//! The temp dir is shown as `/TMP` in captured output. The default `files.dir` of these tests is
//! `$HOME/Documents/hali`, and the data dir does not exist until something writes it.

#[cfg(feature = "goal")]
mod goal {
    use assert_cmd::Command;
    use serde_json::Value;
    use std::collections::{BTreeMap, BTreeSet};
    use std::ffi::{OsStr, OsString};
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::path::{Path, PathBuf};
    use std::time::Duration;
    use tempfile::TempDir;

    const R1: &str = "018bcfe5-6801-710a-adec-89025cc1beeb";
    const C1: &str = "018bcfe5-6802-7893-a2ee-fb32555e71c1";
    const P: &str = "018bcfe5-6803-71bb-94d8-d101b5b9c34d";
    const A: &str = "018bcfe5-6804-7099-ac6c-d7363ca585e7";
    const NOW: &str = "2026-10-24T09:30:00Z";
    const NOW_SECS: i64 = 1_792_834_200; // 2026-10-24T09:30:00Z
    /// sha256 of the bytes `# Weekly review\n`.
    const SHA_WEEKLY: &str = "7a74edee5cc8f550445d1596e91acc3bf238a13123810cdfd8387dcfe34cbd0c";
    const PROV: &str = "http://www.w3.org/ns/prov#";
    const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";

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
        fn new() -> Env {
            let tmp = tempfile::tempdir().unwrap();
            let root = std::fs::canonicalize(tmp.path()).unwrap();
            for d in ["cfg", "home", "state"] {
                std::fs::create_dir_all(root.join(d)).unwrap();
            }
            Env { _root: tmp, root }
        }

        /// An Env whose data dir holds the files of tests/fixtures/<name>.
        fn with_fixture(name: &str) -> Env {
            let env = Env::new();
            std::fs::create_dir_all(env.data()).unwrap();
            let from = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures")
                .join(name);
            for entry in std::fs::read_dir(from).unwrap() {
                let entry = entry.unwrap();
                std::fs::copy(entry.path(), env.data().join(entry.file_name())).unwrap();
            }
            env
        }

        fn data(&self) -> PathBuf {
            self.root.join("data")
        }

        fn home(&self) -> PathBuf {
            self.root.join("home")
        }

        /// The default `files.dir` with a `$HOME` of `home`.
        fn docs(&self) -> PathBuf {
            self.home().join("Documents/hali")
        }

        fn resources_ttl(&self) -> PathBuf {
            self.data().join("resources.ttl")
        }

        fn config(&self, text: &str) {
            std::fs::create_dir_all(self.root.join("cfg/hali")).unwrap();
            std::fs::write(self.root.join("cfg/hali/config.toml"), text).unwrap();
        }

        fn clean(&self, text: &str) -> String {
            text.replace(self.root.to_str().unwrap(), "/TMP")
        }

        fn apply(&self, cmd: &mut Command, seed: u64, envs: &[(&str, OsString)], unset: &[&str]) {
            cmd.env_clear()
                .env("XDG_CONFIG_HOME", self.root.join("cfg"))
                .env("XDG_STATE_HOME", self.root.join("state"))
                .env("HOME", self.home())
                .env("HALI_DATA_DIR", self.data())
                .env("GIT_CONFIG_GLOBAL", self.root.join("gitconfig"))
                .env("GIT_CONFIG_SYSTEM", self.root.join("gitconfig-system"))
                .env("HALI_TEST_NOW", NOW)
                .env("HALI_TEST_ID_SEED", seed.to_string())
                .timeout(Duration::from_secs(60))
                .write_stdin(Vec::new());
            for (k, v) in envs {
                cmd.env(k, v);
            }
            for k in unset {
                cmd.env_remove(k);
            }
        }

        fn out(&self, out: std::process::Output) -> Out {
            Out {
                code: out.status.code(),
                stdout: self.clean(&String::from_utf8(out.stdout).unwrap()),
                stderr: self.clean(&String::from_utf8_lossy(&out.stderr)),
            }
        }

        /// One run with the standard environment plus `envs` (set after the clean slate).
        fn run<I, S>(&self, seed: u64, envs: &[(&str, OsString)], args: I) -> Out
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            self.run_with(seed, envs, &[], args)
        }

        /// `run`, with the variables `unset` removed afterwards.
        fn run_with<I, S>(
            &self,
            seed: u64,
            envs: &[(&str, OsString)],
            unset: &[&str],
            args: I,
        ) -> Out
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            let mut cmd = Command::cargo_bin("hali").unwrap();
            self.apply(&mut cmd, seed, envs, unset);
            cmd.args(args);
            self.out(cmd.output().unwrap())
        }

        /// One run of `hali` under `sh -c 'umask <mask>; exec hali ...'`.
        fn run_umask<I, S>(&self, mask: &str, seed: u64, args: I) -> Out
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            let mut cmd = Command::new("sh");
            self.apply(&mut cmd, seed, &[], &[]);
            cmd.arg("-c")
                .arg(format!("umask {mask}; exec \"$0\" \"$@\""))
                .arg(env!("CARGO_BIN_EXE_hali"))
                .args(args);
            self.out(cmd.output().unwrap())
        }

        /// Every triple of resources.ttl as canonical N-Triples (empty when absent).
        fn triples(&self) -> BTreeSet<String> {
            let Ok(bytes) = std::fs::read(self.resources_ttl()) else {
                return BTreeSet::new();
            };
            oxttl::TurtleParser::new()
                .for_slice(&bytes)
                .map(|t| format!("{} .", t.expect("resources.ttl is Turtle")))
                .collect()
        }

        fn count_typed(&self, class: &str) -> usize {
            self.triples()
                .iter()
                .filter(|t| t.contains(&format!("<{RDF_TYPE}> <{PROV}{class}> .")))
                .count()
        }

        /// The data dir's files with their bytes (None when the dir does not exist).
        fn data_bytes(&self) -> Option<BTreeMap<String, Vec<u8>>> {
            let dir = std::fs::read_dir(self.data()).ok()?;
            Some(
                dir.map(|e| {
                    let e = e.unwrap();
                    (
                        e.file_name().into_string().unwrap(),
                        std::fs::read(e.path()).unwrap_or_default(),
                    )
                })
                .collect(),
            )
        }

        /// Nothing was created: `$HOME` is empty and the data dir does not exist.
        fn assert_nothing_created(&self, what: &str) {
            let home: Vec<_> = std::fs::read_dir(self.home())
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect();
            assert!(home.is_empty(), "{what}: $HOME holds {home:?}");
            assert!(!self.data().exists(), "{what}: the data dir was created");
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

    /// A failed run: its exit code, nothing on stdout, and the error envelope on stderr.
    fn failed(out: &Out, code: i32, rule: &str) -> Value {
        assert_eq!(out.code, Some(code), "{}", out.stderr);
        assert_eq!(out.stdout, "");
        let v = json(&out.stderr);
        assert_eq!(v["error"]["rule"], rule, "{}", out.stderr);
        v["error"].clone()
    }

    fn snapshot(name: &str, out: &Out, code: i32, stream: &str) {
        assert_eq!(out.code, Some(code), "{}", out.stderr);
        let (shown, other) = match stream {
            "stdout" => (&out.stdout, &out.stderr),
            _ => (&out.stderr, &out.stdout),
        };
        assert_eq!(other, "", "nothing on the other stream");
        // The locked snapshots are named without the `goal__` module segment.
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!(name, shown);
        });
    }

    fn env_of(key: &'static str, value: &str) -> Vec<(&'static str, OsString)> {
        vec![(key, OsString::from(value))]
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        v.sort();
        v
    }

    // ---- row 1: the first `new` in an empty store (locked golden) -------------------------------

    #[test]
    fn new_json() {
        let env = Env::new();
        let out = env.run(1, &[], ["--json", "new", "Weekly review"]);
        snapshot("cli_new__new_json", &out, 0, "stdout");

        let file = env.docs().join("weekly-review.md");
        assert_eq!(std::fs::read(&file).unwrap(), b"# Weekly review\n");
        assert_eq!(std::fs::read(&file).unwrap().len(), 16);
        assert_eq!(names(&env.docs()), ["weekly-review.md"]);
        let ttl = std::fs::read_to_string(env.resources_ttl()).unwrap();
        assert!(ttl.contains(SHA_WEEKLY), "{ttl}");
        assert!(ttl.contains("text/markdown"), "{ttl}");
    }

    // ---- row 2: the files.dir chain, and a symlinked directory ---------------------------------

    #[test]
    fn files_dir_chain() {
        // Nothing configured: ~/Documents/hali.
        let env = Env::new();
        ok(&env.run(1, &[], ["--json", "new", "Default"]));
        assert_eq!(names(&env.docs()), ["default.md"]);

        // user-dirs.dirs: $XDG_DOCUMENTS_DIR/hali.
        let env = Env::new();
        std::fs::write(
            env.root.join("cfg/user-dirs.dirs"),
            "XDG_DOCUMENTS_DIR=\"$HOME/Papers\"\n",
        )
        .unwrap();
        ok(&env.run(1, &[], ["--json", "new", "From user dirs"]));
        assert_eq!(
            names(&env.home().join("Papers/hali")),
            ["from-user-dirs.md"]
        );
        assert!(!env.home().join("Documents").exists());

        // [files] dir beats user-dirs.dirs; a leading ~/ is $HOME, and missing parents are made.
        let env = Env::new();
        std::fs::write(
            env.root.join("cfg/user-dirs.dirs"),
            "XDG_DOCUMENTS_DIR=\"$HOME/Papers\"\n",
        )
        .unwrap();
        env.config("[files]\ndir = \"~/Notes/inner\"\n");
        ok(&env.run(1, &[], ["--json", "new", "Configured"]));
        assert_eq!(names(&env.home().join("Notes/inner")), ["configured.md"]);
        assert!(!env.home().join("Papers").exists());

        // `~` alone is $HOME.
        let env = Env::new();
        env.config("[files]\ndir = \"~\"\n");
        ok(&env.run(1, &[], ["--json", "new", "Home"]));
        assert_eq!(names(&env.home()), ["home.md"]);

        // An absolute path is used as given.
        let env = Env::new();
        let abs = env.root.join("elsewhere/docs");
        env.config(&format!("[files]\ndir = \"{}\"\n", abs.display()));
        let v = ok(&env.run(1, &[], ["--json", "new", "Absolute"]));
        assert_eq!(v["resource"]["path"], "/TMP/elsewhere/docs/absolute.md");
        assert_eq!(names(&abs), ["absolute.md"]);
    }

    #[test]
    fn symlinked_files_dir_records_real_path() {
        // A configured dir that is a symlink.
        let env = Env::new();
        let real = env.root.join("real/docs");
        std::fs::create_dir_all(&real).unwrap();
        let link = env.root.join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        env.config(&format!("[files]\ndir = \"{}\"\n", link.display()));
        let v = ok(&env.run(1, &[], ["--json", "new", "Linked"]));
        assert_eq!(v["resource"]["path"], "/TMP/real/docs/linked.md");
        assert_eq!(names(&real), ["linked.md"]);
        let ttl = std::fs::read_to_string(env.resources_ttl()).unwrap();
        assert!(
            !ttl.contains("/link/"),
            "the symlink is not recorded: {ttl}"
        );

        // A symlinked ~/Documents is followed too.
        let env = Env::new();
        let elsewhere = env.root.join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::os::unix::fs::symlink(&elsewhere, env.home().join("Documents")).unwrap();
        let v = ok(&env.run(1, &[], ["--json", "new", "Linked"]));
        assert_eq!(v["resource"]["path"], "/TMP/elsewhere/hali/linked.md");
        assert_eq!(names(&elsewhere.join("hali")), ["linked.md"]);
    }

    // ---- row 3: the slug cannot leave the directory --------------------------------------------

    #[test]
    fn traversal_title_stays_in_dir() {
        let env = Env::new();
        let v = ok(&env.run(1, &[], ["--json", "new", "../../etc/x"]));
        assert_eq!(v["resource"]["path"], "/TMP/home/Documents/hali/etc-x.md");
        assert_eq!(v["resource"]["title"], "../../etc/x");
        assert_eq!(names(&env.docs()), ["etc-x.md"]);
        assert_eq!(
            std::fs::read(env.docs().join("etc-x.md")).unwrap(),
            b"# ../../etc/x\n"
        );
        assert_eq!(names(&env.home()), ["Documents"]);
        assert_eq!(names(&env.home().join("Documents")), ["hali"]);
        assert!(!env.root.join("etc").exists());

        // Slashes and backslashes in a title are separators like any other.
        let v = ok(&env.run(2, &[], ["--json", "new", "a/b\\c"]));
        assert_eq!(v["resource"]["path"], "/TMP/home/Documents/hali/a-b-c.md");
    }

    #[test]
    fn long_title_is_kept_whole_and_the_name_is_capped() {
        let env = Env::new();
        let title = "a".repeat(300);
        let v = ok(&env.run(1, &[], ["--json", "new", &title]));
        assert_eq!(v["resource"]["title"], title.as_str());
        assert_eq!(
            std::fs::read(env.docs().join(format!("{}.md", "a".repeat(128)))).unwrap(),
            format!("# {title}\n").as_bytes()
        );
    }

    // ---- row 4: collisions ----------------------------------------------------------------------

    #[test]
    fn collision_suffixes() {
        let env = Env::new();
        for (i, want) in ["same.md", "same-2.md", "same-3.md"].iter().enumerate() {
            let v = ok(&env.run(i as u64 + 1, &[], ["--json", "new", "Same"]));
            assert_eq!(
                v["resource"]["path"],
                format!("/TMP/home/Documents/hali/{want}").as_str()
            );
        }
        assert_eq!(names(&env.docs()), ["same-2.md", "same-3.md", "same.md"]);
        // The titles are all "Same".
        assert_eq!(
            std::fs::read(env.docs().join("same-3.md")).unwrap(),
            b"# Same\n"
        );
    }

    #[test]
    fn existing_things_are_never_overwritten_or_followed() {
        let env = Env::new();
        let docs = env.docs();
        std::fs::create_dir_all(&docs).unwrap();
        // A foreign file.
        std::fs::write(docs.join("other.md"), b"mine\n").unwrap();
        // A directory.
        std::fs::create_dir(docs.join("dir.md")).unwrap();
        // A symlink to a file outside the directory.
        let victim = env.root.join("victim.txt");
        std::fs::write(&victim, b"precious\n").unwrap();
        std::os::unix::fs::symlink(&victim, docs.join("link.md")).unwrap();
        // A dangling symlink.
        let nowhere = env.root.join("nowhere.txt");
        std::os::unix::fs::symlink(&nowhere, docs.join("dangling.md")).unwrap();

        for (i, (title, want)) in [
            ("other", "other-2.md"),
            ("dir", "dir-2.md"),
            ("link", "link-2.md"),
            ("dangling", "dangling-2.md"),
        ]
        .iter()
        .enumerate()
        {
            let v = ok(&env.run(i as u64 + 1, &[], ["--json", "new", title]));
            assert_eq!(
                v["resource"]["path"],
                format!("/TMP/home/Documents/hali/{want}").as_str(),
                "{title}"
            );
        }
        assert_eq!(std::fs::read(docs.join("other.md")).unwrap(), b"mine\n");
        assert!(docs.join("dir.md").is_dir());
        assert_eq!(std::fs::read(&victim).unwrap(), b"precious\n");
        assert!(!nowhere.exists(), "a dangling symlink is never followed");
        assert!(
            std::fs::symlink_metadata(docs.join("dangling.md"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn a_recorded_path_is_taken_even_when_its_file_is_gone() {
        let env = Env::new();
        ok(&env.run(1, &[], ["--json", "new", "Gone"]));
        std::fs::remove_file(env.docs().join("gone.md")).unwrap();
        let v = ok(&env.run(2, &[], ["--json", "new", "Gone"]));
        assert_eq!(v["resource"]["path"], "/TMP/home/Documents/hali/gone-2.md");
        assert_eq!(names(&env.docs()), ["gone-2.md"]);
    }

    // ---- row 5: what is recorded, and the file's own mtime --------------------------------------

    #[test]
    fn file_mtime_is_now() {
        let env = Env::new();
        let v = ok(&env.run(1, &[], ["--json", "new", "Weekly review"]));
        assert_eq!(v["resource"]["modified"], NOW);
        assert_eq!(v["resource"]["stale"], false);
        assert_eq!(v["resource"]["missing"], false);
        assert_eq!(v["resource"]["editable"], true);
        assert_eq!(v["resource"]["media_type"], "text/markdown");
        let meta = std::fs::metadata(env.docs().join("weekly-review.md")).unwrap();
        assert_eq!(meta.mtime(), NOW_SECS);

        // So the next look at the Resource is not Stale.
        let shown = ok(&env.run(2, &[], ["--json", "show", R1]));
        assert_eq!(shown["resource"]["stale"], false, "{shown}");
        assert_eq!(shown["resource"]["missing"], false);
        assert_eq!(shown["resource"]["byte_size"], 16);
        assert_eq!(shown["resource"]["modified"], NOW);
        let found = ok(&env.run(3, &[], ["--json", "find", "--stale"]));
        assert_eq!(found["total"], 0, "{found}");
    }

    #[test]
    fn created_and_attribution_triples_are_written() {
        let env = Env::new();
        ok(&env.run(1, &[], ["--json", "new", "Weekly review"]));
        let t = env.triples();
        let r = format!("<urn:uuid:{R1}>");
        let want = [
            format!(
                "{r} <http://purl.org/dc/terms/created> \"{NOW}\"^^<http://www.w3.org/2001/XMLSchema#dateTime> ."
            ),
            format!("{r} <{PROV}wasAttributedTo> <urn:uuid:{P}> ."),
            format!("<urn:uuid:{C1}> <{PROV}wasAttributedTo> <urn:uuid:{P}> ."),
            format!(
                "{r} <http://www.w3.org/ns/dcat#mediaType> <http://www.iana.org/assignments/media-types/text/markdown> ."
            ),
        ];
        for line in want {
            assert!(t.contains(&line), "missing: {line}");
        }
        assert_eq!(env.count_typed("SoftwareAgent"), 0);
        assert_eq!(env.count_typed("Person"), 1);
    }

    #[test]
    fn types_and_tags_are_applied() {
        let env = Env::with_fixture("store");
        let v = ok(&env.run(
            1,
            &[],
            ["--json", "new", "Typed", "--type", "Paper", "--tag", "ml"],
        ));
        assert_eq!(v["resource"]["types"], serde_json::json!(["Paper"]));
        assert_eq!(
            v["resource"]["tags"],
            serde_json::json!([{"id": "ml", "label": "machine learning"}])
        );
        let found = ok(&env.run(2, &[], ["--json", "find", "--type", "Paper", "--tag", "ml"]));
        assert_eq!(found["total"], 1, "{found}");
    }

    // ---- row 6: the acting agent -----------------------------------------------------------------

    #[test]
    fn new_agent_dry_run_json() {
        let env = Env::new();
        let out = env.run(
            1,
            &[],
            [
                "--json",
                "--dry-run",
                "--agent",
                "claude",
                "new",
                "Weekly review",
            ],
        );
        snapshot("cli_dry_run__new_agent_dry_run_json", &out, 0, "stdout");
        env.assert_nothing_created("an agent dry run");
    }

    #[test]
    fn the_record_and_the_resource_are_attributed_to_the_agent() {
        let env = Env::new();
        ok(&env.run(
            1,
            &[],
            ["--json", "--agent", "claude", "new", "Weekly review"],
        ));
        let t = env.triples();
        for line in [
            format!("<urn:uuid:{R1}> <{PROV}wasAttributedTo> <urn:uuid:{A}> ."),
            format!("<urn:uuid:{C1}> <{PROV}wasAttributedTo> <urn:uuid:{A}> ."),
            format!("<urn:uuid:{A}> <{RDF_TYPE}> <{PROV}SoftwareAgent> ."),
            format!("<urn:uuid:{A}> <{PROV}actedOnBehalfOf> <urn:uuid:{P}> ."),
        ] {
            assert!(t.contains(&line), "missing: {line}");
        }
        assert_eq!(env.count_typed("SoftwareAgent"), 1);
        assert_eq!(env.count_typed("Person"), 1);
        let shown = ok(&env.run(2, &[], ["--json", "show", R1]));
        assert_eq!(
            shown["resource"]["catalog_record"]["attributed_to"]["name"],
            "claude"
        );

        // HALI_AGENT does the same, and the same name is the same Agent.
        let v = ok(&env.run(
            3,
            &env_of("HALI_AGENT", "claude"),
            ["--json", "new", "Second"],
        ));
        let id = v["resource"]["id"].as_str().unwrap().to_string();
        assert_eq!(env.count_typed("SoftwareAgent"), 1, "the Agent is reused");
        let t = env.triples();
        assert!(
            t.contains(&format!(
                "<urn:uuid:{id}> <{PROV}wasAttributedTo> <urn:uuid:{A}> ."
            )),
            "{t:?}"
        );
    }

    // ---- row 7: unknown references create nothing ------------------------------------------------

    #[test]
    fn unknown_refs_create_nothing() {
        // An empty store: not even the data dir appears.
        let env = Env::new();
        let e = failed(
            &env.run(1, &[], ["--json", "new", "Nope", "--type", "Papr"]),
            1,
            "UNKNOWN_TYPE",
        );
        assert_eq!(e["details"]["type"], "Papr");
        env.assert_nothing_created("an unknown type");
        failed(
            &env.run(1, &[], ["--json", "new", "Nope", "--tag", "machin"]),
            1,
            "UNKNOWN_CONCEPT",
        );
        env.assert_nothing_created("an unknown tag");

        // A store with Types and Concepts: its bytes stay as they were, and no directory or
        // file appears.
        let env = Env::with_fixture("store");
        let before = env.data_bytes();
        failed(
            &env.run(1, &[], ["--json", "new", "Nope", "--type", "Papr"]),
            1,
            "UNKNOWN_TYPE",
        );
        failed(
            &env.run(1, &[], ["--json", "new", "Nope", "--tag", "machin"]),
            1,
            "UNKNOWN_CONCEPT",
        );
        failed(
            &env.run(
                1,
                &[],
                [
                    "--json", "new", "Nope", "--type", "Paper", "--tag", "machin",
                ],
            ),
            1,
            "UNKNOWN_CONCEPT",
        );
        assert_eq!(env.data_bytes(), before);
        assert!(names(&env.home()).is_empty(), "nothing in $HOME");
    }

    // ---- row 8: titles ---------------------------------------------------------------------------

    #[test]
    fn invalid_titles_usage() {
        // A control run proves `new` is there at all.
        let control = Env::new();
        ok(&control.run(1, &[], ["--json", "new", "fine"]));

        let env = Env::new();
        for bad in ["", "   ", "a\nb", "tab\there", "bell\u{7}", "\n"] {
            let out = env.run(1, &[], ["--json", "new", bad]);
            let e = failed(&out, 2, "USAGE");
            assert!(
                e["message"].as_str().unwrap().contains("title"),
                "{bad:?}: {e}"
            );
            env.assert_nothing_created(&format!("title {bad:?}"));
        }

        // Not UTF-8: a clap usage error, exit 2, nothing created.
        let mut title = b"bad ".to_vec();
        title.push(0xff);
        let out = env.run(
            1,
            &[],
            [
                OsString::from("--json"),
                OsString::from("new"),
                OsString::from_vec(title),
            ],
        );
        assert_eq!(out.code, Some(2), "{}", out.stderr);
        assert_eq!(out.stdout, "");
        assert!(
            !out.stderr.contains("unrecognized subcommand"),
            "`new` must be a subcommand: {}",
            out.stderr
        );
        env.assert_nothing_created("a non-UTF-8 title");
    }

    #[test]
    fn hash_title_verbatim() {
        let env = Env::new();
        let v = ok(&env.run(1, &[], ["--json", "new", "#x"]));
        assert_eq!(v["resource"]["title"], "#x");
        assert_eq!(v["resource"]["path"], "/TMP/home/Documents/hali/x.md");
        assert_eq!(std::fs::read(env.docs().join("x.md")).unwrap(), b"# #x\n");

        // The title is trimmed.
        let v = ok(&env.run(2, &[], ["--json", "new", "  Padded  "]));
        assert_eq!(v["resource"]["title"], "Padded");
        assert_eq!(
            std::fs::read(env.docs().join("padded.md")).unwrap(),
            b"# Padded\n"
        );

        // No escaping: Markdown characters, quotes and Unicode go in as they are.
        let v = ok(&env.run(3, &[], ["--json", "new", "*Caf\u{e9}* \"q\" <b>"]));
        assert_eq!(v["resource"]["title"], "*Caf\u{e9}* \"q\" <b>");
        assert_eq!(
            std::fs::read(env.docs().join("caf\u{e9}-q-b.md")).unwrap(),
            "# *Caf\u{e9}* \"q\" <b>\n".as_bytes()
        );
    }

    // ---- row 9: --path-only ----------------------------------------------------------------------

    #[test]
    fn path_only_prints_the_path() {
        let env = Env::new();
        let out = env.run(1, &[], ["new", "Weekly review", "--path-only"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stdout, "/TMP/home/Documents/hali/weekly-review.md\n");
        assert_eq!(out.stderr, "");
        assert_eq!(
            std::fs::read(env.docs().join("weekly-review.md")).unwrap(),
            b"# Weekly review\n"
        );
        assert!(env.resources_ttl().exists(), "the Resource is cataloged");

        // --path-only wins over --json.
        let out = env.run(2, &[], ["--json", "new", "Second one", "--path-only"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stdout, "/TMP/home/Documents/hali/second-one.md\n");
        assert_eq!(out.stderr, "");
    }

    #[test]
    fn path_only_with_dry_run_creates_nothing() {
        let env = Env::new();
        let out = env.run(1, &[], ["--dry-run", "new", "Weekly review", "--path-only"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stdout, "/TMP/home/Documents/hali/weekly-review.md\n");
        env.assert_nothing_created("--dry-run --path-only");

        // The same under --json.
        let out = env.run(
            1,
            &[],
            ["--json", "--dry-run", "new", "Weekly review", "--path-only"],
        );
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stdout, "/TMP/home/Documents/hali/weekly-review.md\n");
        env.assert_nothing_created("--dry-run --json --path-only");
    }

    #[test]
    fn path_only_does_not_hide_errors() {
        let env = Env::new();
        let out = env.run(1, &[], ["new", "Nope", "--type", "Papr", "--path-only"]);
        assert_eq!(out.code, Some(1), "{}", out.stderr);
        assert_eq!(out.stdout, "");
        env.assert_nothing_created("--path-only with an unknown type");
    }

    // ---- row 10: dry run ---------------------------------------------------------------------------

    #[test]
    fn new_dry_run_json() {
        let env = Env::new();
        let out = env.run(1, &[], ["--json", "--dry-run", "new", "Weekly review"]);
        snapshot("cli_dry_run__new_dry_run_json", &out, 0, "stdout");
        env.assert_nothing_created("a dry run");
    }

    #[test]
    fn dry_run_matches_real_same_env() {
        let env = Env::new();
        let dry = ok(&env.run(1, &[], ["--json", "--dry-run", "new", "Weekly review"]));
        assert_eq!(dry["dry_run"], true);
        env.assert_nothing_created("a dry run");

        let before = env.triples();
        let real = ok(&env.run(1, &[], ["--json", "new", "Weekly review"]));
        // The payload equals the real run's.
        assert_eq!(dry["resource"], real["resource"]);
        assert_eq!(dry["schema_version"], real["schema_version"]);
        // The changes are the real run's store diff, all additions.
        let diff: BTreeSet<String> = env
            .triples()
            .difference(&before)
            .map(|t| env.clean(t))
            .collect();
        let mut listed = BTreeSet::new();
        for change in dry["changes"].as_array().unwrap() {
            assert_eq!(change["op"], "add", "{change}");
            listed.insert(change["triple"].as_str().unwrap().to_string());
        }
        assert_eq!(listed, diff);
        assert_eq!(dry["changes"].as_array().unwrap().len(), diff.len());
    }

    #[test]
    fn dry_run_in_a_populated_store_leaves_it_alone() {
        let env = Env::with_fixture("store");
        let before = env.data_bytes();
        let out = env.run(
            1,
            &[],
            [
                "--json",
                "--dry-run",
                "new",
                "Typed",
                "--type",
                "Paper",
                "--tag",
                "ml",
            ],
        );
        let v = ok(&out);
        assert_eq!(v["dry_run"], true);
        assert_eq!(v["resource"]["types"], serde_json::json!(["Paper"]));
        assert_eq!(env.data_bytes(), before);
        assert!(names(&env.home()).is_empty());
    }

    // ---- row 11: failures after the file would be written -----------------------------------------

    #[test]
    fn shacl_creates_no_file() {
        let env = Env::with_fixture("shacl/new-resource-shape");
        let before = env.data_bytes();
        let e = failed(
            &env.run(1, &[], ["--json", "new", "x"]),
            1,
            "SHACL_VIOLATION",
        );
        assert_eq!(
            e["details"]["violations"][0]["shape"],
            "http://example.org/shapes#DescriptionRequired"
        );
        assert_eq!(env.data_bytes(), before, "the store is untouched");
        assert!(names(&env.home()).is_empty(), "no directory, no file");
    }

    #[test]
    fn store_io_removes_created_file() {
        let env = Env::new();
        std::fs::create_dir_all(env.data()).unwrap();
        let header = "# generated by hali; edit with the CLI\n";
        std::fs::write(env.resources_ttl(), header).unwrap();
        std::fs::set_permissions(env.resources_ttl(), std::fs::Permissions::from_mode(0o444))
            .unwrap();
        // A root user writes through the mode.
        if std::fs::OpenOptions::new()
            .write(true)
            .open(env.resources_ttl())
            .is_ok()
        {
            return;
        }
        let out = env.run(1, &[], ["--json", "new", "Weekly review"]);
        failed(&out, 3, "STORE_IO");
        assert!(
            !env.docs().join("weekly-review.md").exists(),
            "the created file is removed"
        );
        assert!(env.docs().is_dir(), "the directory stays");
        assert_eq!(
            std::fs::read(env.resources_ttl()).unwrap(),
            header.as_bytes()
        );
        assert_eq!(names(&env.docs()), Vec::<String>::new());
    }

    // ---- row 12: FILES_DIR_UNUSABLE ---------------------------------------------------------------

    fn unusable(out: &Out, reason: &str) -> Value {
        let e = failed(out, 3, "FILES_DIR_UNUSABLE");
        assert_eq!(e["details"]["reason"], reason, "{e}");
        assert!(e["details"].get("path").is_some(), "{e}");
        assert!(!e["message"].as_str().unwrap().is_empty());
        e
    }

    #[test]
    fn files_dir_unusable_unresolved() {
        let env = Env::new();
        let out = env.run_with(1, &[], &["HOME"], ["--json", "new", "x"]);
        let e = unusable(&out, "unresolved");
        assert_eq!(e["details"]["path"], Value::Null);
        assert!(!env.data().exists());
        assert!(names(&env.home()).is_empty());
    }

    #[test]
    fn files_dir_unusable_relative() {
        let env = Env::new();
        env.config("[files]\ndir = \"docs\"\n");
        let out = env.run(1, &[], ["--json", "new", "x"]);
        let e = unusable(&out, "relative");
        assert_eq!(e["details"]["path"], "docs");
        env.assert_nothing_created("a relative files.dir");
    }

    #[test]
    fn files_dir_unusable_json_envelope() {
        let env = Env::new();
        env.config("[files]\ndir = \"docs\"\n");
        let out = env.run(1, &[], ["--json", "new", "x"]);
        assert_eq!(out.stdout, "");
        snapshot(
            "cli_new__files_dir_unusable_json_envelope",
            &out,
            3,
            "stderr",
        );
    }

    #[test]
    fn files_dir_unusable_not_a_directory() {
        let env = Env::new();
        let file = env.root.join("afile");
        std::fs::write(&file, b"keep\n").unwrap();
        env.config(&format!("[files]\ndir = \"{}\"\n", file.display()));
        let out = env.run(1, &[], ["--json", "new", "x"]);
        let e = unusable(&out, "not_a_directory");
        assert_eq!(e["details"]["path"], "/TMP/afile");
        assert_eq!(std::fs::read(&file).unwrap(), b"keep\n");
        assert!(!env.data().exists());
    }

    #[test]
    fn files_dir_unusable_own_dir() {
        // The data dir, below it, and the state dir; none of them exists, and none is made.
        let env = Env::new();
        let state = env.root.join("state/hali");
        for dir in [
            env.data(),
            env.data().join("files"),
            state.clone(),
            state.join("notes"),
        ] {
            env.config(&format!("[files]\ndir = \"{}\"\n", dir.display()));
            let out = env.run(1, &[], ["--json", "new", "x"]);
            unusable(&out, "own_dir");
            assert!(!env.data().exists(), "{dir:?}: the data dir was created");
            assert!(!state.exists(), "{dir:?}: the state dir was created");
        }

        // The same when the data dir exists already.
        let env = Env::with_fixture("store");
        let before = env.data_bytes();
        env.config(&format!("[files]\ndir = \"{}\"\n", env.data().display()));
        unusable(&env.run(1, &[], ["--json", "new", "x"]), "own_dir");
        assert_eq!(env.data_bytes(), before);
    }

    #[test]
    fn files_dir_unusable_cannot_create() {
        let env = Env::new();
        let ro = env.root.join("ro");
        std::fs::create_dir(&ro).unwrap();
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o555)).unwrap();
        let target = ro.join("files");
        // A root user creates the directory regardless.
        let rooted = std::fs::create_dir(&target).is_ok();
        if rooted {
            std::fs::remove_dir(&target).unwrap();
        } else {
            env.config(&format!("[files]\ndir = \"{}\"\n", target.display()));
            let out = env.run(1, &[], ["--json", "new", "x"]);
            let e = unusable(&out, "cannot_create");
            assert_eq!(e["details"]["path"], "/TMP/ro/files");
            assert!(!env.data().exists(), "nothing else was written");
        }
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn a_files_dir_that_is_not_utf8_is_non_utf8_path() {
        let env = Env::new();
        let elsewhere = env.root.join("elsewhere");
        let odd = elsewhere.join(OsString::from_vec(vec![b'd', 0xff, b'r']));
        std::fs::create_dir_all(odd.join("hali")).unwrap();
        std::os::unix::fs::symlink(&elsewhere, env.home().join("Docs")).unwrap();
        std::os::unix::fs::symlink(&odd, env.home().join("Documents")).unwrap();
        let out = env.run(1, &[], ["--json", "new", "x"]);
        let e = failed(&out, 1, "NON_UTF8_PATH");
        assert!(
            e["details"]["path"].as_str().unwrap().contains('\u{fffd}'),
            "{e}"
        );
        assert_eq!(names(&odd.join("hali")), Vec::<String>::new());
        assert!(!env.data().exists());
    }

    // ---- row 13: no editor, no stdin ---------------------------------------------------------------

    #[test]
    fn never_launches_editor() {
        let env = Env::new();
        let marker = env.root.join("editor-ran");
        let script = env.root.join("editor.sh");
        std::fs::write(
            &script,
            format!("#!/bin/sh\ntouch '{}'\ncat >/dev/null\n", marker.display()),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let editor = vec![
            ("EDITOR", script.clone().into_os_string()),
            ("VISUAL", script.into_os_string()),
        ];
        let runs: Vec<Vec<&str>> = vec![
            vec!["new", "One"],
            vec!["--json", "new", "Two"],
            vec!["new", "Three", "--path-only"],
            vec!["--dry-run", "new", "Four"],
        ];
        for (i, args) in runs.iter().enumerate() {
            let out = env.run(i as u64 + 1, &editor, args);
            assert_eq!(out.code, Some(0), "{args:?}: {}", out.stderr);
        }
        assert!(!marker.exists(), "an editor was launched");
    }

    #[test]
    fn never_reads_stdin() {
        let env = Env::new();
        let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_hali"));
        cmd.env_clear()
            .env("XDG_CONFIG_HOME", env.root.join("cfg"))
            .env("XDG_STATE_HOME", env.root.join("state"))
            .env("HOME", env.home())
            .env("HALI_DATA_DIR", env.data())
            .env("HALI_TEST_NOW", NOW)
            .env("HALI_TEST_ID_SEED", "1")
            .args(["--json", "new", "Weekly review"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = cmd.spawn().unwrap();
        // The pipe stays open and silent for the whole run: a read would block.
        let held = child.stdin.take();
        let start = std::time::Instant::now();
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            if start.elapsed() > Duration::from_secs(30) {
                child.kill().unwrap();
                panic!("`new` did not finish with an open, silent stdin: it reads stdin");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        drop(held);
        let out = child.wait_with_output().unwrap();
        assert_eq!(out.status.code(), Some(0));
        assert!(env.docs().join("weekly-review.md").exists());
    }

    #[test]
    fn yes_is_ignored() {
        let env = Env::new();
        let v = ok(&env.run(1, &[], ["--json", "--yes", "new", "Asked"]));
        assert_eq!(v["resource"]["title"], "Asked");
    }

    // ---- row 14: text output ------------------------------------------------------------------------

    #[test]
    fn text_output() {
        let env = Env::new();
        let out = env.run(1, &[], ["new", "Weekly review"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(
            out.stdout,
            format!(
                "created resource {R1} \"Weekly review\" /TMP/home/Documents/hali/weekly-review.md\n"
            )
        );
        assert_eq!(out.stderr, "");

        let env = Env::new();
        let out = env.run(1, &[], ["--dry-run", "new", "Weekly review"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        let lines: Vec<&str> = out.stdout.lines().collect();
        assert_eq!(
            lines[0],
            format!(
                "would create resource {R1} \"Weekly review\" /TMP/home/Documents/hali/weekly-review.md"
            )
        );
        assert_eq!(
            lines.len(),
            19,
            "one summary line and 18 `+` lines: {lines:?}"
        );
        assert!(lines[1..].iter().all(|l| l.starts_with("+ <")), "{lines:?}");
        assert_eq!(out.stderr, "note: dry run; nothing was written\n");
        env.assert_nothing_created("a text dry run");
    }

    // ---- row 15: the schema --------------------------------------------------------------------------

    #[test]
    fn schema_new_is_served() {
        let env = Env::new();
        let out = env.run(1, &[], ["schema", "new"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        let v: Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(v["title"], "hali new --json");
        let listed = ok(&env.run(1, &[], ["schema", "--json"]));
        let names: Vec<&str> = listed["schemas"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_str().unwrap())
            .collect();
        assert!(names.contains(&"new"), "{names:?}");
    }

    // ---- row 16: modes follow the umask -----------------------------------------------------------------

    #[test]
    fn modes_follow_umask() {
        let env = Env::new();
        let out = env.run_umask("022", 1, ["--json", "new", "Weekly review"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&env.home().join("Documents")), 0o755);
        assert_eq!(mode(&env.docs()), 0o755);
        assert_eq!(mode(&env.docs().join("weekly-review.md")), 0o644);

        let env = Env::new();
        let out = env.run_umask("077", 1, ["--json", "new", "Weekly review"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(mode(&env.home().join("Documents")), 0o700);
        assert_eq!(mode(&env.docs()), 0o700);
        assert_eq!(mode(&env.docs().join("weekly-review.md")), 0o600);
    }

    // ---- row 19: a files.dir that cannot exist is not_a_directory in every mode ----------------------

    /// Everything under `dir`, without following symlinks, for "nothing changed" comparisons.
    fn tree(dir: &Path) -> Vec<String> {
        fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for e in entries {
                let p = e.unwrap().path();
                let rel = p.strip_prefix(base).unwrap().display().to_string();
                let meta = std::fs::symlink_metadata(&p).unwrap();
                if meta.file_type().is_symlink() {
                    out.push(format!("{rel} -> {:?}", std::fs::read_link(&p).unwrap()));
                } else if meta.is_dir() {
                    out.push(format!("{rel}/"));
                    walk(&p, base, out);
                } else {
                    out.push(format!("{rel} ({} B)", meta.len()));
                }
            }
        }
        let mut out = Vec::new();
        walk(dir, dir, &mut out);
        out.sort();
        out
    }

    #[test]
    fn a_files_dir_that_cannot_exist_is_not_a_directory_with_and_without_dry_run() {
        for case in [
            "parent_is_a_file",
            "deeper_below_a_file",
            "dangling_symlink",
        ] {
            let env = Env::new();
            let dir = match case {
                "parent_is_a_file" | "deeper_below_a_file" => {
                    let file = env.root.join("afile");
                    std::fs::write(&file, b"keep\n").unwrap();
                    if case == "parent_is_a_file" {
                        file.join("sub")
                    } else {
                        file.join("a/b")
                    }
                }
                _ => {
                    let link = env.root.join("dangling");
                    std::os::unix::fs::symlink(env.root.join("nowhere"), &link).unwrap();
                    link
                }
            };
            env.config(&format!("[files]\ndir = \"{}\"\n", dir.display()));
            let before = tree(&env.root);

            // The JSON envelope: the same error in each mode.
            let mut paths = Vec::new();
            for args in [
                vec!["--json", "new", "x"],
                vec!["--json", "--dry-run", "new", "x"],
                vec!["--json", "--dry-run", "--path-only", "new", "x"],
                vec!["--json", "--path-only", "new", "x"],
            ] {
                let out = env.run(1, &[], args.clone());
                let e = unusable(&out, "not_a_directory");
                assert!(e["details"]["path"].is_string(), "{case} {args:?}: {e}");
                paths.push(e["details"]["path"].clone());
                assert_eq!(
                    tree(&env.root),
                    before,
                    "{case} {args:?}: something changed"
                );
                env.assert_nothing_created(&format!("{case} {args:?}"));
            }
            assert!(
                paths.windows(2).all(|w| w[0] == w[1]),
                "{case}: the modes disagree on the path: {paths:?}"
            );

            // The text form: the same error with and without --dry-run.
            let mut texts = Vec::new();
            for args in [
                vec!["new", "x"],
                vec!["--dry-run", "new", "x"],
                vec!["--dry-run", "--path-only", "new", "x"],
            ] {
                let out = env.run(1, &[], args.clone());
                assert_eq!(out.code, Some(3), "{case} {args:?}: {}", out.stderr);
                assert_eq!(out.stdout, "", "{case} {args:?}");
                assert!(!out.stderr.is_empty(), "{case} {args:?}");
                texts.push(out.stderr);
            }
            assert!(
                texts.windows(2).all(|w| w[0] == w[1]),
                "{case}: the text errors differ: {texts:?}"
            );
            assert_eq!(
                tree(&env.root),
                before,
                "{case}: text runs changed something"
            );
        }
    }

    // ---- row 20: `..` after a missing component resolves against the real prefix ------------------

    #[test]
    fn dotdot_after_a_missing_component_resolves_through_symlinks() {
        // Chosen contract: the real path is recorded (`<target>/a`), never the lexical
        // `<home>/link/a`; `..` pops the missing component, then `link` is followed.
        for dry in [false, true] {
            let env = Env::new();
            let target = env.root.join("real");
            std::fs::create_dir(&target).unwrap();
            std::os::unix::fs::symlink(&target, env.home().join("link")).unwrap();
            env.config(&format!(
                "[files]\ndir = \"{}/nope/../link/a\"\n",
                env.home().display()
            ));
            let mut args = vec!["--json", "new", "Dotted"];
            if dry {
                args.insert(1, "--dry-run");
            }
            let v = ok(&env.run(1, &[], args));
            assert_eq!(v["resource"]["path"], "/TMP/real/a/dotted.md", "dry {dry}");
            assert!(!env.home().join("nope").exists(), "dry {dry}");
            if dry {
                assert!(!target.join("a").exists());
            } else {
                assert_eq!(names(&target.join("a")), ["dotted.md"]);
                let ttl = std::fs::read_to_string(env.resources_ttl()).unwrap();
                assert!(
                    !ttl.contains("/link/"),
                    "the lexical path is recorded: {ttl}"
                );
            }
        }
    }

    // ---- row 22: own dirs through symlinks; a read-only files.dir -------------------------------------

    #[test]
    fn a_symlink_to_an_own_dir_is_own_dir() {
        // files.dir is a symlink to a directory inside the data dir, and one into the state dir.
        let env = Env::with_fixture("store");
        let inside = env.data().join("files");
        std::fs::create_dir(&inside).unwrap();
        let notes = env.root.join("state/hali/notes");
        std::fs::create_dir_all(&notes).unwrap();
        let before = env.data_bytes();
        for (target, name) in [(&inside, "to-data"), (&notes, "to-state")] {
            let link = env.root.join(name);
            std::os::unix::fs::symlink(target, &link).unwrap();
            env.config(&format!("[files]\ndir = \"{}\"\n", link.display()));
            let out = env.run(1, &[], ["--json", "new", "x"]);
            unusable(&out, "own_dir");
            assert_eq!(env.data_bytes(), before, "{name}");
            assert!(names(target).is_empty(), "{name}: something was created");
        }
    }

    #[test]
    fn an_own_dir_that_is_itself_a_symlink_is_resolved() {
        // The data dir and the state dir are reached through a symlink; files.dir names the
        // real directory behind it.
        let env = Env::new();
        let real_data = env.root.join("datareal");
        std::fs::create_dir(&real_data).unwrap();
        std::os::unix::fs::symlink(&real_data, env.root.join("datalink")).unwrap();
        env.config(&format!(
            "[files]\ndir = \"{}\"\n",
            real_data.join("files").display()
        ));
        let out = env.run(
            1,
            &[("HALI_DATA_DIR", env.root.join("datalink").into())],
            ["--json", "new", "x"],
        );
        unusable(&out, "own_dir");
        assert!(names(&real_data).is_empty());

        let env = Env::new();
        let real_state = env.root.join("statereal");
        std::fs::create_dir_all(real_state.join("hali")).unwrap();
        std::os::unix::fs::symlink(&real_state, env.root.join("statelink")).unwrap();
        env.config(&format!(
            "[files]\ndir = \"{}\"\n",
            real_state.join("hali/notes").display()
        ));
        let out = env.run(
            1,
            &[("XDG_STATE_HOME", env.root.join("statelink").into())],
            ["--json", "new", "x"],
        );
        unusable(&out, "own_dir");
        assert!(names(&real_state.join("hali")).is_empty());
    }

    #[test]
    fn a_read_only_existing_files_dir_is_cannot_create_and_changes_nothing() {
        let env = Env::with_fixture("store");
        let ro = env.root.join("ro");
        std::fs::create_dir(&ro).unwrap();
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o555)).unwrap();
        // A root user can write there regardless.
        let rooted = std::fs::File::create(ro.join("probe")).is_ok();
        if !rooted {
            let before = env.data_bytes();
            env.config(&format!("[files]\ndir = \"{}\"\n", ro.display()));
            for title in ["x", "y"] {
                let out = env.run(1, &[], ["--json", "new", title]);
                let e = unusable(&out, "cannot_create");
                assert_eq!(e["details"]["path"], "/TMP/ro");
                assert_eq!(env.data_bytes(), before, "the store changed");
                assert!(names(&ro).is_empty(), "something was created");
            }
        }
        std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}
