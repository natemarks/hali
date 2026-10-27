//! Goal tests for `hali add`, `hali show` and `hali edit --title` through the real binary
//! (#10, spec §2.1, §2.5, §6, §9.2, §9.3). Locked golden snapshots sit in tests/snapshots/
//! (hand-written from the spec and the issue's contract).
//!
//! Every run sets HALI_DATA_DIR / XDG_CONFIG_HOME / HOME to temp dirs and clears the rest of
//! the environment, so the real store and config are never read. The temp root is
//! canonicalised and shown as `/TMP` in captured output. Files get a pinned mtime of
//! 2026-10-20T14:02:11Z plus a sub-second part, which `modified` must truncate away.
//!
//! Ids: `HALI_TEST_ID_SEED=1` yields R1, C1, P, R2, C2 (see the constants); a second process
//! restarts the sequence, so a second `add` run uses another seed.
//!
//! Only `.txt` files are added through the real MIME path here (glob `text/plain`, and
//! text-looking content is `text/plain` even without a database), so these tests need no
//! outside software. The two that do are `#[ignore]`d.

#[cfg(feature = "goal")]
mod goal {
    use assert_cmd::Command;
    use serde_json::Value;
    use std::collections::BTreeSet;
    use std::ffi::{OsStr, OsString};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime};
    use tempfile::TempDir;

    const R1: &str = "018bcfe5-6801-710a-adec-89025cc1beeb";
    const C1: &str = "018bcfe5-6802-7893-a2ee-fb32555e71c1";
    const P: &str = "018bcfe5-6803-71bb-94d8-d101b5b9c34d";
    const R2: &str = "018bcfe5-6804-7099-ac6c-d7363ca585e7";
    const C2: &str = "018bcfe5-6805-7917-98de-357e3da8cb43";

    const NOW: &str = "2026-10-24T09:30:00Z";
    const LATER: &str = "2026-10-25T10:00:00Z";
    const MTIME_SECS: u64 = 1_792_504_931; // 2026-10-20T14:02:11Z
    const HELLO_SHA: &str = "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03";
    const EMPTY_SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    const XSD: &str = "http://www.w3.org/2001/XMLSchema#";
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
            for d in ["cfg", "home", "files"] {
                std::fs::create_dir_all(root.join(d)).unwrap();
            }
            Env { _root: tmp, root }
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

        /// A file under files/ with the pinned mtime (plus nanoseconds).
        fn file(&self, name: &str, contents: &[u8]) -> PathBuf {
            let path = self.files().join(name);
            std::fs::write(&path, contents).unwrap();
            pin_mtime(&path);
            path
        }

        fn write_config(&self, toml: &str) {
            let dir = self.root.join("cfg/hali");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("config.toml"), toml).unwrap();
        }

        /// The command with the standard environment; the caller adds args.
        fn command(&self, seed: u64, now: &str) -> Command {
            let mut cmd = Command::cargo_bin("hali").unwrap();
            cmd.env_clear()
                .env("XDG_CONFIG_HOME", self.root.join("cfg"))
                .env("HOME", self.root.join("home"))
                .env("HALI_DATA_DIR", self.data())
                .env("HALI_TEST_NOW", now)
                .env("HALI_TEST_ID_SEED", seed.to_string())
                .timeout(Duration::from_secs(30));
            cmd
        }

        fn finish(&self, mut cmd: Command) -> Out {
            let out = cmd.output().unwrap();
            let root = self.root.to_str().unwrap();
            let clean = |bytes: Vec<u8>| String::from_utf8(bytes).unwrap().replace(root, "/TMP");
            Out {
                code: out.status.code(),
                stdout: clean(out.stdout),
                stderr: clean(out.stderr),
            }
        }

        fn run_with<I, S>(&self, seed: u64, now: &str, args: I) -> Out
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            let mut cmd = self.command(seed, now);
            cmd.args(args);
            self.finish(cmd)
        }

        fn run<I, S>(&self, args: I) -> Out
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            self.run_with(1, NOW, args)
        }

        fn run_env<I, S>(&self, envs: &[(&str, &str)], seed: u64, args: I) -> Out
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            let mut cmd = self.command(seed, NOW);
            for (k, v) in envs {
                cmd.env(k, v);
            }
            cmd.args(args);
            self.finish(cmd)
        }

        fn run_in<I, S>(&self, cwd: &Path, args: I) -> Out
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            let mut cmd = self.command(1, NOW);
            cmd.current_dir(cwd).args(args);
            self.finish(cmd)
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

        fn resources_bytes(&self) -> Option<Vec<u8>> {
            std::fs::read(self.resources_ttl()).ok()
        }
    }

    fn pin_mtime(path: &Path) {
        let t = SystemTime::UNIX_EPOCH + Duration::new(MTIME_SECS, 987_654_321);
        std::fs::File::open(path).unwrap().set_modified(t).unwrap();
    }

    fn json(s: &str) -> Value {
        assert_eq!(
            s.matches('\n').count(),
            1,
            "one compact line expected: {s:?}"
        );
        assert!(s.ends_with('\n'), "{s:?}");
        serde_json::from_str(s).unwrap_or_else(|e| panic!("not JSON ({e}): {s:?}"))
    }

    fn ok(out: &Out) -> Value {
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        json(&out.stdout)
    }

    /// The error envelope on stderr: asserts exit `code`, empty stdout, returns `error`.
    fn error(out: &Out, code: i32) -> Value {
        assert_eq!(out.code, Some(code), "stderr: {}", out.stderr);
        assert_eq!(out.stdout, "");
        let v = json(&out.stderr);
        assert_eq!(v["schema_version"], 1);
        let e = v["error"].clone();
        assert!(e["message"].as_str().is_some_and(|m| !m.is_empty()), "{e}");
        e
    }

    fn nt(s: &str, p: &str, o: &str) -> String {
        format!("<{s}> <{p}> {o} .")
    }

    fn dt(v: &str) -> String {
        format!("\"{v}\"^^<{XSD}dateTime>")
    }

    /// The triples one `add` writes for a `text/plain` file (with the user Person when
    /// `person` is given as `(id, name)`).
    #[allow(clippy::too_many_arguments)]
    fn add_triples(
        r: &str,
        c: &str,
        person: Option<(&str, &str)>,
        url: &str,
        title: &str,
        size: u64,
        sha: &str,
        now: &str,
        p_id: &str,
    ) -> BTreeSet<String> {
        let r = format!("urn:uuid:{r}");
        let c = format!("urn:uuid:{c}");
        let sum = format!("{r}#sha256");
        let p = format!("urn:uuid:{p_id}");
        let mut set = BTreeSet::from([
            nt(&r, RDF_TYPE, "<https://w3id.org/hali#Resource>"),
            nt(
                &r,
                "http://purl.org/dc/terms/title",
                &format!("\"{title}\""),
            ),
            nt(
                &r,
                "http://www.w3.org/ns/dcat#downloadURL",
                &format!("<{url}>"),
            ),
            nt(
                &r,
                "http://www.w3.org/ns/dcat#mediaType",
                "<http://www.iana.org/assignments/media-types/text/plain>",
            ),
            nt(
                &r,
                "http://www.w3.org/ns/dcat#byteSize",
                &format!("\"{size}\"^^<{XSD}integer>"),
            ),
            nt(
                &r,
                "http://purl.org/dc/terms/modified",
                &dt("2026-10-20T14:02:11Z"),
            ),
            nt(
                &r,
                "http://spdx.org/rdf/terms#checksum",
                &format!("<{sum}>"),
            ),
            nt(
                &sum,
                "http://spdx.org/rdf/terms#algorithm",
                "<http://spdx.org/rdf/terms#checksumAlgorithm_sha256>",
            ),
            nt(
                &sum,
                "http://spdx.org/rdf/terms#checksumValue",
                &format!("\"{sha}\"^^<{XSD}hexBinary>"),
            ),
            nt(&c, RDF_TYPE, "<http://www.w3.org/ns/dcat#CatalogRecord>"),
            nt(
                &c,
                "http://xmlns.com/foaf/0.1/primaryTopic",
                &format!("<{r}>"),
            ),
            nt(&c, "http://purl.org/dc/terms/issued", &dt(now)),
            nt(&c, "http://purl.org/dc/terms/modified", &dt(now)),
            nt(
                &c,
                "http://www.w3.org/ns/prov#wasAttributedTo",
                &format!("<{p}>"),
            ),
        ]);
        if let Some((_, name)) = person {
            set.insert(nt(&p, RDF_TYPE, "<http://www.w3.org/ns/prov#Person>"));
            set.insert(nt(
                &p,
                "http://xmlns.com/foaf/0.1/name",
                &format!("\"{name}\""),
            ));
        }
        set
    }

    fn notes(env: &Env) -> PathBuf {
        env.file("notes.txt", b"hello\n")
    }

    /// Adds notes.txt (seed 1) and returns its path.
    fn add_notes(env: &Env) -> PathBuf {
        let path = notes(env);
        ok(&env.run(["--json", "add", path.to_str().unwrap()]));
        path
    }

    fn snapshot_stdout(name: &str, out: &Out) {
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        // The locked snapshots are named without the `goal__` module segment.
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!(name, out.stdout);
        });
    }

    // ---- criterion 1: the Resource record -------------------------------------------------

    #[test]
    fn add_json() {
        let env = Env::new();
        let path = notes(&env);
        let out = env.run(["--json", "add", path.to_str().unwrap()]);
        snapshot_stdout("cli_resource__add_json", &out);

        let url = format!("file://{}/files/notes.txt", env.root.display());
        let want = add_triples(
            R1,
            C1,
            Some((P, "user")),
            &url,
            "notes",
            6,
            HELLO_SHA,
            NOW,
            P,
        );
        assert_eq!(env.triples(), want);
    }

    #[test]
    fn add_text_mode_names_the_resource() {
        let env = Env::new();
        let path = notes(&env);
        let out = env.run(["add", path.to_str().unwrap()]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert!(out.stdout.contains(R1), "{}", out.stdout);
        assert!(out.stdout.contains("notes"), "{}", out.stdout);
        assert!(!env.triples().is_empty());
    }

    #[test]
    fn add_empty_file() {
        let env = Env::new();
        let path = env.file("empty.txt", b"");
        let v = ok(&env.run(["--json", "add", path.to_str().unwrap()]));
        let rec = &v["added"][0];
        assert_eq!(rec["byte_size"], 0);
        assert_eq!(rec["media_type"], "text/plain");
        assert!(
            env.triples().iter().any(|t| t.contains(EMPTY_SHA)),
            "the SHA-256 of nothing"
        );
    }

    #[test]
    fn modified_is_truncated_to_the_second() {
        let env = Env::new();
        let path = notes(&env);
        let v = ok(&env.run(["--json", "add", path.to_str().unwrap()]));
        assert_eq!(v["added"][0]["modified"], "2026-10-20T14:02:11Z");
    }

    #[test]
    fn add_percent_encodes_the_url_and_show_decodes_it() {
        let env = Env::new();
        let path = env.file("my notes \u{e9}.txt", b"hello\n");
        ok(&env.run(["--json", "add", path.to_str().unwrap()]));
        let want = format!(
            "<file://{}/files/my%20notes%20%C3%A9.txt>",
            env.root.display()
        );
        assert!(
            env.triples()
                .iter()
                .any(|t| t.contains("<http://www.w3.org/ns/dcat#downloadURL>") && t.contains(&want)),
            "{:?}",
            env.triples()
        );
        let v = ok(&env.run(["--json", "show", path.to_str().unwrap()]));
        assert_eq!(v["resource"]["path"], "/TMP/files/my notes \u{e9}.txt");
    }

    #[test]
    fn default_title_is_the_file_stem() {
        let env = Env::new();
        let tar = env.file("archive.tar.gz", b"hello\n");
        let ctl = env.file("a\u{1}b.txt", b"hello\n");
        let v = ok(&env.run([
            "--json",
            "add",
            tar.to_str().unwrap(),
            ctl.to_str().unwrap(),
        ]));
        assert_eq!(v["added"][0]["title"], "archive.tar");
        assert_eq!(v["added"][1]["title"], "a\u{fffd}b");
    }

    #[test]
    fn title_flag_overrides_and_is_trimmed() {
        let env = Env::new();
        let path = notes(&env);
        let v = ok(&env.run([
            "--json",
            "add",
            "--title",
            "  Field Notes  ",
            path.to_str().unwrap(),
        ]));
        assert_eq!(v["added"][0]["title"], "Field Notes");
    }

    #[test]
    fn invalid_titles_are_usage_errors() {
        // A valid title works (so a bare "unknown subcommand" usage error cannot pass this).
        let fine = Env::new();
        let fine_path = notes(&fine);
        let v = ok(&fine.run([
            "--json",
            "add",
            "--title",
            "Fine",
            fine_path.to_str().unwrap(),
        ]));
        assert_eq!(v["added"][0]["title"], "Fine");

        let env = Env::new();
        let path = notes(&env);
        for title in ["", "   ", "a\u{7}b", "tab\there"] {
            let out = env.run(["add", "--title", title, path.to_str().unwrap()]);
            assert_eq!(out.code, Some(2), "{title:?}: {}", out.stderr);
            assert_eq!(out.stdout, "");
            assert!(!env.data().exists(), "{title:?} wrote");
        }
        // `--title` with more than one path is a usage error too.
        let other = env.file("other.txt", b"x");
        let out = env.run([
            "add",
            "--title",
            "T",
            path.to_str().unwrap(),
            other.to_str().unwrap(),
        ]);
        assert_eq!(out.code, Some(2), "{}", out.stderr);
        assert!(!env.data().exists());
    }

    #[test]
    #[ignore = "needs shared-mime-info"]
    fn add_sh_file_uses_dcterms_format() {
        let env = Env::new();
        let path = env.file("run.sh", b"#!/bin/sh\necho hi\n");
        let v = ok(&env.run(["--json", "add", path.to_str().unwrap()]));
        assert_eq!(v["added"][0]["media_type"], "application/x-shellscript");
        assert_eq!(v["added"][0]["editable"], false);
        let iri = "https://w3id.org/hali/media-types/application/x-shellscript";
        let triples = env.triples();
        assert!(
            triples
                .iter()
                .any(|t| t.contains("<http://purl.org/dc/terms/format>") && t.contains(iri))
        );
        assert!(triples.contains(&format!(
            "<{iri}> <{RDF_TYPE}> <http://purl.org/dc/terms/MediaType> ."
        )));
        assert!(!triples.iter().any(|t| t.contains("dcat#mediaType")));
    }

    #[test]
    #[ignore = "needs shared-mime-info"]
    fn add_md_file_is_markdown_and_editable() {
        let env = Env::new();
        let path = env.file("README.md", b"# hi\n");
        let v = ok(&env.run(["--json", "add", path.to_str().unwrap()]));
        assert_eq!(v["added"][0]["media_type"], "text/markdown");
        assert_eq!(v["added"][0]["editable"], true);
        assert_eq!(v["added"][0]["title"], "README");
    }

    // ---- several paths ---------------------------------------------------------------------

    #[test]
    fn several_paths_keep_argument_order_and_count_a_repeat_once() {
        let env = Env::new();
        let a = env.file("a.txt", b"hello\n");
        let b = env.file("b.txt", b"second file\n");
        let (a, b) = (a.to_str().unwrap(), b.to_str().unwrap());
        let v = ok(&env.run(["--json", "add", b, a, b]));
        let added = v["added"].as_array().unwrap();
        assert_eq!(added.len(), 2, "{v}");
        assert_eq!(added[0]["title"], "b");
        assert_eq!(added[1]["title"], "a");
        assert_eq!(v["existing"], serde_json::json!([]));

        // Ids are minted per path: Resource, catalog record, then the Person (first time only).
        assert_eq!(added[0]["id"], R1);
        assert_eq!(added[1]["id"], R2);
        let triples = env.triples();
        let topic = |c: &str, r: &str| {
            nt(
                &format!("urn:uuid:{c}"),
                "http://xmlns.com/foaf/0.1/primaryTopic",
                &format!("<urn:uuid:{r}>"),
            )
        };
        assert!(triples.contains(&topic(C1, R1)), "{triples:?}");
        assert!(triples.contains(&topic(C2, R2)), "{triples:?}");
        assert!(triples.contains(&nt(
            &format!("urn:uuid:{P}"),
            RDF_TYPE,
            "<http://www.w3.org/ns/prov#Person>"
        )));
    }

    #[test]
    fn several_paths_split_added_and_existing() {
        let env = Env::new();
        let a = add_notes(&env);
        let b = env.file("b.txt", b"second file\n");
        let v = ok(&env.run_with(
            2,
            NOW,
            ["--json", "add", b.to_str().unwrap(), a.to_str().unwrap()],
        ));
        assert_eq!(v["added"].as_array().unwrap().len(), 1);
        assert_eq!(v["added"][0]["title"], "b");
        assert_eq!(v["existing"].as_array().unwrap().len(), 1);
        assert_eq!(v["existing"][0]["id"], R1);
    }

    #[test]
    fn several_paths_are_all_or_nothing() {
        let env = Env::new();
        let good = env.file("good.txt", b"hello\n");
        let missing = env.files().join("nope.txt");
        let out = env.run([
            "--json",
            "add",
            good.to_str().unwrap(),
            missing.to_str().unwrap(),
        ]);
        let e = error(&out, 1);
        assert_eq!(e["rule"], "NOT_A_REGULAR_FILE");
        assert_eq!(e["details"]["path"], "/TMP/files/nope.txt");
        assert!(!env.data().exists(), "nothing was written");

        // The same with a store already holding a Resource: its bytes do not change.
        let env = Env::new();
        add_notes(&env);
        let before = env.resources_bytes();
        let b = env.file("b.txt", b"second file\n");
        let dir = env.files();
        let out = env.run_with(
            2,
            NOW,
            ["--json", "add", b.to_str().unwrap(), dir.to_str().unwrap()],
        );
        assert_eq!(error(&out, 1)["details"]["kind"], "directory");
        assert_eq!(env.resources_bytes(), before);
    }

    // ---- criterion 3: catalog record and the user Person ------------------------------------

    fn attributed_name(env: &Env, path: &Path) -> String {
        let v = ok(&env.run(["--json", "show", path.to_str().unwrap()]));
        v["resource"]["catalog_record"]["attributed_to"]["name"]
            .as_str()
            .unwrap()
            .to_string()
    }

    #[test]
    fn user_name_from_config() {
        let env = Env::new();
        env.write_config("[user]\nname = \"Nate Marks\"\n");
        let path = notes(&env);
        let out = env.run_env(
            &[("USER", "alice")],
            1,
            ["--json", "add", path.to_str().unwrap()],
        );
        ok(&out);
        assert_eq!(attributed_name(&env, &path), "Nate Marks");
    }

    #[test]
    fn user_name_from_env_user() {
        let env = Env::new();
        let path = notes(&env);
        let out = env.run_env(
            &[("USER", "alice"), ("LOGNAME", "carol")],
            1,
            ["--json", "add", path.to_str().unwrap()],
        );
        ok(&out);
        assert_eq!(attributed_name(&env, &path), "alice");
    }

    #[test]
    fn user_name_falls_back_to_logname_then_user() {
        let env = Env::new();
        let path = notes(&env);
        ok(&env.run_env(
            &[("LOGNAME", "carol")],
            1,
            ["--json", "add", path.to_str().unwrap()],
        ));
        assert_eq!(attributed_name(&env, &path), "carol");

        let env = Env::new();
        let path = notes(&env);
        ok(&env.run(["--json", "add", path.to_str().unwrap()]));
        assert_eq!(attributed_name(&env, &path), "user");
    }

    #[test]
    fn empty_or_control_user_name_is_config_invalid() {
        for toml in ["[user]\nname = \"\"\n", "[user]\nname = \"a\\u0001b\"\n"] {
            let env = Env::new();
            env.write_config(toml);
            let path = notes(&env);
            let out = env.run(["--json", "add", path.to_str().unwrap()]);
            let e = error(&out, 3);
            assert_eq!(e["rule"], "CONFIG_INVALID", "{toml:?}");
            assert!(!env.data().exists(), "{toml:?} wrote");
        }
    }

    #[test]
    fn the_person_is_minted_once_and_its_name_is_fixed() {
        let env = Env::new();
        let a = notes(&env);
        ok(&env.run_env(
            &[("USER", "alice")],
            1,
            ["--json", "add", a.to_str().unwrap()],
        ));
        let b = env.file("b.txt", b"second file\n");
        ok(&env.run_env(
            &[("USER", "bob")],
            2,
            ["--json", "add", b.to_str().unwrap()],
        ));

        let triples = env.triples();
        let persons = triples.iter().filter(|t| t.contains("prov#Person")).count();
        assert_eq!(persons, 1, "{triples:?}");
        assert_eq!(attributed_name(&env, &a), "alice");
        assert_eq!(attributed_name(&env, &b), "alice");
        assert!(!triples.iter().any(|t| t.contains("\"bob\"")));
    }

    // ---- criterion 4: symlinks and refs ------------------------------------------------------

    #[test]
    fn add_via_symlink_stores_the_real_path() {
        let env = Env::new();
        let real = env.file("real.txt", b"hello\n");
        let link = env.files().join("link.txt");
        symlink(&real, &link).unwrap();
        let v = ok(&env.run(["--json", "add", link.to_str().unwrap()]));
        assert_eq!(v["added"][0]["path"], "/TMP/files/real.txt");
        assert_eq!(v["added"][0]["title"], "real");
        let stored: Vec<String> = env
            .triples()
            .into_iter()
            .filter(|t| t.contains("dcat#downloadURL"))
            .collect();
        assert_eq!(stored.len(), 1);
        assert!(stored[0].contains("/files/real.txt>"), "{stored:?}");

        // The same Resource is found by the link, the real path, and a relative spelling.
        for r in [
            link.to_str().unwrap().to_string(),
            real.to_str().unwrap().to_string(),
        ] {
            let shown = ok(&env.run(["--json", "show", &r]));
            assert_eq!(shown["resource"]["id"], R1, "{r}");
        }
    }

    #[test]
    fn show_by_relative_path() {
        let env = Env::new();
        let link_target = add_notes(&env);
        symlink(&link_target, env.files().join("alias.txt")).unwrap();
        for r in [
            "notes.txt",
            "./notes.txt",
            "alias.txt",
            "../files/notes.txt",
        ] {
            let out = env.run_in(&env.files(), ["--json", "show", r]);
            assert_eq!(ok(&out)["resource"]["id"], R1, "{r}");
        }
    }

    #[test]
    fn show_by_uuid() {
        let env = Env::new();
        add_notes(&env);
        let upper = R1.to_uppercase();
        let urn = format!("urn:uuid:{R1}");
        let urn_upper = format!("urn:uuid:{upper}");
        for r in [R1, upper.as_str(), urn.as_str(), urn_upper.as_str()] {
            let v = ok(&env.run(["--json", "show", r]));
            assert_eq!(v["resource"]["id"], R1, "{r}");
        }
    }

    #[test]
    fn uuid_prefix_is_unknown_resource() {
        let env = Env::new();
        add_notes(&env);
        for r in [
            "018bcfe5",
            "018bcfe5-6801",
            "018bcfe5-6801-710a-adec-89025cc1bee",
        ] {
            let out = env.run(["--json", "show", r]);
            let e = error(&out, 1);
            assert_eq!(e["rule"], "UNKNOWN_RESOURCE", "{r}");
            assert_eq!(e["details"]["ref"], r);
            assert!(
                e["suggested_fix"].as_str().unwrap().contains("prefix"),
                "the hint says prefixes are not accepted: {e}"
            );
        }
    }

    #[test]
    fn unknown_resource() {
        let env = Env::new();
        // No store at all: still a rule error, and nothing is created.
        let out = env.run(["--json", "show", "0192f0c4-7b1e-7cc3-9a43-6c1d2e8f4b10"]);
        let e = error(&out, 1);
        assert_eq!(e["rule"], "UNKNOWN_RESOURCE");
        assert_eq!(e["details"]["ref"], "0192f0c4-7b1e-7cc3-9a43-6c1d2e8f4b10");
        assert!(!env.data().exists());

        // A path that exists but is not cataloged, and one that does not exist.
        add_notes(&env);
        let other = env.file("other.txt", b"x");
        for r in [other.to_str().unwrap(), "no/such/file.txt"] {
            let rendered = r.replace(env.root.to_str().unwrap(), "/TMP");
            let e = error(&env.run(["--json", "show", r]), 1);
            assert_eq!(e["rule"], "UNKNOWN_RESOURCE", "{r}");
            assert_eq!(e["details"]["ref"], rendered);
        }
        // Text mode: an `error:` line and a `hint:` line.
        let out = env.run(["show", "nope"]);
        assert_eq!(out.code, Some(1));
        assert!(out.stderr.starts_with("error: "), "{}", out.stderr);
        assert!(out.stderr.contains("hint: "), "{}", out.stderr);
    }

    // ---- show ---------------------------------------------------------------------------------

    #[test]
    fn show_json() {
        let env = Env::new();
        let path = add_notes(&env);
        let out = env.run(["--json", "show", path.to_str().unwrap()]);
        snapshot_stdout("cli_resource__show_json", &out);
    }

    #[test]
    fn show_text_mode() {
        let env = Env::new();
        let path = add_notes(&env);
        let out = env.run(["show", path.to_str().unwrap()]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert!(out.stdout.contains("notes"), "{}", out.stdout);
        assert!(
            out.stdout.contains("/TMP/files/notes.txt"),
            "{}",
            out.stdout
        );
    }

    #[test]
    fn show_computes_missing_and_stale_from_stat_only() {
        let env = Env::new();
        let path = add_notes(&env);
        let shown = |env: &Env| ok(&env.run(["--json", "show", R1]))["resource"].clone();

        let v = shown(&env);
        assert_eq!(
            (v["missing"].clone(), v["stale"].clone()),
            (false.into(), false.into())
        );

        // Changed size and mtime: stale. The record keeps what was cataloged.
        std::fs::write(&path, b"hello, a longer line\n").unwrap();
        let v = shown(&env);
        assert_eq!(v["missing"], false);
        assert_eq!(v["stale"], true);
        assert_eq!(v["byte_size"], 6);
        assert_eq!(v["modified"], "2026-10-20T14:02:11Z");

        // Same size, later mtime: stale. Restoring the mtime clears it.
        std::fs::write(&path, b"HELLO\n").unwrap();
        assert_eq!(shown(&env)["stale"], true);
        pin_mtime(&path);
        assert_eq!(shown(&env)["stale"], false);

        std::fs::remove_file(&path).unwrap();
        assert_eq!(shown(&env)["missing"], true);
    }

    #[test]
    fn show_path_only() {
        let env = Env::new();
        let path = env.file("my notes \u{e9}.txt", b"hello\n");
        ok(&env.run(["--json", "add", path.to_str().unwrap()]));
        for args in [
            vec!["show", "--path-only", R1],
            vec!["--json", "show", "--path-only", R1],
            vec!["show", R1, "--path-only", "--json"],
        ] {
            let out = env.run(&args);
            assert_eq!(out.code, Some(0), "{args:?}: {}", out.stderr);
            assert_eq!(out.stderr, "", "{args:?}");
            assert_eq!(
                out.stdout, "/TMP/files/my notes \u{e9}.txt\n",
                "{args:?}: the decoded real path and a newline; --path-only wins over --json"
            );
        }
    }

    // ---- criterion 5: re-add is idempotent -----------------------------------------------------

    #[test]
    fn re_add_json_existing() {
        let env = Env::new();
        let path = add_notes(&env);
        let before = env.resources_bytes();
        let out = env.run_with(
            2,
            "2026-11-01T00:00:00Z",
            ["--json", "add", path.to_str().unwrap()],
        );
        snapshot_stdout("cli_resource__re_add_json_existing", &out);
        assert_eq!(env.resources_bytes(), before, "a re-add writes nothing");

        // A title given now changes nothing; neither does a changed file (no refresh).
        std::fs::write(&path, b"a much longer line now\n").unwrap();
        let out = env.run_with(
            3,
            "2026-11-02T00:00:00Z",
            ["--json", "add", "--title", "Other", path.to_str().unwrap()],
        );
        let v = ok(&out);
        assert_eq!(v["added"], serde_json::json!([]));
        assert_eq!(v["existing"][0]["title"], "notes");
        assert_eq!(v["existing"][0]["byte_size"], 6);
        assert_eq!(v["existing"][0]["id"], R1);
        assert_eq!(env.resources_bytes(), before);
    }

    #[test]
    fn re_add_through_a_symlink_or_relative_path_is_the_same_resource() {
        let env = Env::new();
        let real = add_notes(&env);
        let link = env.files().join("link.txt");
        symlink(&real, &link).unwrap();
        let v = ok(&env.run_with(2, NOW, ["--json", "add", link.to_str().unwrap()]));
        assert_eq!(v["existing"][0]["id"], R1);
        let mut cmd = env.command(3, NOW);
        cmd.current_dir(env.files())
            .args(["--json", "add", "./notes.txt"]);
        let v = ok(&env.finish(cmd));
        assert_eq!(v["existing"][0]["id"], R1);
        assert_eq!(
            env.triples()
                .iter()
                .filter(|t| t.contains("dcat#downloadURL"))
                .count(),
            1
        );
    }

    // ---- criterion 6: error rules ---------------------------------------------------------------

    fn not_a_regular_file(env: &Env, path: &Path, kind: &str) {
        let shown = path
            .to_str()
            .unwrap()
            .replace(env.root.to_str().unwrap(), "/TMP");
        let e = error(&env.run(["--json", "add", path.to_str().unwrap()]), 1);
        assert_eq!(e["rule"], "NOT_A_REGULAR_FILE", "{kind}");
        assert_eq!(e["details"]["path"], shown, "{kind}");
        assert_eq!(e["details"]["kind"], kind);
        assert!(!env.data().exists(), "{kind}: nothing was written");

        let out = env.run(["add", path.to_str().unwrap()]);
        assert_eq!(out.code, Some(1), "{kind}");
        assert_eq!(out.stdout, "");
        assert!(out.stderr.starts_with("error: "), "{}", out.stderr);
        assert!(out.stderr.contains("hint: "), "{}", out.stderr);
    }

    #[test]
    fn not_a_regular_file_directory() {
        let env = Env::new();
        not_a_regular_file(&env, &env.files(), "directory");
    }

    #[test]
    fn not_a_regular_file_fifo() {
        let env = Env::new();
        let fifo = env.files().join("pipe");
        let status = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap();
        assert!(status.success());
        // Must fail fast, not block opening the pipe (the run has a timeout).
        not_a_regular_file(&env, &fifo, "other");
    }

    #[test]
    fn not_a_regular_file_missing() {
        let env = Env::new();
        not_a_regular_file(&env, &env.files().join("nope.txt"), "missing");
    }

    #[test]
    fn non_utf8_path_arg() {
        let env = Env::new();
        let mut path: OsString = env.files().into_os_string();
        path.push(OsStr::from_bytes(b"/bad\xff.txt"));
        // The argument is rejected on its own, whether or not the file exists.
        let out = env.run([OsStr::new("--json"), OsStr::new("add"), path.as_os_str()]);
        let e = error(&out, 1);
        assert_eq!(e["rule"], "NON_UTF8_PATH");
        assert_eq!(e["details"]["path"], "/TMP/files/bad\u{fffd}.txt");
        assert!(!env.data().exists());

        let out = env.run([OsStr::new("add"), path.as_os_str()]);
        assert_eq!(out.code, Some(1));
        assert!(out.stderr.starts_with("error: "), "{}", out.stderr);
    }

    #[test]
    fn non_utf8_symlink_target() {
        let env = Env::new();
        let mut real: OsString = env.files().into_os_string();
        real.push(OsStr::from_bytes(b"/real\xff.txt"));
        std::fs::write(&real, b"hello\n").unwrap();
        let link = env.files().join("link.txt");
        symlink(&real, &link).unwrap();
        // The argument is UTF-8; the resolved real path is not.
        let out = env.run(["--json", "add", link.to_str().unwrap()]);
        let e = error(&out, 1);
        assert_eq!(e["rule"], "NON_UTF8_PATH");
        assert_eq!(e["details"]["path"], "/TMP/files/real\u{fffd}.txt");
        assert!(!env.data().exists());
    }

    #[test]
    fn file_unreadable() {
        let env = Env::new();
        let path = notes(&env);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::File::open(&path).is_ok() {
            eprintln!("skipped: running as a user that can read a mode 000 file (root)");
            return;
        }
        let e = error(&env.run(["--json", "add", path.to_str().unwrap()]), 1);
        assert_eq!(e["rule"], "FILE_UNREADABLE");
        assert_eq!(e["details"]["path"], "/TMP/files/notes.txt");
        assert!(!env.data().exists());
    }

    // ---- criterion 8: the cataloged file is never written ----------------------------------------

    #[test]
    fn cataloged_file_is_never_written() {
        let env = Env::new();
        let path = notes(&env);
        let p = path.to_str().unwrap();
        let stat = || {
            let m = std::fs::metadata(&path).unwrap();
            (
                std::fs::read(&path).unwrap(),
                m.modified().unwrap(),
                m.len(),
                m.permissions().mode(),
            )
        };
        let before = stat();
        let runs: Vec<(u64, Vec<&str>)> = vec![
            (1, vec!["--json", "add", p]),
            (2, vec!["--json", "add", p]),
            (3, vec!["--json", "show", p]),
            (3, vec!["show", "--path-only", p]),
            (3, vec!["--json", "edit", p, "--title", "Renamed"]),
            (3, vec!["--json", "--dry-run", "add", p]),
            (
                3,
                vec!["--json", "--dry-run", "edit", p, "--title", "Again"],
            ),
        ];
        for (seed, args) in runs {
            let out = env.run_with(seed, NOW, &args);
            assert_eq!(out.code, Some(0), "{args:?}: {}", out.stderr);
            assert_eq!(stat(), before, "{args:?} touched the file");
        }
    }

    // ---- criterion 9: dry-run text and goldens ----------------------------------------------------

    #[test]
    fn add_dry_run_json() {
        let env = Env::new();
        let path = notes(&env);
        let out = env.run(["--json", "--dry-run", "add", path.to_str().unwrap()]);
        snapshot_stdout("cli_dry_run__add_dry_run_json", &out);
        assert!(!env.data().exists(), "a dry run writes nothing");
    }

    #[test]
    fn edit_dry_run_json() {
        let env = Env::new();
        let path = add_notes(&env);
        let before = env.resources_bytes();
        let out = env.run_with(
            1,
            LATER,
            [
                "--json",
                "--dry-run",
                "edit",
                path.to_str().unwrap(),
                "--title",
                "Notes",
            ],
        );
        snapshot_stdout("cli_dry_run__edit_dry_run_json", &out);
        assert_eq!(env.resources_bytes(), before);
    }

    #[test]
    fn add_dry_run_text() {
        let env = Env::new();
        let path = notes(&env);
        let out = env.run(["--dry-run", "add", path.to_str().unwrap()]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "note: dry run; nothing was written\n");
        let lines: Vec<&str> = out.stdout.lines().collect();
        assert_eq!(
            lines[0],
            format!("would add resource {R1} \"notes\" /TMP/files/notes.txt")
        );
        assert_eq!(lines.len(), 1 + 16, "{}", out.stdout);
        assert!(
            lines[1..].iter().all(|l| l.starts_with("+ <urn:uuid:")),
            "{}",
            out.stdout
        );
        let mut sorted = lines[1..].to_vec();
        sorted.sort_unstable();
        assert_eq!(lines[1..], sorted[..], "changes are sorted");
        assert!(!env.data().exists());
    }

    #[test]
    fn edit_dry_run_text() {
        let env = Env::new();
        add_notes(&env);
        let out = env.run_with(1, LATER, ["--dry-run", "edit", R1, "--title", "Notes"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "note: dry run; nothing was written\n");
        let lines: Vec<&str> = out.stdout.lines().collect();
        assert_eq!(lines[0], format!("would retitle resource {R1} \"Notes\""));
        assert_eq!(lines.len(), 1 + 4, "{}", out.stdout);
        assert!(lines[1..3].iter().all(|l| l.starts_with("+ ")));
        assert!(lines[3..].iter().all(|l| l.starts_with("- ")));
    }

    // ---- edit --title ------------------------------------------------------------------------------

    #[test]
    fn edit_json() {
        let env = Env::new();
        let path = add_notes(&env);
        let out = env.run_with(
            1,
            LATER,
            [
                "--json",
                "edit",
                path.to_str().unwrap(),
                "--title",
                "  Notes  ",
            ],
        );
        snapshot_stdout("cli_resource__edit_json", &out);

        // One title, and the catalog record's modified (not issued) moved to now.
        let triples = env.triples();
        let titles: Vec<&String> = triples
            .iter()
            .filter(|t| t.contains("dc/terms/title"))
            .collect();
        assert_eq!(titles.len(), 1, "{titles:?}");
        let c = format!("urn:uuid:{C1}");
        assert!(triples.contains(&nt(&c, "http://purl.org/dc/terms/issued", &dt(NOW))));
        assert!(triples.contains(&nt(&c, "http://purl.org/dc/terms/modified", &dt(LATER))));
        assert!(!triples.contains(&nt(&c, "http://purl.org/dc/terms/modified", &dt(NOW))));
        let v = ok(&env.run_with(1, LATER, ["--json", "show", R1]));
        assert_eq!(v["resource"]["title"], "Notes");
        assert_eq!(v["resource"]["catalog_record"]["modified"], LATER);
        assert_eq!(v["resource"]["catalog_record"]["issued"], NOW);
    }

    #[test]
    fn edit_by_uuid_and_by_path() {
        let env = Env::new();
        let path = add_notes(&env);
        ok(&env.run_with(1, LATER, ["--json", "edit", R1, "--title", "One"]));
        let v = ok(&env.run_with(
            1,
            LATER,
            ["--json", "edit", path.to_str().unwrap(), "--title", "Two"],
        ));
        assert_eq!(v["resource"]["title"], "Two");
        assert_eq!(v["resource"]["id"], R1);
    }

    #[test]
    fn edit_with_the_same_title_is_a_no_op() {
        let env = Env::new();
        add_notes(&env);
        let before = env.resources_bytes();
        let out = env.run_with(
            1,
            LATER,
            ["--json", "--dry-run", "edit", R1, "--title", "notes"],
        );
        let v = ok(&out);
        assert_eq!(v["dry_run"], true);
        assert_eq!(v["changes"], serde_json::json!([]));
        let out = env.run_with(1, LATER, ["--json", "edit", R1, "--title", "notes"]);
        assert_eq!(ok(&out)["resource"]["title"], "notes");
        assert_eq!(env.resources_bytes(), before, "nothing was written");
        let v = ok(&env.run_with(1, LATER, ["--json", "show", R1]));
        assert_eq!(v["resource"]["catalog_record"]["modified"], NOW);
    }

    #[test]
    fn edit_without_a_flag_is_a_usage_error() {
        let env = Env::new();
        add_notes(&env);
        let out = env.run(["edit", R1]);
        assert_eq!(out.code, Some(2), "{}", out.stderr);
        assert_eq!(out.stdout, "");
    }

    #[test]
    fn edit_title_must_be_non_empty_without_controls() {
        let env = Env::new();
        add_notes(&env);
        let before = env.resources_bytes();
        for title in ["", "  ", "a\u{7}b"] {
            let out = env.run(["edit", R1, "--title", title]);
            assert_eq!(out.code, Some(2), "{title:?}: {}", out.stderr);
        }
        assert_eq!(env.resources_bytes(), before);
    }

    #[test]
    fn edit_unknown_resource() {
        let env = Env::new();
        add_notes(&env);
        let e = error(
            &env.run([
                "--json",
                "edit",
                "0192f0c4-7b1e-7cc3-9a43-6c1d2e8f4b10",
                "--title",
                "x",
            ]),
            1,
        );
        assert_eq!(e["rule"], "UNKNOWN_RESOURCE");
        assert_eq!(e["details"]["ref"], "0192f0c4-7b1e-7cc3-9a43-6c1d2e8f4b10");
    }

    // ---- P2: hooks are compiled into test builds ----------------------------------------------------

    /// A bare `cargo test` (no `test-hooks`) must fail loudly rather than mint random ids:
    /// here the binary rejects a malformed `HALI_TEST_NOW`, which only a build with the hooks
    /// compiled in does. `make unit` therefore builds with `--features test-hooks`.
    #[test]
    fn test_hooks_are_compiled_in() {
        let env = Env::new();
        let out = env.run_env(&[("HALI_TEST_NOW", "not a time")], 1, ["validate"]);
        assert_eq!(out.code, Some(3), "{}", out.stderr);
        assert!(out.stderr.contains("HALI_TEST_NOW"), "{}", out.stderr);
    }
}
