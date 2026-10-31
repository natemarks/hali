//! Goal tests for `hali refresh` through the real binary (#20, spec §6, §9.1 **Refresh**,
//! §9.4, §9.5, §9.8, §14.1). Locked golden snapshots sit in tests/snapshots/ (hand-written from
//! the issue's contract and the fixture's known hashes and times): `cli_refresh__refresh_json`,
//! `cli_refresh__refresh_all_json` and `cli_dry_run__refresh_dry_run_json` (stdout).
//!
//! Every run sets HALI_DATA_DIR / XDG_CONFIG_HOME / HOME to temp dirs and clears the rest of the
//! environment, so the real store and config are never read. The store is
//! tests/fixtures/refresh with its `file:///fixtures/refresh/` URLs pointed at files under the
//! temp root (the `cli_forget.rs` pattern); the temp root is canonicalised and shown as `/TMP`
//! in captured output. `refresh` mints no id, so one HALI_TEST_ID_SEED serves every run.
//!
//! The files the tests write, against the records of the fixture (every Resource is recorded as
//! "hello\n": 6 bytes, sha256 5891b5b5..., modified 2026-10-20T14:02:11Z):
//!   R1 same.txt     "hello\n", mtime as recorded              unchanged
//!   R2 edited.txt   "hello world\n" (12 bytes), mtime 2026-10-22T10:00:00Z   Stale
//!   R3 touched.txt  "hello\n", mtime 2026-10-22T10:00:00Z     Stale by mtime only
//!   R4 gone.txt     not created                               Missing
//!   R5 sneaky.txt   "HELLO\n", mtime as recorded              not Stale, different content
//!
//! Wording the contract leaves open and these tests fix (change them, with the human, if the
//! wording should differ):
//! - under `--dry-run` in text mode the lines come first (`would refresh resource ...`,
//!   `missing resource ...`, `unchanged <n>`), then the `+ `/`- ` change lines, then the note on
//!   stderr, as every other mutating command prints them;
//! - `refresh` of a store whose files are all fine and unchanged prints `unchanged <n>` alone.
//!
//! The `hali schema` list is covered here too (tests/cli_schema.rs and tests/core_schema.rs are
//! not goal-owned): the implementer must also update `NAMES` and the text list in
//! tests/cli_schema.rs, the name list of tests/core_schema.rs, the hardcoded 23 in
//! tests/cli_relation.rs and tests/cli_type.rs, the snapshot
//! tests/snapshots/cli_schema__schema_list_json.snap and the `refresh` row of
//! tests/dry_run_guard.rs; the test `schema_lists_refresh` below covers the new list exactly.

#[cfg(feature = "goal")]
mod support;

#[cfg(feature = "goal")]
mod goal {
    use assert_cmd::Command;
    use serde_json::{Value, json};
    use std::collections::BTreeMap;
    use std::ffi::OsStr;
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime};
    use tempfile::TempDir;

    use super::support;

    const R1: &str = "0192f0c4-0000-7000-8000-000000000901";
    const R2: &str = "0192f0c4-0000-7000-8000-000000000902";
    const R3: &str = "0192f0c4-0000-7000-8000-000000000903";
    const R4: &str = "0192f0c4-0000-7000-8000-000000000904";
    const R5: &str = "0192f0c4-0000-7000-8000-000000000905";
    const NOW: &str = "2026-10-24T09:30:00Z";
    const RECORDED_SECS: u64 = 1_792_504_931; // 2026-10-20T14:02:11Z
    const NEW_SECS: u64 = 1_792_663_200; // 2026-10-22T10:00:00Z
    const SHA_HELLO: &str = "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03";
    const SHA_WORLD: &str = "a948904f2f0f479b8f8197694b30184b0d2ed1c1cd2a1ec0fb85d299a192a447";
    const SHA_UPPER: &str = "3b09aeb6f5f5336beb205d7f720371bc927cd46c21922e334d47ba264acb5ba4";
    const EMPTY: &str = r#"{"schema_version":1,"refreshed":[],"missing":[],"unchanged":0}"#;

    struct Env {
        _root: TempDir,
        root: PathBuf,
    }

    struct Out {
        code: Option<i32>,
        stdout: String,
        stderr: String,
    }

    type Fingerprint = BTreeMap<String, (Vec<u8>, SystemTime)>;

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::new(secs, 987_654_321)
    }

    fn write_file(path: &Path, bytes: &[u8], secs: u64) {
        std::fs::write(path, bytes).unwrap();
        std::fs::File::open(path)
            .unwrap()
            .set_modified(at(secs))
            .unwrap();
    }

    impl Env {
        fn base() -> Env {
            let tmp = tempfile::tempdir().unwrap();
            let root = std::fs::canonicalize(tmp.path()).unwrap();
            for d in ["cfg", "home", "files"] {
                std::fs::create_dir_all(root.join(d)).unwrap();
            }
            Env { _root: tmp, root }
        }

        /// The `refresh` fixture and its files (see the module comment).
        fn new() -> Env {
            let env = Env::base();
            std::fs::create_dir_all(env.data()).unwrap();
            let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/refresh");
            for entry in std::fs::read_dir(&fixture).unwrap() {
                let entry = entry.unwrap();
                let text = std::fs::read_to_string(entry.path()).unwrap().replace(
                    "file:///fixtures/refresh/",
                    &format!("file://{}/files/", env.root.display()),
                );
                std::fs::write(env.data().join(entry.file_name()), text).unwrap();
            }
            let files = env.root.join("files");
            write_file(&files.join("same.txt"), b"hello\n", RECORDED_SECS);
            write_file(&files.join("edited.txt"), b"hello world\n", NEW_SECS);
            write_file(&files.join("touched.txt"), b"hello\n", NEW_SECS);
            write_file(&files.join("sneaky.txt"), b"HELLO\n", RECORDED_SECS);
            env
        }

        /// An empty data dir.
        fn empty() -> Env {
            let env = Env::base();
            std::fs::create_dir_all(env.data()).unwrap();
            env
        }

        /// No data dir at all.
        fn absent() -> Env {
            Env::base()
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

        /// No terminal: stdin piped and empty.
        fn run<I, S>(&self, args: I) -> Out
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            self.exec(&[], b"", args)
        }

        /// Bytes and mtime of every file in the data dir.
        fn state(&self) -> Fingerprint {
            std::fs::read_dir(self.data())
                .unwrap()
                .map(|e| {
                    let e = e.unwrap();
                    (
                        e.file_name().into_string().unwrap(),
                        (
                            std::fs::read(e.path()).unwrap(),
                            e.metadata().unwrap().modified().unwrap(),
                        ),
                    )
                })
                .collect()
        }

        /// Bytes, mtime and mode of every entry under files/ (a symlink is not followed).
        fn files_state(&self) -> BTreeMap<String, (Vec<u8>, SystemTime, u32)> {
            std::fs::read_dir(self.root.join("files"))
                .unwrap()
                .map(|e| {
                    let e = e.unwrap();
                    let meta = std::fs::symlink_metadata(e.path()).unwrap();
                    let bytes = if meta.is_file() {
                        std::fs::read(e.path()).unwrap()
                    } else {
                        Vec::new()
                    };
                    (
                        e.file_name().into_string().unwrap(),
                        (bytes, meta.modified().unwrap(), meta.permissions().mode()),
                    )
                })
                .collect()
        }

        fn resources_ttl(&self) -> String {
            std::fs::read_to_string(self.data().join("resources.ttl")).unwrap()
        }

        /// Ages resources.ttl so a rewrite would show in its mtime.
        fn age_store(&self) {
            std::fs::File::open(self.data().join("resources.ttl"))
                .unwrap()
                .set_modified(at(RECORDED_SECS))
                .unwrap();
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

    fn ids(list: &Value) -> Vec<&str> {
        list.as_array()
            .unwrap()
            .iter()
            .map(|r| r["id"].as_str().unwrap())
            .collect()
    }

    /// Sets mode 000; false (with a note) when this user can still read the file (root).
    fn made_unreadable(path: &Path) -> bool {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::File::open(path).is_ok() {
            eprintln!("skipped: running as a user that can read a mode 000 file (root)");
            return false;
        }
        true
    }

    // ---- criterion 1: exactly one of refs and --all ---------------------------------------------

    #[test]
    fn needs_refs_or_all() {
        // On an unparseable store: a usage error (2) raised before the store is read, not a
        // store error (3).
        let env = Env::base();
        std::fs::create_dir_all(env.data()).unwrap();
        std::fs::write(env.data().join("resources.ttl"), "this is not turtle {{{\n").unwrap();
        for args in [&["refresh"][..], &["--json", "refresh"][..]] {
            let out = env.run(args);
            assert_eq!(out.code, Some(2), "{args:?}: {}", out.stderr);
            assert_eq!(out.stdout, "");
            assert!(out.stderr.contains("--all"), "{}", out.stderr);
        }
        // On an absent store: no data dir appears.
        let env = Env::absent();
        let out = env.run(["refresh"]);
        assert_eq!(out.code, Some(2), "{}", out.stderr);
        assert!(out.stderr.contains("--all"), "{}", out.stderr);
        assert!(!env.data().exists());
    }

    #[test]
    fn refs_and_all_conflict() {
        let env = Env::base();
        std::fs::create_dir_all(env.data()).unwrap();
        std::fs::write(env.data().join("resources.ttl"), "this is not turtle {{{\n").unwrap();
        for args in [
            vec!["refresh", "--all", R1],
            vec!["refresh", R1, "--all"],
            vec!["--json", "refresh", "--all", "files/same.txt"],
        ] {
            let out = env.run(&args);
            assert_eq!(out.code, Some(2), "{args:?}: {}", out.stderr);
            assert_eq!(out.stdout, "");
            assert!(out.stderr.contains("--all"), "{}", out.stderr);
        }
        let env = Env::absent();
        assert_eq!(env.run(["refresh", "--all", R1]).code, Some(2));
        assert!(!env.data().exists());
    }

    // ---- criteria 2 and 5 (§14.1): golden snapshots --------------------------------------------

    #[test]
    fn refresh_json() {
        let env = Env::new();
        let files_before = env.files_state();
        let out = env.run(["--json", "refresh", env.file("edited.txt").as_str()]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!("cli_refresh__refresh_json", out.stdout);
        });
        // The catalogued file is never written.
        assert_eq!(env.files_state(), files_before);
        // Only resources.ttl changed.
        let ttl = env.resources_ttl();
        assert!(ttl.contains(SHA_WORLD));
        assert_eq!(
            ttl.matches(SHA_HELLO).count(),
            4,
            "R1, R3, R4 and R5 keep theirs"
        );
        assert!(ttl.contains("\"2026-10-22T10:00:00Z\"^^xsd:dateTime"));
        assert!(ttl.contains("dcat:byteSize 12"));
        assert!(ttl.contains("\"2026-10-24T09:30:00Z\"^^xsd:dateTime"));
    }

    #[test]
    fn refresh_all_json() {
        let env = Env::new();
        let out = env.run(["--json", "refresh", "--all"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!("cli_refresh__refresh_all_json", out.stdout);
        });
        let v = json(&out.stdout);
        let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
        assert_eq!(
            keys,
            ["schema_version", "refreshed", "missing", "unchanged"]
        );
        assert_eq!(ids(&v["refreshed"]), [R2, R3]);
        assert_eq!(ids(&v["missing"]), [R4]);
        assert_eq!(v["unchanged"], 2);
    }

    // ---- criterion 2: a refreshed Resource is no longer stale ----------------------------------

    #[test]
    fn show_after_refresh_not_stale() {
        let env = Env::new();
        let shown = ok(&env.run(["--json", "show", R2]));
        assert_eq!(shown["resource"]["stale"], true);
        assert_eq!(shown["resource"]["byte_size"], 6);
        ok(&env.run(["--json", "refresh", R2]));

        let shown = ok(&env.run(["--json", "show", R2]));
        assert_eq!(shown["resource"]["stale"], false);
        assert_eq!(shown["resource"]["missing"], false);
        assert_eq!(shown["resource"]["byte_size"], 12);
        assert_eq!(shown["resource"]["modified"], "2026-10-22T10:00:00Z");
        assert_eq!(shown["resource"]["catalog_record"]["modified"], NOW);
        assert_eq!(
            shown["resource"]["catalog_record"]["issued"],
            "2026-10-21T08:00:00Z"
        );
        assert_eq!(shown["resource"]["title"], "edited");
        assert_eq!(shown["resource"]["media_type"], "text/plain");

        // The same by path; and only the touched file is still Stale.
        let by_path = ok(&env.run(["--json", "show", env.file("edited.txt").as_str()]));
        assert_eq!(by_path["resource"]["stale"], false);
        let stale = ok(&env.run(["--json", "find", "--stale"]));
        assert_eq!(ids(&stale["results"]), [R3]);
    }

    // ---- criterion 4: unchanged is a no-op -------------------------------------------------------

    #[test]
    fn unchanged_writes_nothing() {
        let env = Env::new();
        env.age_store();
        let before = env.state();
        let files_before = env.files_state();

        // Named and unchanged: hashed, found equal, nothing written.
        let v = ok(&env.run(["--json", "refresh", env.file("same.txt").as_str()]));
        assert_eq!(
            v,
            json!({"schema_version": 1, "refreshed": [], "missing": [], "unchanged": 1})
        );
        assert_eq!(env.state(), before, "resources.ttl not rewritten");
        assert_eq!(env.files_state(), files_before);

        // Missing and unchanged-only: also nothing written.
        let v = ok(&env.run(["--json", "refresh", R1, R4]));
        assert_eq!(ids(&v["missing"]), [R4]);
        assert_eq!(v["unchanged"], 1);
        assert_eq!(env.state(), before);

        // `--all` where nothing is Stale: nothing written either.
        let env = Env::new();
        std::fs::remove_file(env.root.join("files/edited.txt")).unwrap();
        write_file(
            &env.root.join("files/touched.txt"),
            b"hello\n",
            RECORDED_SECS,
        );
        env.age_store();
        let before = env.state();
        let v = ok(&env.run(["--json", "refresh", "--all"]));
        assert_eq!(ids(&v["refreshed"]), Vec::<&str>::new());
        assert_eq!(ids(&v["missing"]), [R2, R4]);
        assert_eq!(v["unchanged"], 3);
        assert_eq!(env.state(), before);
    }

    #[test]
    fn refresh_text() {
        let env = Env::new();
        let out = env.run(["refresh", env.file("same.txt").as_str()]);
        assert_eq!(
            (out.code, out.stdout.as_str(), out.stderr.as_str()),
            (Some(0), "unchanged 1\n", "")
        );

        let out = env.run(["refresh", "--all"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        assert_eq!(
            out.stdout,
            format!(
                "refreshed resource {R2} \"edited\" /TMP/files/edited.txt\n\
                 refreshed resource {R3} \"touched\" /TMP/files/touched.txt\n\
                 missing resource {R4} \"gone\" /TMP/files/gone.txt\n\
                 unchanged 2\n"
            )
        );

        // Nothing left to refresh: the count alone.
        let out = env.run(["refresh", "--all"]);
        assert_eq!(
            out.stdout,
            format!("missing resource {R4} \"gone\" /TMP/files/gone.txt\nunchanged 4\n")
        );
    }

    // ---- criterion 5: which Resources are hashed -----------------------------------------------

    #[test]
    fn sneaky_needs_explicit_ref() {
        // Same size, same mtime, different content: not Stale, so `--all` never reads it.
        let env = Env::new();
        let v = ok(&env.run(["--json", "refresh", "--all"]));
        assert_eq!(ids(&v["refreshed"]), [R2, R3]);
        let ttl = env.resources_ttl();
        assert!(
            !ttl.contains(SHA_UPPER),
            "--all must not hash a non-Stale file"
        );
        assert!(ttl.contains(&format!("<urn:uuid:{R5}#sha256>")));

        // Named, it is hashed and the new checksum is recorded.
        let v = ok(&env.run(["--json", "refresh", env.file("sneaky.txt").as_str()]));
        assert_eq!(ids(&v["refreshed"]), [R5]);
        assert_eq!(v["refreshed"][0]["byte_size"], 6);
        assert_eq!(v["refreshed"][0]["modified"], "2026-10-20T14:02:11Z");
        assert_eq!(v["refreshed"][0]["stale"], false);
        assert_eq!(v["unchanged"], 0);
        assert!(env.resources_ttl().contains(SHA_UPPER));
    }

    // ---- criterion 7: missing, directory, FIFO ------------------------------------------------------

    #[test]
    fn missing_named_ref_exit_0() {
        let env = Env::new();
        env.age_store();
        let before = env.state();
        for target in [env.file("gone.txt"), R4.to_string()] {
            let v = ok(&env.run(["--json", "refresh", target.as_str()]));
            assert_eq!(v["refreshed"], json!([]));
            assert_eq!(ids(&v["missing"]), [R4]);
            assert_eq!(v["missing"][0]["missing"], true);
            assert_eq!(v["missing"][0]["stale"], false);
            assert_eq!(v["missing"][0]["path"], "/TMP/files/gone.txt");
            assert_eq!(v["unchanged"], 0);
            assert_eq!(env.state(), before, "nothing written");
        }
        let out = env.run(["refresh", R4]);
        assert_eq!(
            out.stdout,
            format!("missing resource {R4} \"gone\" /TMP/files/gone.txt\nunchanged 0\n")
        );
    }

    #[test]
    fn fifo_is_missing_not_opened() {
        let env = Env::new();
        let fifo = env.file("gone.txt");
        let status = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap();
        assert!(status.success());
        let before = env.state();
        // Opening a FIFO with no writer would block; the run has a 30 second timeout.
        let v = ok(&env.run(["--json", "refresh", fifo.as_str()]));
        assert_eq!(ids(&v["missing"]), [R4]);
        assert_eq!(v["refreshed"], json!([]));
        let v = ok(&env.run(["--json", "refresh", "--all"]));
        assert_eq!(ids(&v["missing"]), [R4]);
        assert_eq!(ids(&v["refreshed"]), [R2, R3]);
        assert_ne!(env.state(), before, "R2 and R3 were refreshed");
    }

    #[test]
    fn directory_is_missing() {
        let env = Env::new();
        std::fs::create_dir(env.root.join("files/gone.txt")).unwrap();
        for args in [
            vec!["--json", "refresh", R4],
            vec!["--json", "refresh", "--all"],
        ] {
            let v = ok(&env.run(&args));
            assert_eq!(ids(&v["missing"]), [R4], "{args:?}");
            assert_eq!(v["missing"][0]["missing"], true);
        }
        // The record is untouched by it.
        assert!(env.resources_ttl().contains("gone.txt"));
    }

    // ---- criterion 8: errors abort, nothing is written ------------------------------------------

    #[test]
    fn unknown_resource() {
        let env = Env::new();
        let before = env.state();
        let nope = env.file("nope.txt");
        let unknown_uuid = "0192f0c4-0000-7000-8000-000000000999";
        for (args, bad) in [
            (
                vec!["--json", "refresh", nope.as_str()],
                "/TMP/files/nope.txt",
            ),
            // A valid ref first: it is not refreshed when a later one is unknown.
            (vec!["--json", "refresh", R2, unknown_uuid], unknown_uuid),
            // The first unknown one is reported.
            (
                vec!["--json", "refresh", unknown_uuid, nope.as_str()],
                unknown_uuid,
            ),
        ] {
            let out = env.run(&args);
            let e = failed(&out, 1, "UNKNOWN_RESOURCE");
            assert_eq!(e["error"]["details"]["ref"], bad, "{args:?}");
            assert_eq!(env.state(), before, "{args:?}: nothing written");
        }
    }

    #[test]
    fn file_unreadable_all_or_nothing() {
        let env = Env::new();
        // R3 (touched.txt) cannot be read; R2, processed first, can.
        let touched = env.root.join("files/touched.txt");
        if !made_unreadable(&touched) {
            return;
        }
        let before = env.state();
        for args in [
            vec!["--json", "refresh", "--all"],
            vec!["--json", "refresh", R2, R3],
        ] {
            let out = env.run(&args);
            let e = failed(&out, 1, "FILE_UNREADABLE");
            assert_eq!(e["error"]["details"]["path"], "/TMP/files/touched.txt");
            assert_eq!(env.state(), before, "{args:?}: R2's change was not kept");
        }
        // Text mode: the usual error line.
        let out = env.run(["refresh", "--all"]);
        assert_eq!(out.code, Some(1));
        assert_eq!(out.stdout, "");
        assert!(out.stderr.starts_with("error: "), "{}", out.stderr);
        assert!(out.stderr.contains("hint: "), "{}", out.stderr);
    }

    #[test]
    fn symlink_at_path_refused() {
        // `stat` follows a symlink and sees a regular file of another size; hashing must not.
        let env = Env::new();
        let target = env.root.join("target.bin");
        write_file(&target, b"hello world\n", NEW_SECS);
        let edited = env.root.join("files/edited.txt");
        std::fs::remove_file(&edited).unwrap();
        symlink(&target, &edited).unwrap();
        let before = env.state();
        for args in [
            vec!["--json", "refresh", "--all"],
            vec!["--json", "refresh", R2],
        ] {
            let out = env.run(&args);
            let e = failed(&out, 1, "NOT_A_REGULAR_FILE");
            assert_eq!(e["error"]["details"]["kind"], "other", "{args:?}");
            assert_eq!(e["error"]["details"]["path"], "/TMP/files/edited.txt");
            assert_eq!(env.state(), before, "{args:?}: nothing written");
        }
    }

    // ---- criterion 9: a Resource named twice counts once -----------------------------------------

    #[test]
    fn duplicate_refs_once() {
        let once = {
            let env = Env::new();
            env.run(["--json", "refresh", env.file("edited.txt").as_str()])
        };
        let env = Env::new();
        let edited = env.file("edited.txt");
        let out = env.run(["--json", "refresh", edited.as_str(), R2, edited.as_str()]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stdout, once.stdout);
        let v = json(&out.stdout);
        assert_eq!(ids(&v["refreshed"]), [R2]);

        // Argument order, not id order.
        let env = Env::new();
        let v = ok(&env.run(["--json", "refresh", R3, R2, R1]));
        assert_eq!(ids(&v["refreshed"]), [R3, R2]);
        assert_eq!(v["unchanged"], 1);
    }

    // ---- criterion 10: an empty or absent store ----------------------------------------------------

    #[test]
    fn all_on_empty_store() {
        let absent = Env::absent();
        let out_absent = absent.run(["--json", "refresh", "--all"]);
        assert_eq!(out_absent.code, Some(0), "{}", out_absent.stderr);
        assert_eq!(out_absent.stderr, "");
        assert_eq!(out_absent.stdout, format!("{EMPTY}\n"));
        assert!(
            !absent.data().exists(),
            "refresh --all created the data dir"
        );

        let empty = Env::empty();
        let out_empty = empty.run(["--json", "refresh", "--all"]);
        assert_eq!(out_empty.stdout, out_absent.stdout);
        assert_eq!(out_empty.code, Some(0));
        assert_eq!(std::fs::read_dir(empty.data()).unwrap().count(), 0);

        let text = Env::absent().run(["refresh", "--all"]);
        assert_eq!(
            (text.code, text.stdout.as_str()),
            (Some(0), "unchanged 0\n")
        );

        // A named ref on an empty store is simply unknown.
        let out = Env::empty().run(["--json", "refresh", R1]);
        failed(&out, 1, "UNKNOWN_RESOURCE");
    }

    // ---- criterion 11: only refresh hashes -------------------------------------------------------

    #[test]
    fn find_show_do_not_open_files() {
        // A mode 000 file with a changed size: `stat` works, opening does not.
        let env = Env::new();
        let edited = env.root.join("files/edited.txt");
        let unreadable = made_unreadable(&edited);

        let shown = ok(&env.run(["--json", "show", R2]));
        assert_eq!(shown["resource"]["stale"], true);
        let found = ok(&env.run(["--json", "find", "--stale"]));
        assert_eq!(ids(&found["results"]), [R2, R3]);
        assert!(found["results"][0]["stale"].as_bool().unwrap());
        ok(&env.run(["--json", "find"]));
        ok(&env.run(["--json", "validate"]));

        if !unreadable {
            return; // root can read a mode 000 file
        }
        let before = env.state();
        let out = env.run(["--json", "refresh", R2]);
        failed(&out, 1, "FILE_UNREADABLE");
        assert_eq!(env.state(), before);
    }

    // ---- criterion 12: --dry-run -------------------------------------------------------------------

    #[test]
    fn refresh_dry_run_json() {
        let env = Env::new();
        let before = env.state();
        let files_before = env.files_state();
        let out = env.run(["--json", "--dry-run", "refresh", "--all"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        assert_eq!(env.state(), before, "a dry run writes nothing");
        assert_eq!(env.files_state(), files_before);
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!("cli_dry_run__refresh_dry_run_json", out.stdout);
        });
    }

    #[test]
    fn refresh_dry_run_matches_real_run() {
        let env = Env::new();
        let store = env.data();

        let dry = support::assert_dry_run_matches_real_run(Some(&store), &["refresh", "--all"]);
        assert_eq!(dry["refreshed"].as_array().unwrap().len(), 2);
        assert_eq!(dry["missing"].as_array().unwrap().len(), 1);
        assert_eq!(dry["unchanged"], 2);
        assert_eq!(dry["changes"].as_array().unwrap().len(), 12, "{dry}");

        // Named, including a Resource only a named ref reads.
        let sneaky = env.file("sneaky.txt");
        let dry =
            support::assert_dry_run_matches_real_run(Some(&store), &["refresh", sneaky.as_str()]);
        assert_eq!(ids(&dry["refreshed"]), [R5]);
        assert_eq!(dry["changes"].as_array().unwrap().len(), 4);

        // Nothing to change: `changes` is `[]`.
        let same = env.file("same.txt");
        let dry =
            support::assert_dry_run_matches_real_run(Some(&store), &["refresh", same.as_str()]);
        assert_eq!(dry["changes"], json!([]));
        assert_eq!(dry["unchanged"], 1);

        // An absent store: empty results, no store created.
        let dry = support::assert_dry_run_matches_real_run(None, &["refresh", "--all"]);
        assert_eq!(dry["changes"], json!([]));
        assert_eq!(dry["unchanged"], 0);
    }

    #[test]
    fn refresh_dry_run_text() {
        let env = Env::new();
        let before = env.state();
        let out = env.run(["--dry-run", "refresh", "--all"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(env.state(), before);
        assert_eq!(out.stderr, "note: dry run; nothing was written\n");
        let lines: Vec<&str> = out.stdout.lines().collect();
        assert_eq!(
            lines[..4],
            [
                format!("would refresh resource {R2} \"edited\" /TMP/files/edited.txt").as_str(),
                format!("would refresh resource {R3} \"touched\" /TMP/files/touched.txt").as_str(),
                format!("missing resource {R4} \"gone\" /TMP/files/gone.txt").as_str(),
                "unchanged 2",
            ]
        );
        // Then the changes, as the JSON lists them.
        let json_out = env.run(["--json", "--dry-run", "refresh", "--all"]);
        let changes: Vec<String> = json(&json_out.stdout)["changes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                let sign = if c["op"] == "add" { '+' } else { '-' };
                format!("{sign} {}", c["triple"].as_str().unwrap())
            })
            .collect();
        assert_eq!(changes.len(), 12);
        assert_eq!(lines[4..], changes[..]);
    }

    // ---- criterion 13: never asks ---------------------------------------------------------------------

    #[test]
    fn never_asks() {
        // No terminal, no --yes, nothing on stdin: it just runs.
        let env = Env::new();
        let out = env.exec(
            &[("HALI_TEST_TTY", "0")],
            b"",
            ["--json", "refresh", "--all"],
        );
        let v = ok(&out);
        assert_eq!(ids(&v["refreshed"]), [R2, R3]);
        assert!(env.resources_ttl().contains(SHA_WORLD));

        // A terminal whose user would say no: it is never asked, and never reads the answer.
        let env = Env::new();
        let out = env.exec(
            &[("HALI_TEST_TTY", "1")],
            b"n\n",
            ["--json", "refresh", "--all"],
        );
        let v = ok(&out);
        assert_eq!(ids(&v["refreshed"]), [R2, R3]);
        assert!(env.resources_ttl().contains(SHA_WORLD));

        // --yes is accepted and changes nothing.
        let plain = Env::new().run(["--json", "refresh", "--all"]);
        let env = Env::new();
        let with_yes = env.run(["--json", "refresh", "--all", "--yes"]);
        assert_eq!(with_yes.code, Some(0), "{}", with_yes.stderr);
        assert_eq!(with_yes.stdout, plain.stdout);
    }

    // ---- criterion 15: the schema ------------------------------------------------------------------------

    #[test]
    fn schema_lists_refresh() {
        let env = Env::new();
        let v = ok(&env.run(["schema", "--json"]));
        assert_eq!(
            v["schemas"],
            json!([
                "add",
                "concept add",
                "concept edit",
                "concept list",
                "concept remove",
                "concept rename",
                "concept show",
                "describe",
                "edit",
                "error",
                "find",
                "forget",
                "refresh",
                "relate",
                "relation add",
                "relation list",
                "relation remove",
                "schema",
                "show",
                "type add",
                "type list",
                "type remove",
                "unrelate",
                "validate"
            ])
        );
        assert_eq!(v["schemas"].as_array().unwrap().len(), 24);
        let out = env.run(["schema", "refresh"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        let doc = json(&out.stdout);
        assert!(doc.is_object());
        assert!(doc.get("$id").is_none(), "a bare document");
        let out2 = env.run(["schema", "refresh", "--json"]);
        assert_eq!(out2.stdout, out.stdout);
    }
}
