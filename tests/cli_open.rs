//! Goal tests for the `editable` rule and `[open]` config through the real binary (#11, spec
//! §4.1, §5.2). Locked golden snapshots sit in tests/snapshots/ (hand-written from the
//! contract): `cli_open__add_editable_json` and `cli_open__find_open_config_json`.
//!
//! Assumed behaviour (no new API is needed at this seam): `editable` in every Resource record
//! is `open.system` (false) > `open.editor` (true) > built-in never-editable (`text/html`,
//! `image/svg+xml`, `application/postscript`, `application/rtf`, `application/x-desktop`) >
//! "the media type is `text/plain` or has an ancestor with top-level type `text` in the
//! shared-mime-info subclass tree, aliases resolved". The output shape is unchanged.
//!
//! Hermetic: every run clears the environment and sets HOME, XDG_CONFIG_HOME, XDG_STATE_HOME,
//! XDG_DATA_HOME, XDG_DATA_DIRS and HALI_DATA_DIR to temp dirs or to the fixture MIME database
//! tests/fixtures/mime-db (shared-mime-info format: globs2, subclasses, aliases). Needs no
//! outside software. Files get a pinned mtime of 2026-10-20T14:02:11Z. Ids come from
//! `HALI_TEST_ID_SEED=1` (R1, C1, P, R2, C2, ...); a second process uses another seed.

#[cfg(feature = "goal")]
mod goal {
    use assert_cmd::Command;
    use serde_json::Value;
    use std::ffi::OsStr;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime};
    use tempfile::TempDir;

    const NOW: &str = "2026-10-24T09:30:00Z";
    const MTIME_SECS: u64 = 1_792_504_931; // 2026-10-20T14:02:11Z

    struct Env {
        _root: TempDir,
        root: PathBuf,
        /// `XDG_DATA_DIRS`: the fixture database unless a test says otherwise.
        db: PathBuf,
    }

    struct Out {
        code: Option<i32>,
        stdout: String,
        stderr: String,
    }

    fn fixture_db() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mime-db")
    }

    impl Env {
        fn new() -> Env {
            let tmp = tempfile::tempdir().unwrap();
            let root = std::fs::canonicalize(tmp.path()).unwrap();
            for d in ["cfg", "home", "files", "state"] {
                std::fs::create_dir_all(root.join(d)).unwrap();
            }
            Env {
                _root: tmp,
                root,
                db: fixture_db(),
            }
        }

        fn data(&self) -> PathBuf {
            self.root.join("data")
        }

        fn files(&self) -> PathBuf {
            self.root.join("files")
        }

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

        fn run_with<I, S>(&self, seed: u64, args: I) -> Out
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            let mut cmd = Command::cargo_bin("hali").unwrap();
            cmd.env_clear()
                .env("XDG_CONFIG_HOME", self.root.join("cfg"))
                .env("XDG_STATE_HOME", self.root.join("state"))
                .env("XDG_DATA_HOME", self.root.join("xdg-data"))
                .env("XDG_DATA_DIRS", &self.db)
                .env("HOME", self.root.join("home"))
                .env("HALI_DATA_DIR", self.data())
                .env("HALI_TEST_NOW", NOW)
                .env("HALI_TEST_ID_SEED", seed.to_string())
                .timeout(Duration::from_secs(30))
                .args(args);
            let out = cmd.output().unwrap();
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
            self.run_with(1, args)
        }
    }

    fn pin_mtime(path: &Path) {
        let t = SystemTime::UNIX_EPOCH + Duration::new(MTIME_SECS, 987_654_321);
        std::fs::File::open(path).unwrap().set_modified(t).unwrap();
    }

    fn ok(out: &Out) -> Value {
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        assert_eq!(out.stdout.matches('\n').count(), 1, "{:?}", out.stdout);
        serde_json::from_str(&out.stdout)
            .unwrap_or_else(|e| panic!("not JSON ({e}): {out:?}", out = out.stdout))
    }

    fn snapshot_stdout(name: &str, out: &Out) {
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        // The locked snapshots are named without the `goal__` module segment.
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!(name, out.stdout);
        });
    }

    /// `(title, media_type, editable)` of every record in `v["added"]` or `v["results"]`.
    fn rows(v: &Value, key: &str) -> Vec<(String, String, bool)> {
        v[key]
            .as_array()
            .unwrap_or_else(|| panic!("no `{key}` list in {v}"))
            .iter()
            .map(|r| {
                (
                    r["title"].as_str().unwrap().to_string(),
                    r["media_type"].as_str().unwrap().to_string(),
                    r["editable"].as_bool().unwrap(),
                )
            })
            .collect()
    }

    fn editable_of(rows: &[(String, String, bool)], title: &str) -> bool {
        rows.iter()
            .find(|r| r.0 == title)
            .unwrap_or_else(|| panic!("no record titled {title}: {rows:?}"))
            .2
    }

    /// Every `editable` value anywhere in `v`.
    fn editables(v: &Value, out: &mut Vec<bool>) {
        match v {
            Value::Object(m) => {
                for (k, x) in m {
                    if k == "editable" {
                        out.push(x.as_bool().unwrap());
                    } else {
                        editables(x, out);
                    }
                }
            }
            Value::Array(a) => a.iter().for_each(|x| editables(x, out)),
            _ => {}
        }
    }

    // ---- criteria 1, 2, 4: the rule over the fixture database --------------------------------

    /// The locked golden: twelve files, each decided by the fixture database and the built-ins.
    #[test]
    fn add_editable_json() {
        let env = Env::new();
        let mut args = vec!["--json".to_string(), "add".to_string()];
        for name in [
            "notes.txt",
            "README.md",
            "page.html",
            "run.sh",
            "data.json",
            "cfg.toml",
            "paper.pdf",
            "icon.svg",
            "app.desktop",
            "doc.rtf",
            "x.ps",
        ] {
            args.push(env.file(name, b"hello\n").to_str().unwrap().to_string());
        }
        args.push(env.file("blob.bin", b"a\0b").to_str().unwrap().to_string());
        let out = env.run(args);
        snapshot_stdout("cli_open__add_editable_json", &out);
    }

    #[test]
    fn subclasses_of_text_are_editable() {
        let env = Env::new();
        let mut args = vec!["--json".to_string(), "add".to_string()];
        for name in [
            "run.sh",
            "data.json",
            "cfg.toml",
            "conf.yaml",
            "doc.xml",
            "notes.txt",
            "README.md",
            "table.csv",
        ] {
            args.push(env.file(name, b"x: 1\n").to_str().unwrap().to_string());
        }
        let v = ok(&env.run(args));
        let rows = rows(&v, "added");
        // sh -> text/plain; json -> json5 -> text/javascript (two hops to an implicit text/*);
        // toml -> text/plain; x-yaml is an alias of yaml -> text/plain; xml -> text/plain.
        assert_eq!(
            rows.iter().find(|r| r.0 == "run").unwrap().1,
            "application/x-shellscript"
        );
        assert_eq!(
            rows.iter().find(|r| r.0 == "conf").unwrap().1,
            "application/x-yaml"
        );
        for title in [
            "run", "data", "cfg", "conf", "doc", "notes", "README", "table",
        ] {
            assert!(editable_of(&rows, title), "{title}: {rows:?}");
        }
    }

    #[test]
    fn pdf_and_binary_are_not_editable() {
        let env = Env::new();
        let pdf = env.file("paper.pdf", b"%PDF-1.7\n");
        let bin = env.file("blob.bin", b"a\0b");
        let v = ok(&env.run([
            "--json",
            "add",
            pdf.to_str().unwrap(),
            bin.to_str().unwrap(),
        ]));
        let rows = rows(&v, "added");
        assert!(!editable_of(&rows, "paper"), "{rows:?}");
        assert!(!editable_of(&rows, "blob"), "{rows:?}");
    }

    /// A cycle in the subclass tree (`x-loop-a` <-> `x-loop-b`) never crashes or hangs: the
    /// type is simply not editable. (A guard: it passes before the change and must keep passing.)
    #[test]
    fn cyclic_subclasses_do_not_crash() {
        let env = Env::new();
        let path = env.file("x.loop", b"hello\n");
        let v = ok(&env.run(["--json", "add", path.to_str().unwrap()]));
        let rows = rows(&v, "added");
        assert_eq!(rows[0].1, "application/x-loop-a");
        assert!(!rows[0].2, "{rows:?}");
        // and every other read of the record, too
        let v = ok(&env.run(["--json", "find"]));
        assert!(!rows_of(&v)[0].2);
    }

    fn rows_of(v: &Value) -> Vec<(String, String, bool)> {
        rows(v, "results")
    }

    // ---- criterion 3: precedence through the config ------------------------------------------

    /// The locked golden: `[open]` overrides the built-ins and the subclass rule.
    #[test]
    fn find_open_config_json() {
        let env = Env::new();
        env.write_config(
            "[open]\neditor = [\"Text/HTML\", \"image/*\", \"application/pdf\", \"text/csv\"]\n\
             system = [\"text/markdown\", \"text/csv\"]\n",
        );
        let mut args = vec!["--json".to_string(), "add".to_string()];
        for name in [
            "notes.txt",
            "README.md",
            "page.html",
            "paper.pdf",
            "icon.svg",
            "data.csv",
            "run.sh",
            "app.desktop",
        ] {
            args.push(env.file(name, b"hello\n").to_str().unwrap().to_string());
        }
        ok(&env.run(args));
        let out = env.run(["--json", "find"]);
        snapshot_stdout("cli_open__find_open_config_json", &out);
    }

    // ---- criterion 6: validation --------------------------------------------------------------

    #[test]
    fn bad_open_config_exits_3() {
        for text in [
            "[open]\neditor = \"text/plain\"\n",
            "[open]\nsystem = [1]\n",
            "[open]\neditor = [\"\"]\n",
            "[open]\neditor = [\"text\"]\n",
            "[open]\nsystem = [\"*/*\"]\n",
            "[open]\neditor = [\"text/plain; charset=utf-8\"]\n",
            "open = 5\n",
        ] {
            let env = Env::new();
            env.write_config(text);
            let out = env.run(["--json", "find"]);
            assert_eq!(out.code, Some(3), "{text:?}: {}", out.stderr);
            assert_eq!(out.stdout, "", "{text:?}");
            let v: Value = serde_json::from_str(&out.stderr).unwrap();
            assert_eq!(v["error"]["rule"], "CONFIG_INVALID", "{text:?}");
            assert!(
                v["error"]["suggested_fix"]
                    .as_str()
                    .unwrap()
                    .contains("[open] editor and system"),
                "{text:?}: {v}"
            );
            assert!(!env.data().exists(), "{text:?}: something was written");
            // a mutating command is refused the same way, and writes nothing
            let path = env.file("a.txt", b"hello\n");
            let out = env.run(["--json", "add", path.to_str().unwrap()]);
            assert_eq!(out.code, Some(3), "{text:?}: {}", out.stderr);
            assert!(!env.data().exists(), "{text:?}: add wrote");
        }
    }

    /// The `CONFIG_INVALID` fix text lists `[open]` too, for every config error (text mode, so
    /// the hint line is on stderr). The contract only fixes that it mentions `[open] editor and
    /// system`; this pins the sentence the goal chose.
    #[test]
    fn config_invalid_fix_text_lists_open() {
        let want = "hint: Fix the config file; only [user] name, [vocabulary] language, \
                    [files] dir, [scan] exclude, include_hidden and max_size, and [open] \
                    editor and system are read.\n";
        for text in ["[open]\neditor = [1]\n", "x = [", "[scan]\nmax_size = 5\n"] {
            let env = Env::new();
            env.write_config(text);
            let out = env.run(["find"]);
            assert_eq!(out.code, Some(3), "{text:?}: {}", out.stderr);
            assert!(out.stderr.ends_with(want), "{text:?}: {:?}", out.stderr);
        }
    }

    // ---- criterion 7: every record honours [open] --------------------------------------------

    #[test]
    fn config_reaches_every_record() {
        let env = Env::new();
        env.write_config("[open]\nsystem = [\"text/plain\"]\n");
        let a = env.file("a.txt", b"first\n");
        let b = env.file("b.txt", b"second\n");
        let c = env.file("c.txt", b"third\n");
        let (a, b, c) = (
            a.to_str().unwrap(),
            b.to_str().unwrap(),
            c.to_str().unwrap(),
        );
        let files = env.files();
        let files = files.to_str().unwrap();

        let step = |name: &str, seed: u64, args: Vec<&str>, at_least: usize| {
            let out = env.run_with(seed, args);
            let v = ok(&out);
            let mut found = Vec::new();
            editables(&v, &mut found);
            assert!(
                found.len() >= at_least,
                "{name}: {} records in {v}",
                found.len()
            );
            assert!(found.iter().all(|e| !e), "{name}: an editable record: {v}");
            v
        };

        step("add", 1, vec!["--json", "add", a, b], 2);
        // a relation, so that `show` has nested relation records
        let out = env.run_with(2, ["--json", "relate", a, "dcterms:relation", b]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        step("show", 3, vec!["--json", "show", a], 2);
        step("find", 3, vec!["--json", "find"], 2);
        step(
            "edit --dry-run",
            3,
            vec!["--json", "--dry-run", "edit", a, "--title", "A"],
            1,
        );
        step("edit", 3, vec!["--json", "edit", a, "--title", "A"], 1);
        std::fs::write(&a, b"first, changed\n").unwrap();
        step(
            "refresh --dry-run",
            3,
            vec!["--json", "--dry-run", "refresh", a],
            1,
        );
        step("refresh", 3, vec!["--json", "refresh", a], 1);
        step("add --dry-run", 3, vec!["--json", "--dry-run", "add", c], 1);
        step(
            "scan --dry-run",
            3,
            vec!["--json", "--dry-run", "scan", files],
            1,
        );
        step("scan", 4, vec!["--json", "scan", files], 1);
        step(
            "forget --dry-run",
            5,
            vec!["--json", "--dry-run", "forget", "--yes", c],
            1,
        );
        step("forget", 5, vec!["--json", "forget", "--yes", c], 1);
        let moved = env.files().join("b2.txt");
        std::fs::rename(b, &moved).unwrap();
        step(
            "relink --dry-run",
            6,
            vec!["--json", "--dry-run", "relink", b, moved.to_str().unwrap()],
            1,
        );
        step(
            "relink",
            6,
            vec!["--json", "relink", b, moved.to_str().unwrap()],
            1,
        );
    }

    // ---- criterion 8: no database, unknown or empty types --------------------------------------

    #[test]
    fn no_mime_db() {
        let mut env = Env::new();
        // An empty database: no globs, no subclasses.
        env.db = env.root.join("no-db");
        std::fs::create_dir_all(&env.db).unwrap();
        // A hand-written store: the stored types are what they are, whatever detection says.
        let store = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/store");
        std::fs::create_dir_all(env.data()).unwrap();
        for f in ["vocabulary.ttl", "extensions.ttl"] {
            std::fs::copy(store.join(f), env.data().join(f)).unwrap();
        }
        let head = std::fs::read_to_string(store.join("resources.ttl")).unwrap();
        let prefixes: String = head
            .lines()
            .take_while(|l| l.starts_with('@') || l.starts_with('#') || l.is_empty())
            .map(|l| format!("{l}\n"))
            .collect();
        let resource = |n: u32, title: &str, media: Option<&str>| {
            let id = format!("urn:uuid:0192f0c4-0000-7000-8000-0000000008{n:02}");
            let catalog = format!("urn:uuid:0192f0c4-0000-7000-8000-0000000008{:02}", n + 10);
            let media = media
                .map(|m| {
                    format!(
                        "\tdcat:mediaType <http://www.iana.org/assignments/media-types/{m}> ;\n"
                    )
                })
                .unwrap_or_default();
            format!(
                "<{id}#sha256> spdx:algorithm spdx:checksumAlgorithm_sha256 ;\n\
                 \tspdx:checksumValue \"5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03\"^^xsd:hexBinary .\n\
                 <{id}> a hali:Resource ;\n\
                 \tdcterms:modified \"2026-10-20T14:02:11Z\"^^xsd:dateTime ;\n\
                 \tdcterms:title \"{title}\" ;\n\
                 \tspdx:checksum <{id}#sha256> ;\n\
                 \tdcat:byteSize 6 ;\n\
                 {media}\
                 \tdcat:downloadURL <file:///nowhere/{title}> .\n\
                 <{catalog}> a dcat:CatalogRecord ;\n\
                 \tdcterms:issued \"2026-10-21T08:00:00Z\"^^xsd:dateTime ;\n\
                 \tdcterms:modified \"2026-10-21T08:00:00Z\"^^xsd:dateTime ;\n\
                 \tfoaf:primaryTopic <{id}> .\n"
            )
        };
        let text = format!(
            "{prefixes}{}{}{}{}{}",
            resource(1, "foo", Some("text/x-foo")),
            resource(2, "script", Some("application/x-shellscript")),
            resource(3, "bare", None),
            resource(4, "bar", Some("text/x-bar")),
            resource(5, "json", Some("application/json")),
        );
        std::fs::write(env.data().join("resources.ttl"), text).unwrap();
        // The config still applies without a database: `system` turns a text type off and
        // `editor` turns a type only the database would know on.
        env.write_config(
            "[open]\neditor = [\"application/x-shellscript\"]\nsystem = [\"text/x-foo\"]\n",
        );
        let v = ok(&env.run(["--json", "find"]));
        let rows = rows(&v, "results");
        assert_eq!(rows.len(), 5, "{v}");
        assert!(!editable_of(&rows, "foo"), "system entry: {rows:?}");
        assert!(editable_of(&rows, "script"), "editor entry: {rows:?}");
        // Only the implicit text/* rule applies to the rest: text/x-bar yes, a subclass only
        // the database knows (json) no, and no media type at all no.
        assert!(editable_of(&rows, "bar"), "{rows:?}");
        assert!(!editable_of(&rows, "json"), "{rows:?}");
        assert!(!editable_of(&rows, "bare"), "{rows:?}");
        let empty = rows.iter().find(|r| r.0 == "bare").unwrap();
        assert_eq!(empty.1, "");
    }

    // ---- the shared-mime-info database is read defensively (review rows 11-15) ----------------

    /// A run with explicit environment changes: `Some(v)` sets, `None` removes. `cwd` is the
    /// working directory. A run that outlives `secs` is a hang and fails the test.
    fn run_vars(
        env: &Env,
        vars: &[(&str, Option<&Path>)],
        cwd: Option<&Path>,
        secs: u64,
        args: &[&str],
    ) -> Out {
        let mut cmd = Command::cargo_bin("hali").unwrap();
        cmd.env_clear()
            .env("XDG_CONFIG_HOME", env.root.join("cfg"))
            .env("XDG_STATE_HOME", env.root.join("state"))
            .env("XDG_DATA_HOME", env.root.join("xdg-data"))
            .env("XDG_DATA_DIRS", &env.db)
            .env("HOME", env.root.join("home"))
            .env("HALI_DATA_DIR", env.data())
            .env("HALI_TEST_NOW", NOW)
            .env("HALI_TEST_ID_SEED", "1")
            .timeout(Duration::from_secs(secs))
            .args(args);
        for (k, v) in vars {
            match v {
                Some(v) => cmd.env(k, v),
                None => cmd.env_remove(k),
            };
        }
        if let Some(cwd) = cwd {
            cmd.current_dir(cwd);
        }
        let out = cmd
            .output()
            .unwrap_or_else(|e| panic!("`hali {args:?}` did not finish in {secs} s: {e}"));
        assert!(
            out.status.code().is_some(),
            "`hali {args:?}` was killed after {secs} s (a hang?): {:?}",
            out.status
        );
        let root = env.root.to_str().unwrap();
        let clean = |bytes: Vec<u8>| String::from_utf8(bytes).unwrap().replace(root, "/TMP");
        Out {
            code: out.status.code(),
            stdout: clean(out.stdout),
            stderr: clean(out.stderr),
        }
    }

    /// Write a hand-made store whose Resources `t0`, `t1`, ... have exactly these media types.
    fn write_store(env: &Env, types: &[&str]) {
        let store = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/store");
        std::fs::create_dir_all(env.data()).unwrap();
        for f in ["vocabulary.ttl", "extensions.ttl"] {
            std::fs::copy(store.join(f), env.data().join(f)).unwrap();
        }
        let head = std::fs::read_to_string(store.join("resources.ttl")).unwrap();
        let mut text: String = head
            .lines()
            .take_while(|l| l.starts_with('@') || l.starts_with('#') || l.is_empty())
            .map(|l| format!("{l}\n"))
            .collect();
        for (n, media) in types.iter().enumerate() {
            let id = format!("urn:uuid:0192f0c4-0000-7000-8000-00000009{n:04}");
            let catalog = format!("urn:uuid:0192f0c4-0000-7000-8000-0000000a{n:04}");
            text.push_str(&format!(
                "<{id}#sha256> spdx:algorithm spdx:checksumAlgorithm_sha256 ;\n\
                 \tspdx:checksumValue \"5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03\"^^xsd:hexBinary .\n\
                 <{id}> a hali:Resource ;\n\
                 \tdcterms:modified \"2026-10-20T14:02:11Z\"^^xsd:dateTime ;\n\
                 \tdcterms:title \"t{n}\" ;\n\
                 \tspdx:checksum <{id}#sha256> ;\n\
                 \tdcat:byteSize 6 ;\n\
                 \tdcat:mediaType <http://www.iana.org/assignments/media-types/{media}> ;\n\
                 \tdcat:downloadURL <file:///nowhere/t{n}> .\n\
                 <{catalog}> a dcat:CatalogRecord ;\n\
                 \tdcterms:issued \"2026-10-21T08:00:00Z\"^^xsd:dateTime ;\n\
                 \tdcterms:modified \"2026-10-21T08:00:00Z\"^^xsd:dateTime ;\n\
                 \tfoaf:primaryTopic <{id}> .\n"
            ));
        }
        std::fs::write(env.data().join("resources.ttl"), text).unwrap();
    }

    /// `find --json` over the store: the `editable` of `t0`, `t1`, ... in order.
    fn editable_flags(env: &Env, vars: &[(&str, Option<&Path>)], count: usize) -> Vec<bool> {
        let v = ok(&run_vars(
            env,
            vars,
            None,
            20,
            &["--json", "find", "--limit", "2000"],
        ));
        let rows = rows(&v, "results");
        assert_eq!(rows.len(), count, "{v}");
        (0..count)
            .map(|n| editable_of(&rows, &format!("t{n}")))
            .collect()
    }

    fn put(dir: &Path, name: &str, text: &str) {
        std::fs::create_dir_all(dir.join("mime")).unwrap();
        std::fs::write(dir.join("mime").join(name), text).unwrap();
    }

    /// A hostile `subclasses` or `aliases` that is not a plain file must be skipped (treated
    /// as absent), never opened for blocking: the editable rule falls back to the next
    /// directory, here the fixture database, and `find` still exits 0 within seconds.
    #[test]
    fn non_regular_database_files_are_skipped() {
        for name in ["subclasses", "aliases"] {
            for kind in ["fifo", "symlink-to-fifo", "directory", "symlink-to-file"] {
                let env = Env::new();
                // application/pdf is not editable; the fixture knows application/x-shellscript.
                write_store(&env, &["application/pdf", "application/x-shellscript"]);
                let home = env.root.join("hostile");
                let target = home.join("mime").join(name);
                std::fs::create_dir_all(home.join("mime")).unwrap();
                let fifo = env.root.join("a-fifo");
                let mk = |p: &Path| {
                    let st = std::process::Command::new("mkfifo")
                        .arg(p)
                        .status()
                        .unwrap();
                    assert!(st.success());
                };
                match kind {
                    "fifo" => mk(&target),
                    "symlink-to-fifo" => {
                        mk(&fifo);
                        std::os::unix::fs::symlink(&fifo, &target).unwrap();
                    }
                    "directory" => std::fs::create_dir(&target).unwrap(),
                    _ => {
                        // A real file that would make pdf editable, behind a symlink.
                        let real = env.root.join("real-db");
                        std::fs::write(&real, "application/pdf text/plain\n").unwrap();
                        std::os::unix::fs::symlink(&real, &target).unwrap();
                    }
                }
                let out = run_vars(
                    &env,
                    &[("XDG_DATA_HOME", Some(home.as_path()))],
                    None,
                    5,
                    &["--json", "find"],
                );
                let v = ok(&out);
                let rows = rows(&v, "results");
                assert!(
                    !editable_of(&rows, "t0"),
                    "{name} as {kind}: pdf editable: {rows:?}"
                );
                assert!(
                    editable_of(&rows, "t1"),
                    "{name} as {kind}: the next directory was not used: {rows:?}"
                );
            }
        }
    }

    /// A relative `XDG_DATA_HOME` or `HOME` is ignored, like a relative `XDG_DATA_DIRS` entry
    /// and like `paths::xdg_dir`: a database under the working directory is never read.
    #[test]
    fn relative_data_home_and_home_are_ignored() {
        let env = Env::new();
        let db = env.root.join("zzz-db");
        put(&db, "globs2", "50:application/x-zzz:*.zzz\n");
        let cwd = env.root.join("cwd");
        put(
            &cwd.join("rel"),
            "subclasses",
            "application/x-zzz text/plain\n",
        );
        put(
            &cwd.join("relhome/.local/share"),
            "subclasses",
            "application/x-zzz text/plain\n",
        );
        let abs_home = env.root.join("abs-home");
        put(
            &abs_home.join(".local/share"),
            "subclasses",
            "application/x-zzz text/plain\n",
        );
        let abs_data = env.root.join("abs-data");
        put(&abs_data, "subclasses", "application/x-zzz text/plain\n");
        write_store(&env, &["application/x-zzz"]);
        let flags = |vars: &[(&str, Option<&Path>)], cwd: Option<&Path>| {
            let mut all = vec![("XDG_DATA_DIRS", Some(db.as_path()))];
            all.extend_from_slice(vars);
            let v = ok(&run_vars(&env, &all, cwd, 20, &["--json", "find"]));
            rows(&v, "results")[0].2
        };
        // Controls: the same files under absolute paths are read.
        assert!(flags(&[("XDG_DATA_HOME", Some(abs_data.as_path()))], None));
        assert!(flags(
            &[("XDG_DATA_HOME", None), ("HOME", Some(abs_home.as_path()))],
            None
        ));
        // Relative: ignored.
        assert!(
            !flags(&[("XDG_DATA_HOME", Some(Path::new("rel")))], Some(&cwd)),
            "relative XDG_DATA_HOME was read"
        );
        assert!(
            !flags(
                &[
                    ("XDG_DATA_HOME", None),
                    ("HOME", Some(Path::new("relhome")))
                ],
                Some(&cwd)
            ),
            "relative HOME was read"
        );
    }

    /// A database with a huge fan-out from one type reachable from `application/pdf`.
    fn fanout_db(dir: &Path, edges: usize) {
        put(dir, "globs2", "50:application/pdf:*.pdf\n");
        let mut text = String::from("application/pdf application/x-fan\n");
        for i in 0..edges {
            text.push_str(&format!("application/x-fan application/x-f{i}\n"));
        }
        put(dir, "subclasses", &text);
    }

    /// Many Resources of one type over a database with a huge fan-out: the walk is memoized
    /// per media type (or its edges are capped), so `find` is quick. There is no
    /// deterministic cost counter through the binary, so the bound is a generous time that a
    /// debug build doing the work once per Resource misses.
    #[test]
    fn fanout_database_is_cheap_per_resource() {
        let env = Env::new();
        let db = env.root.join("fan-db");
        fanout_db(&db, 80_000);
        write_store(&env, &vec!["application/pdf"; 1500]);
        let t = std::time::Instant::now();
        let flags = editable_flags(&env, &[("XDG_DATA_DIRS", Some(db.as_path()))], 1500);
        let took = t.elapsed();
        assert!(flags.iter().all(|e| !e));
        assert!(took < Duration::from_secs(5), "find took {took:?}");
    }

    /// The same directory listed 50 times in `XDG_DATA_DIRS` is read once.
    #[test]
    fn duplicate_data_dirs_are_read_once() {
        let env = Env::new();
        let db = env.root.join("fan-db");
        fanout_db(&db, 45_000);
        write_store(&env, &["application/pdf"]);
        let dirs = PathBuf::from(std::env::join_paths(vec![db.clone(); 50]).unwrap());
        let t = std::time::Instant::now();
        let flags = editable_flags(&env, &[("XDG_DATA_DIRS", Some(dirs.as_path()))], 1);
        let took = t.elapsed();
        assert_eq!(flags, [false]);
        assert!(took < Duration::from_secs(5), "find took {took:?}");
    }

    /// Fifty different directories whose files are all the same large file (hard links): the
    /// total bytes read are budgeted, so the cost stays bounded however many are listed.
    #[test]
    fn total_database_bytes_are_budgeted() {
        let env = Env::new();
        let first = env.root.join("db0");
        fanout_db(&first, 45_000);
        let mut dirs = vec![first.clone()];
        for i in 1..50 {
            let d = env.root.join(format!("db{i}"));
            std::fs::create_dir_all(d.join("mime")).unwrap();
            for name in ["globs2", "subclasses"] {
                std::fs::hard_link(first.join("mime").join(name), d.join("mime").join(name))
                    .unwrap();
            }
            dirs.push(d);
        }
        write_store(&env, &["application/pdf"]);
        let joined = PathBuf::from(std::env::join_paths(dirs).unwrap());
        let t = std::time::Instant::now();
        let flags = editable_flags(&env, &[("XDG_DATA_DIRS", Some(joined.as_path()))], 1);
        let took = t.elapsed();
        assert_eq!(flags, [false]);
        assert!(took < Duration::from_secs(5), "find took {took:?}");
    }

    /// Five distinct directories, each with its own 4 MiB `subclasses` (sparse, not hard
    /// links): the first four use up the 16 MiB total budget, so the fifth is never read.
    #[test]
    fn total_database_bytes_cut_off_later_directories() {
        use std::io::Write;
        let env = Env::new();
        let mut dirs = Vec::new();
        for i in 0..5 {
            let d = env.root.join(format!("d{i}"));
            std::fs::create_dir_all(d.join("mime")).unwrap();
            let mut f = std::fs::File::create(d.join("mime/subclasses")).unwrap();
            f.set_len(4 * 1024 * 1024).unwrap();
            f.write_all(format!("application/x-d{i} text/plain\n").as_bytes())
                .unwrap();
            dirs.push(d);
        }
        let joined = PathBuf::from(std::env::join_paths(dirs).unwrap());
        let cases = run_cases(
            &env,
            &["d0", "d1", "d2", "d3", "d4"],
            &[("XDG_DATA_DIRS", Some(joined.as_path()))],
        );
        for n in ["d0", "d1", "d2", "d3"] {
            assert!(flag(&cases, n), "{n} is inside the budget: {cases:?}");
        }
        assert!(!flag(&cases, "d4"), "read past the total budget: {cases:?}");
    }

    /// Each type `application/x-<name>` is one Resource, `t<n>` in `names` order.
    fn run_cases(env: &Env, names: &[&str], vars: &[(&str, Option<&Path>)]) -> Vec<(String, bool)> {
        let types: Vec<String> = names.iter().map(|n| format!("application/x-{n}")).collect();
        let refs: Vec<&str> = types.iter().map(String::as_str).collect();
        write_store(env, &refs);
        names
            .iter()
            .map(|n| n.to_string())
            .zip(editable_flags(env, vars, names.len()))
            .collect()
    }

    fn flag(cases: &[(String, bool)], name: &str) -> bool {
        cases.iter().find(|c| c.0 == name).unwrap().1
    }

    #[test]
    fn database_parent_that_is_an_alias_is_resolved() {
        let env = Env::new();
        let db = env.root.join("db");
        put(&db, "subclasses", "application/x-kid application/x-nick\n");
        put(&db, "aliases", "application/x-nick text/plain\n");
        let cases = run_cases(&env, &["kid"], &[("XDG_DATA_DIRS", Some(db.as_path()))]);
        assert!(flag(&cases, "kid"), "{cases:?}");
    }

    #[test]
    fn database_directories_are_merged_user_first() {
        let env = Env::new();
        let user = env.root.join("user");
        let sys = env.root.join("sys");
        put(
            &user,
            "subclasses",
            "application/x-a application/x-b\napplication/x-c application/x-d\n",
        );
        put(
            &user,
            "aliases",
            "application/x-al text/plain\napplication/x-d text/plain\n",
        );
        put(
            &sys,
            "subclasses",
            "application/x-b text/plain\napplication/x-only-sys text/plain\n",
        );
        put(&sys, "aliases", "application/x-al application/pdf\n");
        let cases = run_cases(
            &env,
            &["a", "al", "only-sys", "c"],
            &[
                ("XDG_DATA_HOME", Some(user.as_path())),
                ("XDG_DATA_DIRS", Some(sys.as_path())),
            ],
        );
        // parents merged across directories: a -> b (user), b -> text/plain (system)
        assert!(flag(&cases, "a"), "{cases:?}");
        // the system directory is read after the user one
        assert!(flag(&cases, "only-sys"), "{cases:?}");
        // aliases: the user directory wins (first wins), not the later system one
        assert!(flag(&cases, "al"), "{cases:?}");
        // a parent that the user directory aliases
        assert!(flag(&cases, "c"), "{cases:?}");
    }

    #[test]
    fn database_falls_back_to_home_local_share() {
        let env = Env::new();
        let home = env.root.join("home");
        put(
            &home.join(".local/share"),
            "subclasses",
            "application/x-h text/plain\n",
        );
        let empty = env.root.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        let cases = run_cases(
            &env,
            &["h"],
            &[
                ("XDG_DATA_HOME", None),
                ("HOME", Some(home.as_path())),
                ("XDG_DATA_DIRS", Some(empty.as_path())),
            ],
        );
        assert!(flag(&cases, "h"), "{cases:?}");
    }

    #[test]
    fn malformed_database_lines_are_ignored_and_case_is_folded() {
        let env = Env::new();
        let user = env.root.join("user");
        let sys = env.root.join("sys");
        let junk: &[u8] = b"application/x-three text/plain extra\n\
            garbage\n\
            \n\
            # a comment text/plain\n\
            a/b/c text/plain\n\
            application/x-w text/x$y\n\
            Application/X-Upper TEXT/Plain\n\
            application/x-ok text/plain\n";
        std::fs::create_dir_all(user.join("mime")).unwrap();
        std::fs::write(user.join("mime/subclasses"), junk).unwrap();
        // not UTF-8 at all
        std::fs::write(user.join("mime/aliases"), b"\xff\xfe\x00 \xc3\x28\n").unwrap();
        put(&sys, "subclasses", "application/x-sysok text/plain\n");
        let cases = run_cases(
            &env,
            &["three", "w", "upper", "ok", "sysok"],
            &[
                ("XDG_DATA_HOME", Some(user.as_path())),
                ("XDG_DATA_DIRS", Some(sys.as_path())),
            ],
        );
        assert!(!flag(&cases, "three"), "three words: {cases:?}");
        assert!(!flag(&cases, "w"), "an unsafe type name: {cases:?}");
        assert!(
            flag(&cases, "upper"),
            "database lines are lowercased: {cases:?}"
        );
        assert!(flag(&cases, "ok"), "{cases:?}");
        assert!(flag(&cases, "sysok"), "{cases:?}");
    }

    #[test]
    fn database_file_is_read_up_to_a_cap() {
        use std::os::unix::fs::FileExt;
        let env = Env::new();
        let user = env.root.join("user");
        std::fs::create_dir_all(user.join("mime")).unwrap();
        // 5 MiB sparse file (zero bytes read as garbage lines): a valid line first, one after
        // the 4 MiB cap.
        let path = user.join("mime/subclasses");
        let head = b"application/x-early text/plain\n";
        let tail = b"\napplication/x-late text/plain\n";
        let size = 5 * 1024 * 1024u64;
        let f = std::fs::File::create(&path).unwrap();
        f.set_len(size).unwrap();
        f.write_all_at(head, 0).unwrap();
        f.write_all_at(tail, size - tail.len() as u64).unwrap();
        let empty = env.root.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        let cases = run_cases(
            &env,
            &["early", "late"],
            &[
                ("XDG_DATA_HOME", Some(user.as_path())),
                ("XDG_DATA_DIRS", Some(empty.as_path())),
            ],
        );
        assert!(flag(&cases, "early"), "{cases:?}");
        assert!(!flag(&cases, "late"), "read past the cap: {cases:?}");
    }

    /// A chain of subclasses is followed through at most 256 types (the start included).
    #[test]
    fn long_subclass_chains_stop_at_256_types() {
        let env = Env::new();
        let db = env.root.join("db");
        let mut text = String::new();
        // `in`: 255 application types then text/plain = 256 types; `out`: one more.
        for (name, types) in [("in", 255), ("out", 256)] {
            for i in 0..types - 1 {
                text.push_str(&format!(
                    "application/x-{name}{i} application/x-{name}{}\n",
                    i + 1
                ));
            }
            text.push_str(&format!("application/x-{name}{} text/plain\n", types - 1));
        }
        put(&db, "subclasses", &text);
        let cases = run_cases(
            &env,
            &["in0", "out0"],
            &[("XDG_DATA_DIRS", Some(db.as_path()))],
        );
        assert!(flag(&cases, "in0"), "256 types: {cases:?}");
        assert!(!flag(&cases, "out0"), "257 types: {cases:?}");
    }
}
