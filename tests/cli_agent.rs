//! Goal tests for agent identity through the real binary (#13, spec §2.5, §9.6). Locked golden
//! snapshots sit in tests/snapshots/ (hand-written from the approved contract; the ids are the
//! ones `HALI_TEST_ID_SEED=1` yields: Resource, record, Person, then the SoftwareAgent):
//! `cli_dry_run__add_agent_dry_run_json` and `cli_agent__show_agent_json`.
//!
//! Assumed CLI (the approved contract): a global `--agent NAME` flag (before or after the
//! subcommand) and the `HALI_AGENT` environment variable. A usage error (an empty or blank
//! `--agent`, a control character in either, a non-UTF-8 `HALI_AGENT`) is exit 2 with nothing on
//! stdout and the store untouched, raised before the store is read, on every command except
//! `schema`. `show` keeps its schema: `catalog_record.attributed_to` is `{"id","name"}` of the
//! acting Agent, whatever its kind.
//!
//! Isolation: `HOME`, `XDG_CONFIG_HOME`, `XDG_STATE_HOME`, `HALI_DATA_DIR`, `GIT_CONFIG_GLOBAL`
//! and `GIT_CONFIG_SYSTEM` point at temp paths and the rest of the environment is cleared, so
//! `HALI_AGENT` is only set where a test sets it and the real `~/.local/state` is never touched.
//! Every run pins `HALI_TEST_NOW` and `HALI_TEST_ID_SEED` (a second run in one store uses
//! another seed so that ids never repeat). The temp dir is shown as `/TMP` in captured output.

#[cfg(feature = "goal")]
mod goal {
    use assert_cmd::Command;
    use serde_json::Value;
    use std::collections::BTreeSet;
    use std::ffi::{OsStr, OsString};
    use std::os::unix::ffi::OsStringExt;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime};
    use tempfile::TempDir;

    const R1: &str = "018bcfe5-6801-710a-adec-89025cc1beeb";
    const A: &str = "018bcfe5-6804-7099-ac6c-d7363ca585e7";
    const NOW: &str = "2026-10-24T09:30:00Z";
    const MTIME_SECS: u64 = 1_792_504_931; // 2026-10-20T14:02:11Z
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
            for dir in root.ancestors().skip(1) {
                for name in [".gitignore", ".ignore", ".haliignore", ".git", ".jj"] {
                    assert!(
                        !dir.join(name).exists(),
                        "{} exists: the host's ignore rules would reach the scan root; point TMPDIR elsewhere",
                        dir.join(name).display()
                    );
                }
            }
            for d in ["cfg", "home", "state", "files"] {
                std::fs::create_dir_all(root.join(d)).unwrap();
            }
            Env { _root: tmp, root }
        }

        /// An Env whose store holds `notes.txt` (R1), added with no agent.
        fn with_note() -> Env {
            let env = Env::new();
            let path = env.file("notes.txt", b"hello\n");
            ok(&env.run(1, &[], ["--json", "add", path.to_str().unwrap()]));
            env
        }

        fn data(&self) -> PathBuf {
            self.root.join("data")
        }

        fn files(&self) -> PathBuf {
            self.root.join("files")
        }

        fn resources_ttl(&self) -> PathBuf {
            self.data().join("resources.ttl")
        }

        fn file(&self, name: &str, contents: &[u8]) -> PathBuf {
            let path = self.files().join(name);
            std::fs::write(&path, contents).unwrap();
            let t = SystemTime::UNIX_EPOCH + Duration::new(MTIME_SECS, 0);
            std::fs::File::open(&path).unwrap().set_modified(t).unwrap();
            path
        }

        fn clean(&self, text: &str) -> String {
            text.replace(self.root.to_str().unwrap(), "/TMP")
        }

        /// One run with the standard environment plus `envs` (set after the clean slate).
        fn run<I, S>(&self, seed: u64, envs: &[(&str, OsString)], args: I) -> Out
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            let mut cmd = Command::cargo_bin("hali").unwrap();
            cmd.env_clear()
                .env("XDG_CONFIG_HOME", self.root.join("cfg"))
                .env("XDG_STATE_HOME", self.root.join("state"))
                .env("HOME", self.root.join("home"))
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
            cmd.args(args);
            let out = cmd.output().unwrap();
            Out {
                code: out.status.code(),
                stdout: self.clean(&String::from_utf8(out.stdout).unwrap()),
                stderr: self.clean(&String::from_utf8_lossy(&out.stderr)),
            }
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

        /// resources.ttl with the temp dir shown as `/TMP` (None when absent).
        fn ttl_clean(&self) -> Option<String> {
            std::fs::read(self.resources_ttl())
                .ok()
                .map(|b| self.clean(&String::from_utf8(b).unwrap()))
        }

        fn count_typed(&self, class: &str) -> usize {
            self.triples()
                .iter()
                .filter(|t| t.contains(&format!("<{RDF_TYPE}> <{PROV}{class}> .")))
                .count()
        }

        /// `attributed_to` of the record of the Resource at `path` or with the id `reference`.
        fn attributed_to(&self, reference: &str) -> Value {
            let v = ok(&self.run(1, &[], ["--json", "show", reference]));
            v["resource"]["catalog_record"]["attributed_to"].clone()
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

    fn env_of(value: &str) -> Vec<(&'static str, OsString)> {
        vec![("HALI_AGENT", OsString::from(value))]
    }

    fn s(path: &Path) -> String {
        path.to_str().unwrap().to_string()
    }

    // ---- row 1: the record is attributed to a SoftwareAgent named claude ----------------------

    #[test]
    fn add_agent_dry_run_json() {
        let env = Env::new();
        let path = env.file("notes.txt", b"hello\n");
        let out = env.run(
            1,
            &[],
            ["--json", "--dry-run", "--agent", "claude", "add", &s(&path)],
        );
        snapshot_stdout("cli_dry_run__add_agent_dry_run_json", &out);
        assert!(!env.data().exists(), "a dry run writes nothing");
    }

    #[test]
    fn show_agent_json() {
        let env = Env::new();
        let path = env.file("notes.txt", b"hello\n");
        ok(&env.run(1, &[], ["--json", "add", "--agent", "claude", &s(&path)]));
        let out = env.run(1, &[], ["--json", "show", &s(&path)]);
        snapshot_stdout("cli_agent__show_agent_json", &out);

        // The graph: the record is attributed to the SoftwareAgent, which acts for the Person.
        let triples = env.triples();
        let want = [
            format!("<urn:uuid:{A}> <{RDF_TYPE}> <{PROV}SoftwareAgent> ."),
            format!("<urn:uuid:{A}> <http://xmlns.com/foaf/0.1/name> \"claude\" ."),
            format!(
                "<urn:uuid:{A}> <{PROV}actedOnBehalfOf> <urn:uuid:018bcfe5-6803-71bb-94d8-d101b5b9c34d> ."
            ),
            format!(
                "<urn:uuid:018bcfe5-6802-7893-a2ee-fb32555e71c1> <{PROV}wasAttributedTo> <urn:uuid:{A}> ."
            ),
        ];
        for t in want {
            assert!(triples.contains(&t), "missing: {t}");
        }
    }

    // ---- row 2: HALI_AGENT, and --agent wins ---------------------------------------------------

    #[test]
    fn env_used() {
        let env = Env::new();
        let path = env.file("notes.txt", b"hello\n");
        ok(&env.run(1, &env_of("codex"), ["--json", "add", &s(&path)]));
        let who = env.attributed_to(&s(&path));
        assert_eq!(who["name"], "codex");
        assert_eq!(who["id"], A);
        assert_eq!(env.count_typed("SoftwareAgent"), 1);

        // An empty or whitespace-only HALI_AGENT counts as unset.
        for blank in ["", "   "] {
            let env = Env::new();
            let path = env.file("notes.txt", b"hello\n");
            ok(&env.run(1, &env_of(blank), ["--json", "add", &s(&path)]));
            assert_eq!(env.attributed_to(&s(&path))["name"], "user");
            assert_eq!(env.count_typed("SoftwareAgent"), 0);
        }
    }

    #[test]
    fn flag_overrides_env() {
        let env = Env::new();
        let path = env.file("notes.txt", b"hello\n");
        ok(&env.run(
            1,
            &env_of("codex"),
            ["--json", "--agent", "claude", "add", &s(&path)],
        ));
        assert_eq!(env.attributed_to(&s(&path))["name"], "claude");
        assert_eq!(env.count_typed("SoftwareAgent"), 1);
        let ttl = env.ttl_clean().unwrap();
        assert!(
            !ttl.contains("codex"),
            "the overridden env name is not recorded"
        );
    }

    // ---- row 3: neither set, the user is the Agent (regression pin) -------------------------------

    #[test]
    fn no_agent_attributes_user() {
        // add
        let env = Env::new();
        let path = env.file("notes.txt", b"hello\n");
        ok(&env.run(1, &[], ["--json", "add", &s(&path)]));
        let who = env.attributed_to(&s(&path));
        assert_eq!(who["name"], "user");
        assert_eq!(who["id"], "018bcfe5-6803-71bb-94d8-d101b5b9c34d");
        assert_eq!(env.count_typed("SoftwareAgent"), 0);
        assert_eq!(env.count_typed("Person"), 1);

        // scan
        let env = Env::new();
        env.file("a.txt", b"alpha\n");
        env.file("b.txt", b"beta\n");
        ok(&env.run(1, &[], ["--json", "scan", &s(&env.files())]));
        for name in ["a.txt", "b.txt"] {
            let p = s(&env.files().join(name));
            assert_eq!(env.attributed_to(&p)["name"], "user");
        }
        assert_eq!(env.count_typed("SoftwareAgent"), 0);
        assert_eq!(env.count_typed("Person"), 1);
    }

    // ---- row 4: the same name reuses the Agent ---------------------------------------------------

    #[test]
    fn reuse_across_runs() {
        let env = Env::new();
        let a = env.file("a.txt", b"alpha\n");
        let b = env.file("b.txt", b"beta\n");
        ok(&env.run(1, &[], ["--json", "--agent", "claude", "add", &s(&a)]));
        let before = env.triples();
        ok(&env.run(2, &[], ["--json", "--agent", "  claude ", "add", &s(&b)]));
        let first = env.attributed_to(&s(&a));
        let second = env.attributed_to(&s(&b));
        assert_eq!(first["id"], A);
        assert_eq!(second["id"], A, "the same Agent");
        assert_eq!(second["name"], "claude");
        assert_eq!(env.count_typed("SoftwareAgent"), 1);
        assert_eq!(env.count_typed("Person"), 1);
        // The first record's triples are untouched.
        let after = env.triples();
        assert!(before.is_subset(&after));

        // The same through HALI_AGENT, with spaces around the name.
        let c = env.file("c.txt", b"gamma\n");
        ok(&env.run(3, &env_of(" claude "), ["--json", "add", &s(&c)]));
        assert_eq!(env.attributed_to(&s(&c))["id"], A);
        assert_eq!(env.count_typed("SoftwareAgent"), 1);
    }

    // ---- row 6: scan attributes newly cataloged Resources -----------------------------------------

    #[test]
    fn scan_attributes_agent() {
        let env = Env::new();
        env.file("a.txt", b"alpha\n");
        env.file("b.txt", b"beta\n");
        ok(&env.run(
            1,
            &[],
            ["--json", "--agent", "codex", "scan", &s(&env.files())],
        ));
        let a = env.attributed_to(&s(&env.files().join("a.txt")));
        let b = env.attributed_to(&s(&env.files().join("b.txt")));
        assert_eq!(a["name"], "codex");
        assert_eq!(a["id"], b["id"], "one Agent for the whole scan");
        assert_eq!(env.count_typed("SoftwareAgent"), 1);
        assert_eq!(env.count_typed("Person"), 1);

        // The flag after the subcommand works too; a later scan without an agent attributes
        // its new file to the user and leaves the earlier records alone.
        env.file("c.txt", b"gamma\n");
        ok(&env.run(2, &[], ["--json", "scan", &s(&env.files())]));
        assert_eq!(
            env.attributed_to(&s(&env.files().join("c.txt")))["name"],
            "user"
        );
        assert_eq!(
            env.attributed_to(&s(&env.files().join("a.txt")))["name"],
            "codex"
        );
        env.file("d.txt", b"delta\n");
        ok(&env.run(
            3,
            &[],
            ["--json", "scan", &s(&env.files()), "--agent", "codex"],
        ));
        assert_eq!(
            env.attributed_to(&s(&env.files().join("d.txt")))["id"],
            a["id"]
        );
        assert_eq!(env.count_typed("SoftwareAgent"), 1);
    }

    // ---- row 7: the dry run reports what the real run writes --------------------------------------

    fn changes(out: &Out) -> BTreeSet<String> {
        let v = ok(out);
        assert_eq!(v["dry_run"], true);
        v["changes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                assert_eq!(c["op"], "add");
                c["triple"].as_str().unwrap().to_string()
            })
            .collect()
    }

    #[test]
    fn dry_run_matches_real() {
        // add
        let env = Env::new();
        let path = env.file("notes.txt", b"hello\n");
        let dry = env.run(
            1,
            &[],
            ["--json", "--dry-run", "--agent", "claude", "add", &s(&path)],
        );
        let dry = changes(&dry);
        assert!(!env.data().exists(), "a dry run writes nothing");
        assert!(dry.iter().any(|t| t.contains("SoftwareAgent")));
        ok(&env.run(1, &[], ["--json", "--agent", "claude", "add", &s(&path)]));
        let real: BTreeSet<String> = env.triples().iter().map(|t| env.clean(t)).collect();
        assert_eq!(dry, real);

        // scan
        let env = Env::new();
        env.file("a.txt", b"alpha\n");
        env.file("b.txt", b"beta\n");
        let dry = env.run(
            1,
            &[],
            [
                "--json",
                "--dry-run",
                "--agent",
                "codex",
                "scan",
                &s(&env.files()),
            ],
        );
        let dry = changes(&dry);
        assert!(!env.data().exists(), "a dry run writes nothing");
        assert_eq!(
            dry.iter().filter(|t| t.contains("SoftwareAgent")).count(),
            1
        );
        ok(&env.run(
            1,
            &[],
            ["--json", "--agent", "codex", "scan", &s(&env.files())],
        ));
        let real: BTreeSet<String> = env.triples().iter().map(|t| env.clean(t)).collect();
        assert_eq!(dry, real);
    }

    // ---- row 8: other commands accept --agent and ignore it -----------------------------------------

    /// Runs `args_for(env)` in two identical stores, with and without `--agent claude`, and
    /// asserts the same output and the same store: nothing minted, nothing re-attributed.
    fn ignores_agent(name: &str, args_for: impl Fn(&Env) -> Vec<String>, change_file: bool) {
        let run_one = |agent: bool| {
            let env = Env::with_note();
            if change_file {
                env.file("notes.txt", b"hello world\n");
            }
            let mut args = vec!["--json".to_string()];
            if agent {
                args.extend(["--agent".to_string(), "claude".to_string()]);
            }
            args.extend(args_for(&env));
            let out = env.run(2, &[], args);
            (out, env.ttl_clean(), env.count_typed("SoftwareAgent"), env)
        };
        let (plain, plain_ttl, _, _) = run_one(false);
        let (with, with_ttl, agents, env) = run_one(true);
        assert_eq!(plain.code, Some(0), "{name}: {}", plain.stderr);
        assert_eq!(with.code, plain.code, "{name}: {}", with.stderr);
        assert_eq!(with.stdout, plain.stdout, "{name}");
        assert_eq!(with.stderr, plain.stderr, "{name}");
        assert_eq!(with_ttl, plain_ttl, "{name}: the store differs");
        assert_eq!(agents, 0, "{name}: an Agent was minted");
        if name != "forget" {
            assert_eq!(env.attributed_to(R1)["name"], "user", "{name}");
        }
    }

    #[test]
    fn other_commands_ignore_agent() {
        ignores_agent(
            "edit",
            |_| {
                ["edit", R1, "--title", "Renamed"]
                    .map(String::from)
                    .to_vec()
            },
            false,
        );
        ignores_agent(
            "refresh",
            |_| ["refresh", R1].map(String::from).to_vec(),
            true,
        );
        ignores_agent(
            "relink",
            |env| vec!["relink".into(), "--root".into(), s(&env.files())],
            false,
        );
        ignores_agent(
            "forget",
            |_| ["forget", R1, "--yes"].map(String::from).to_vec(),
            false,
        );
        // Read-only commands: identical output, and they never write.
        for (name, args) in [
            ("show", vec!["show", R1]),
            ("find", vec!["find"]),
            ("describe", vec!["describe"]),
            ("type list", vec!["type", "list"]),
            ("concept list", vec!["concept", "list"]),
            ("schema", vec!["schema"]),
        ] {
            ignores_agent(
                name,
                |_| args.iter().map(|a| a.to_string()).collect(),
                false,
            );
        }
    }

    // ---- row 9: invalid names are usage errors, before the store is read --------------------------

    fn usage_error(env: &Env, envs: &[(&str, OsString)], args: &[&str], what: &str) {
        let before = std::fs::read(env.resources_ttl()).ok();
        let out = env.run(5, envs, args.iter().copied());
        assert_eq!(out.code, Some(2), "{what}: stderr {}", out.stderr);
        assert_eq!(out.stdout, "", "{what}");
        assert!(!out.stderr.is_empty(), "{what}: the error is explained");
        assert_eq!(
            std::fs::read(env.resources_ttl()).ok(),
            before,
            "{what}: the store changed"
        );
        if before.is_none() {
            assert!(!env.data().exists(), "{what}: the data dir was created");
        }
    }

    #[test]
    fn invalid_names_usage() {
        let notes = |env: &Env| s(&env.files().join("notes.txt"));

        // Invalid --agent values, on a fresh store and on an existing one.
        for (label, bad) in [
            ("empty", ""),
            ("blank", "   "),
            ("newline", "a\nb"),
            ("escape", "a\u{1b}[31mb"),
            ("tab", "a\tb"),
            ("del", "a\u{7f}b"),
        ] {
            let env = Env::new();
            let p = s(&env.file("notes.txt", b"hello\n"));
            usage_error(
                &env,
                &[],
                &["--json", "--agent", bad, "add", &p],
                &format!("--agent {label}"),
            );
            let env = Env::with_note();
            usage_error(
                &env,
                &[],
                &["--json", "--agent", bad, "edit", R1, "--title", "X"],
                &format!("--agent {label} on edit"),
            );
        }
        // `--agent ""` is a usage error even when HALI_AGENT is valid: it is not a reset.
        let env = Env::new();
        let p = s(&env.file("notes.txt", b"hello\n"));
        usage_error(
            &env,
            &env_of("codex"),
            &["--json", "--agent", "", "add", &p],
            "--agent empty over a valid env",
        );

        // An invalid HALI_AGENT fails every command but `schema`, read-only ones included.
        let bad_envs: Vec<(&str, OsString)> = vec![
            ("newline", OsString::from("a\nb")),
            ("escape", OsString::from("a\u{1b}[31mb")),
            ("non-utf8", OsString::from_vec(vec![b'a', 0xff, b'b'])),
        ];
        for (label, bad) in &bad_envs {
            let envs = vec![("HALI_AGENT", bad.clone())];
            let env = Env::with_note();
            let p = notes(&env);
            for args in [
                vec!["--json", "add", &p],
                vec!["--json", "show", R1],
                vec!["--json", "find"],
                vec!["--json", "describe"],
                vec!["--json", "validate"],
                vec!["--json", "type", "list"],
                vec!["--json", "forget", R1, "--yes"],
            ] {
                usage_error(&env, &envs, &args, &format!("HALI_AGENT {label}: {args:?}"));
            }
            // `schema` does not read the identity.
            let out = env.run(5, &envs, ["--json", "schema"]);
            assert_eq!(out.code, Some(0), "HALI_AGENT {label}: {}", out.stderr);
            // The store (and the fresh case) stay untouched too.
            let fresh = Env::new();
            let p = s(&fresh.file("notes.txt", b"hello\n"));
            usage_error(
                &fresh,
                &envs,
                &["--json", "add", &p],
                &format!("HALI_AGENT {label} on a fresh store"),
            );
        }
    }
}
