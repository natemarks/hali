//! Goal tests for `hali find` through the real binary (#12, spec §6, §9.3, §9.7). Locked
//! golden snapshots sit in tests/snapshots/ (hand-written from the spec and the issue's
//! contract): `cli_find__find_json`, `cli_find__find_json_fields`, `cli_find__find_json_empty`.
//!
//! Every run sets HALI_DATA_DIR / XDG_CONFIG_HOME / HOME to temp dirs and clears the rest of
//! the environment, so the real store and config are never read. The temp root is
//! canonicalised and shown as `/TMP` in captured output. Files get a pinned mtime of
//! 2026-10-20T14:02:11Z plus a sub-second part, as in tests/cli_resource.rs.
//!
//! Fixtures are made by the real `hali add`. One process with `HALI_TEST_ID_SEED=1` mints
//! R1, C1, P for its first file and R2, C2 for its second, so `add notes.txt ideas.txt` gives
//! notes R1 and ideas R2; both have the same `modified` and `issued`, so `find` lists R1 first
//! (every tie breaks on id, ascending). A process with another seed mints other ids, which
//! the tests derive with `SeededIdGen` (its first id is the Resource's).
//!
//! The `hali schema` tests for `find` live here too (tests/cli_schema.rs is not goal-owned):
//! the implementer must also update `NAMES` and the text list in tests/cli_schema.rs and the
//! snapshot tests/snapshots/cli_schema__schema_list_json.snap to include `find`; the test
//! `schema_lists_find` below covers the new list exactly.

mod goal {
    use assert_cmd::Command;
    use hali::core::{IdGen, SeededIdGen};
    use serde_json::Value;
    use std::ffi::OsStr;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime};
    use tempfile::TempDir;

    const R1: &str = "018bcfe5-6801-710a-adec-89025cc1beeb";
    const R2: &str = "018bcfe5-6804-7099-ac6c-d7363ca585e7";

    const NOW: &str = "2026-10-24T09:30:00Z";
    const MTIME_SECS: u64 = 1_792_504_931; // 2026-10-20T14:02:11Z

    const VALID_FIELDS: [&str; 11] = [
        "id",
        "path",
        "title",
        "media_type",
        "editable",
        "missing",
        "stale",
        "types",
        "tags",
        "byte_size",
        "modified",
    ];

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

        /// A file under files/ with the pinned mtime (plus nanoseconds).
        fn file(&self, name: &str, contents: &[u8]) -> PathBuf {
            self.file_at(name, contents, MTIME_SECS)
        }

        fn file_at(&self, name: &str, contents: &[u8], mtime_secs: u64) -> PathBuf {
            let path = self.files().join(name);
            std::fs::write(&path, contents).unwrap();
            pin_mtime(&path, mtime_secs);
            path
        }

        fn run_with<I, S>(&self, seed: u64, now: &str, args: I) -> Out
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
                .env("HALI_TEST_ID_SEED", seed.to_string())
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
            self.run_with(1, NOW, args)
        }

        /// notes.txt and ideas.txt, cataloged by one `add`: ids R1 and R2.
        fn two(&self) -> (PathBuf, PathBuf) {
            let notes = self.file("notes.txt", b"hello\n");
            let ideas = self.file("ideas.txt", b"ideas\n");
            ok(&self.run([
                "--json",
                "add",
                notes.to_str().unwrap(),
                ideas.to_str().unwrap(),
            ]));
            (notes, ideas)
        }

        /// Catalogs one file with its own seed, instant and title; returns its Resource id.
        fn add_one(&self, seed: u64, now: &str, title: &str, path: &Path) -> String {
            ok(&self.run_with(
                seed,
                now,
                ["--json", "add", "--title", title, path.to_str().unwrap()],
            ));
            SeededIdGen::new(seed).next_id()
        }
    }

    fn pin_mtime(path: &Path, secs: u64) {
        let t = SystemTime::UNIX_EPOCH + Duration::new(secs, 987_654_321);
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

    fn snapshot_stdout(name: &str, out: &Out) {
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        // The locked snapshots are named without the `goal__` module segment.
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!(name, out.stdout);
        });
    }

    /// A usage error: exit 2 and nothing on stdout; returns stderr.
    fn usage(out: &Out, what: &str) -> String {
        assert_eq!(out.code, Some(2), "{what}: stderr: {}", out.stderr);
        assert_eq!(out.stdout, "", "{what}");
        assert!(
            !out.stderr.is_empty(),
            "{what}: the error says what is wrong"
        );
        out.stderr.clone()
    }

    fn titles(v: &Value) -> Vec<&str> {
        v["results"]
            .as_array()
            .expect("results is an array")
            .iter()
            .map(|r| r["title"].as_str().expect("title"))
            .collect()
    }

    // ---- criterion 1: paging, --fields, --path-only ----------------------------------------

    #[test]
    fn find_json() {
        let env = Env::new();
        env.two();
        let out = env.run(["--json", "find"]);
        snapshot_stdout("cli_find__find_json", &out);
    }

    #[test]
    fn find_paged_json() {
        let env = Env::new();
        env.two();

        let v = ok(&env.run(["--json", "find", "--limit", "1"]));
        assert_eq!(v["schema_version"], 1);
        assert_eq!(v["results"].as_array().unwrap().len(), 1);
        assert_eq!(v["results"][0]["id"], R1);
        assert_eq!(v["total"], 2);
        assert_eq!(v["truncated"], true);
        assert_eq!(v["next_offset"], 1);

        let v = ok(&env.run(["--json", "find", "--limit", "1", "--offset", "1"]));
        assert_eq!(v["results"].as_array().unwrap().len(), 1);
        assert_eq!(v["results"][0]["id"], R2);
        assert_eq!(v["total"], 2);
        assert_eq!(v["truncated"], false);
        assert_eq!(v["next_offset"], Value::Null);

        // An offset at or past the total is an empty page, not an error.
        for offset in ["2", "3", "1000"] {
            let v = ok(&env.run(["--json", "find", "--offset", offset]));
            assert_eq!(v["results"], serde_json::json!([]), "offset {offset}");
            assert_eq!(v["total"], 2, "offset {offset}");
            assert_eq!(v["truncated"], false, "offset {offset}");
            assert_eq!(v["next_offset"], Value::Null, "offset {offset}");
        }

        // The default limit is 50: both Resources, not truncated.
        let v = ok(&env.run(["--json", "find"]));
        assert_eq!(v["results"].as_array().unwrap().len(), 2);
        assert_eq!(v["truncated"], false);
    }

    #[test]
    fn limit_zero_is_usage_error() {
        let env = Env::new();
        env.two();
        // The command exists and takes a valid limit: only the bad ones are usage errors.
        ok(&env.run(["--json", "find", "--limit", "1"]));
        for limit in ["0", "-1", "x", ""] {
            usage(
                &env.run(["--json", "find", "--limit", limit]),
                &format!("--limit {limit:?}"),
            );
        }
        usage(&env.run(["find", "--limit", "0"]), "--limit 0, text mode");
    }

    #[test]
    fn find_fields_json() {
        let env = Env::new();
        env.two();
        // Given out of record order: printed in record order; `id` is not forced in.
        let out = env.run(["--json", "find", "--fields", "title,id"]);
        snapshot_stdout("cli_find__find_json_fields", &out);

        let v = ok(&env.run(["--json", "find", "--fields", "title"]));
        assert_eq!(v["results"][0], serde_json::json!({"title": "notes"}));
        let v = ok(&env.run(["--json", "find", "--fields", "modified,media_type,path"]));
        let keys: Vec<&String> = v["results"][0].as_object().unwrap().keys().collect();
        assert_eq!(keys, ["path", "media_type", "modified"]);

        // --fields applies to --json only: the text line is the same with or without it.
        let plain = env.run(["find"]);
        let fielded = env.run(["find", "--fields", "title"]);
        assert_eq!(plain.code, Some(0), "{}", plain.stderr);
        assert_eq!(fielded.code, Some(0), "{}", fielded.stderr);
        assert_eq!(plain.stdout, fielded.stdout);
    }

    #[test]
    fn unknown_field_exit_2() {
        let env = Env::new();
        env.two();
        // The command exists and takes a valid value: only the bad ones are usage errors.
        ok(&env.run(["--json", "find", "--fields", "id,title"]));
        for bad in [
            "nope",
            "",
            "id,",
            "id,,title",
            "content",
            "id,content",
            "Title",
        ] {
            let stderr = usage(
                &env.run(["--json", "find", "--fields", bad]),
                &format!("--fields {bad:?}"),
            );
            for name in VALID_FIELDS {
                assert!(
                    stderr.contains(name),
                    "--fields {bad:?}: the error lists the valid names, lacks {name}: {stderr}"
                );
            }
        }
    }

    #[test]
    fn path_only_lists_page_paths() {
        let env = Env::new();
        let (_notes, ideas) = env.two();

        // One stored absolute path per line, nothing on stderr, even when truncated.
        let out = env.run(["find", "--path-only"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stdout, "/TMP/files/notes.txt\n/TMP/files/ideas.txt\n");
        assert_eq!(out.stderr, "");

        let out = env.run(["find", "--path-only", "--limit", "1"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stdout, "/TMP/files/notes.txt\n");
        assert_eq!(out.stderr, "", "no truncation note with --path-only");

        let out = env.run(["find", "--path-only", "--limit", "1", "--offset", "1"]);
        assert_eq!(out.stdout, "/TMP/files/ideas.txt\n");
        assert_eq!(out.stderr, "");

        // It wins over --json, and over --fields; missing Resources are included.
        std::fs::remove_file(&ideas).unwrap();
        for args in [
            vec!["--json", "find", "--path-only"],
            vec!["find", "--path-only", "--json"],
            vec!["--json", "find", "--path-only", "--fields", "title"],
        ] {
            let out = env.run(&args);
            assert_eq!(out.code, Some(0), "{args:?}: {}", out.stderr);
            assert_eq!(
                out.stdout, "/TMP/files/notes.txt\n/TMP/files/ideas.txt\n",
                "{args:?}"
            );
            assert_eq!(out.stderr, "", "{args:?}");
        }

        // No match: nothing at all.
        let out = env.run(["find", "--path-only", "--title", "absent"]);
        assert_eq!(
            (out.code, out.stdout.as_str(), out.stderr.as_str()),
            (Some(0), "", "")
        );
    }

    #[test]
    fn find_text_lines_and_note() {
        let env = Env::new();
        let (_notes, ideas) = env.two();

        let out = env.run(["find"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(
            out.stdout,
            format!("{R1}\tnotes\t/TMP/files/notes.txt\n{R2}\tideas\t/TMP/files/ideas.txt\n")
        );
        assert_eq!(out.stderr, "");

        // A truncated page: the `concept list` note goes to stderr.
        let out = env.run(["find", "--limit", "1"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stdout, format!("{R1}\tnotes\t/TMP/files/notes.txt\n"));
        assert_eq!(out.stderr, "note: showing 1 of 2; next: --offset 1\n");

        // A missing Resource gets a marker.
        std::fs::remove_file(&ideas).unwrap();
        let out = env.run(["find"]);
        assert_eq!(
            out.stdout,
            format!(
                "{R1}\tnotes\t/TMP/files/notes.txt\n{R2}\tideas\t/TMP/files/ideas.txt\tmissing\n"
            )
        );
    }

    // ---- criterion 2: filters --------------------------------------------------------------

    #[test]
    fn title_and_media_type_and() {
        let env = Env::new();
        let notes = env.file("notes.txt", b"hello\n");
        let guide = env.file("guide.txt", b"guide\n");
        let other = env.file("other.txt", b"other\n");
        env.add_one(1, NOW, "Field Notes", &notes);
        env.add_one(2, NOW, "Field Guide", &guide);
        env.add_one(3, NOW, "Other Notes", &other);
        let find = |args: &[&str]| -> Value {
            let mut full = vec!["--json", "find", "--sort", "title"];
            full.extend_from_slice(args);
            ok(&env.run(&full))
        };

        let v = find(&["--title", "FIELD"]);
        assert_eq!(titles(&v), ["Field Guide", "Field Notes"]);
        assert_eq!(v["total"], 2);
        assert_eq!(titles(&find(&["--title", "guide"])), ["Field Guide"]);

        // Every file here is text/plain; the media type matches whole or by wildcard, any case.
        let all = ["Field Guide", "Field Notes", "Other Notes"];
        assert_eq!(titles(&find(&["--media-type", "text/*"])), all);
        assert_eq!(titles(&find(&["--media-type", "TEXT/PLAIN"])), all);
        assert_eq!(find(&["--media-type", "application/pdf"])["total"], 0);
        assert_eq!(find(&["--media-type", "image/*"])["total"], 0);

        // Filters AND, and `total` counts the filtered set.
        let v = find(&["--title", "notes", "--media-type", "text/plain"]);
        assert_eq!(titles(&v), ["Field Notes", "Other Notes"]);
        assert_eq!(v["total"], 2);
        let v = find(&["--title", "notes", "--media-type", "application/pdf"]);
        assert_eq!(v["results"], serde_json::json!([]));
        assert_eq!(v["total"], 0);
        let v = find(&["--title", "field", "--media-type", "text/*", "--limit", "1"]);
        assert_eq!(titles(&v), ["Field Guide"]);
        assert_eq!(v["total"], 2);
        assert_eq!(v["next_offset"], 1);

        // An empty --title matches everything.
        assert_eq!(find(&["--title", ""])["total"], 3);
    }

    #[test]
    fn bad_media_type_exit_2() {
        let env = Env::new();
        env.two();
        // The command exists and takes a valid value: only the bad ones are usage errors.
        ok(&env.run(["--json", "find", "--media-type", "text/*"]));
        for bad in ["*/*", "text/x-*", "text", "", "text/", "/plain", "*/plain"] {
            usage(
                &env.run(["--json", "find", "--media-type", bad]),
                &format!("--media-type {bad:?}"),
            );
        }
        usage(
            &env.run([
                "--json",
                "find",
                "--media-type",
                "text/plain",
                "--media-type",
                "text/html",
            ]),
            "--media-type given twice",
        );
    }

    // ---- criterion 3: sort -----------------------------------------------------------------

    #[test]
    fn sort_flag() {
        let env = Env::new();
        let day = 86_400;
        // Added in this order (issued rises), with modified in another order.
        let banana = env.file_at("banana.txt", b"b\n", MTIME_SECS + 2 * day);
        let apple = env.file_at("apple.txt", b"a\n", MTIME_SECS);
        let cherry = env.file_at("cherry.txt", b"c\n", MTIME_SECS + day);
        env.add_one(1, "2026-10-21T00:00:00Z", "banana", &banana);
        env.add_one(2, "2026-10-22T00:00:00Z", "Apple", &apple);
        env.add_one(3, "2026-10-23T00:00:00Z", "cherry", &cherry);

        let order = |args: &[&str]| -> Vec<String> {
            let mut full = vec!["--json", "find"];
            full.extend_from_slice(args);
            let v = ok(&env.run(&full));
            titles(&v).into_iter().map(str::to_string).collect()
        };
        // The default is modified, descending.
        assert_eq!(order(&[]), ["banana", "cherry", "Apple"]);
        assert_eq!(
            order(&["--sort", "modified"]),
            ["banana", "cherry", "Apple"]
        );
        assert_eq!(order(&["--sort", "added"]), ["cherry", "Apple", "banana"]);
        assert_eq!(order(&["--sort", "title"]), ["Apple", "banana", "cherry"]);

        // The sort applies before paging.
        assert_eq!(
            order(&["--sort", "title", "--limit", "1", "--offset", "1"]),
            ["banana"]
        );

        usage(
            &env.run(["--json", "find", "--sort", "size"]),
            "--sort size",
        );
        usage(
            &env.run(["--json", "find", "--sort", "Title"]),
            "--sort Title",
        );
    }

    // ---- criterion 4: missing and stale ----------------------------------------------------

    #[test]
    fn missing_and_stale_from_stat() {
        let env = Env::new();
        let ok_file = env.file("ok.txt", b"hello\n");
        let gone = env.file("gone.txt", b"hello\n");
        let grown = env.file("grown.txt", b"hello\n");
        let touched = env.file("touched.txt", b"hello\n");
        env.add_one(1, NOW, "ok", &ok_file);
        env.add_one(2, NOW, "gone", &gone);
        env.add_one(3, NOW, "grown", &grown);
        env.add_one(4, NOW, "touched", &touched);

        std::fs::remove_file(&gone).unwrap();
        std::fs::write(&grown, b"hello, a longer line\n").unwrap();
        std::fs::write(&touched, b"HELLO\n").unwrap(); // same size, later mtime

        let flags = |args: &[&str]| -> Vec<(String, bool, bool)> {
            let mut full = vec!["--json", "find", "--sort", "title"];
            full.extend_from_slice(args);
            let v = ok(&env.run(&full));
            v["results"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| {
                    (
                        r["title"].as_str().unwrap().to_string(),
                        r["missing"].as_bool().unwrap(),
                        r["stale"].as_bool().unwrap(),
                    )
                })
                .collect()
        };
        let row = |t: &str, m, s| (t.to_string(), m, s);
        assert_eq!(
            flags(&[]),
            [
                row("gone", true, false),
                row("grown", false, true),
                row("ok", false, false),
                row("touched", false, true),
            ]
        );
        assert_eq!(flags(&["--missing"]), [row("gone", true, false)]);
        assert_eq!(
            flags(&["--stale"]),
            [row("grown", false, true), row("touched", false, true)]
        );

        // The filters apply before paging.
        let v = ok(&env.run([
            "--json", "find", "--stale", "--sort", "title", "--limit", "1",
        ]));
        assert_eq!(v["total"], 2);
        assert_eq!(v["truncated"], true);
        assert_eq!(v["next_offset"], 1);

        // Together they AND, so they match nothing, and that is still exit 0.
        let v = ok(&env.run(["--json", "find", "--missing", "--stale"]));
        assert_eq!(v["results"], serde_json::json!([]));
        assert_eq!(v["total"], 0);
        let out = env.run(["find", "--missing", "--stale"]);
        assert_eq!(
            (out.code, out.stdout.as_str(), out.stderr.as_str()),
            (Some(0), "", "")
        );

        // Text mode marks them.
        let out = env.run(["find", "--stale", "--sort", "title"]);
        let lines: Vec<&str> = out.stdout.lines().collect();
        assert_eq!(lines.len(), 2, "{}", out.stdout);
        assert!(lines[0].ends_with("/grown.txt\tstale"), "{}", lines[0]);
        assert!(lines[1].ends_with("/touched.txt\tstale"), "{}", lines[1]);
    }

    // ---- criterion 5: empty is exit 0, no content, reads never hash ---------------------------

    #[test]
    fn empty_store_exit_0() {
        let env = Env::new();
        let out = env.run(["--json", "find"]);
        snapshot_stdout("cli_find__find_json_empty", &out);
        assert!(!env.data().exists(), "reads never create anything");

        let out = env.run(["find"]);
        assert_eq!(
            (out.code, out.stdout.as_str(), out.stderr.as_str()),
            (Some(0), "", "")
        );
        let out = env.run(["find", "--path-only"]);
        assert_eq!(
            (out.code, out.stdout.as_str(), out.stderr.as_str()),
            (Some(0), "", "")
        );
        assert!(!env.data().exists(), "reads never create anything");
    }

    #[test]
    fn no_match_exit_0() {
        let env = Env::new();
        env.two();
        let v = ok(&env.run(["--json", "find", "--title", "no such title"]));
        assert_eq!(
            v,
            serde_json::json!({"schema_version": 1, "results": [], "total": 0,
                "truncated": false, "next_offset": null})
        );
        let out = env.run(["find", "--media-type", "image/*"]);
        assert_eq!(
            (out.code, out.stdout.as_str(), out.stderr.as_str()),
            (Some(0), "", "")
        );
    }

    #[test]
    fn never_prints_file_content() {
        let env = Env::new();
        let secret = "SECRET-CONTENT-4f9c1b7e";
        let path = env.file("diary.txt", format!("{secret}\nmore\n").as_bytes());
        ok(&env.run(["--json", "add", path.to_str().unwrap()]));
        for args in [
            vec!["find"],
            vec!["--json", "find"],
            vec!["--json", "find", "--fields", "title,path"],
            vec!["find", "--path-only"],
            vec!["--json", "find", "--stale"],
        ] {
            let out = env.run(&args);
            assert_eq!(out.code, Some(0), "{args:?}: {}", out.stderr);
            assert!(!out.stdout.contains(secret), "{args:?}: {}", out.stdout);
            assert!(!out.stderr.contains(secret), "{args:?}: {}", out.stderr);
        }
        let v = ok(&env.run(["--json", "find"]));
        assert!(
            !v["results"][0].as_object().unwrap().contains_key("content"),
            "{v}"
        );
    }

    #[test]
    fn reads_never_hash() {
        let env = Env::new();
        let (notes, _ideas) = env.two();
        let resources = env.data().join("resources.ttl");
        let before = std::fs::read(&resources).unwrap();

        // (a) A file that cannot be read still stats: found, `missing: false`.
        std::fs::set_permissions(&notes, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::File::open(&notes).is_ok() {
            eprintln!("skipped (a): running as a user that can read a mode 000 file (root)");
        } else {
            let v = ok(&env.run(["--json", "find"]));
            assert_eq!(v["results"][0]["id"], R1);
            assert_eq!(v["results"][0]["missing"], false);
            assert_eq!(v["results"][0]["stale"], false);
            assert_eq!(v["total"], 2);
        }
        std::fs::set_permissions(&notes, std::fs::Permissions::from_mode(0o644)).unwrap();

        // (b) New content of the same size under the pinned mtime is not stale: nothing read it.
        std::fs::write(&notes, b"HELLO\n").unwrap();
        pin_mtime(&notes, MTIME_SECS);
        let v = ok(&env.run(["--json", "find"]));
        assert_eq!(v["results"][0]["id"], R1);
        assert_eq!(v["results"][0]["missing"], false);
        assert_eq!(v["results"][0]["stale"], false);

        // And `find` wrote nothing.
        assert_eq!(std::fs::read(&resources).unwrap(), before);
    }

    // ---- criterion 6: --dry-run is accepted and ignored ---------------------------------------

    #[test]
    fn dry_run_is_ignored() {
        let env = Env::new();
        let (_notes, ideas) = env.two();
        std::fs::remove_file(&ideas).unwrap();
        let data_before = std::fs::read(env.data().join("resources.ttl")).unwrap();
        for base in [
            vec!["--json", "find"],
            vec!["find"],
            vec!["find", "--limit", "1"],
            vec!["find", "--path-only"],
            vec!["--json", "find", "--missing", "--fields", "title"],
        ] {
            let plain = env.run(&base);
            assert_eq!(plain.code, Some(0), "{base:?}: {}", plain.stderr);

            let mut before = vec!["--dry-run"];
            before.extend_from_slice(&base);
            let mut after = base.clone();
            after.push("--dry-run");
            for args in [before, after] {
                let dry = env.run(&args);
                assert_eq!(dry.code, plain.code, "{args:?}");
                assert_eq!(dry.stdout, plain.stdout, "{args:?}");
                assert_eq!(dry.stderr, plain.stderr, "{args:?}");
                assert!(!dry.stdout.contains("dry_run"), "{args:?}");
            }
        }
        assert_eq!(
            std::fs::read(env.data().join("resources.ttl")).unwrap(),
            data_before
        );
    }

    // ---- criterion 7: the schema -------------------------------------------------------------

    #[test]
    fn schema_lists_find() {
        let env = Env::new();
        let out = env.run(["schema", "--json"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(
            out.stdout,
            "{\"schema_version\":1,\"schemas\":[\"add\",\"concept add\",\"concept edit\",\
             \"concept list\",\"concept remove\",\"concept rename\",\"concept show\",\"describe\",\"edit\",\"error\",\"find\",\"forget\",\"relate\",\
             \"relation add\",\"relation list\",\"relation remove\",\"schema\",\"show\",\
             \"type add\",\"type list\",\"type remove\",\"unrelate\",\"validate\"]}\n"
        );
        let out = env.run(["schema"]);
        assert_eq!(
            out.stdout,
            "add\nconcept add\nconcept edit\nconcept list\nconcept remove\nconcept rename\nconcept show\ndescribe\nedit\nerror\nfind\nforget\nrelate\nrelation add\nrelation list\nrelation remove\nschema\nshow\ntype add\ntype list\ntype remove\nunrelate\nvalidate\n"
        );
    }

    #[test]
    fn find_schema_is_document() {
        let env = Env::new();
        let text = env.run(["schema", "find"]);
        let json_mode = env.run(["schema", "--json", "find"]);
        for (mode, out) in [("text", &text), ("--json", &json_mode)] {
            assert_eq!(out.code, Some(0), "{mode}: {}", out.stderr);
            assert_eq!(out.stderr, "", "{mode}");
        }
        assert_eq!(text.stdout, json_mode.stdout);
        let doc = json(&text.stdout);
        assert_eq!(
            doc["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
        assert!(doc.get("$id").is_none());
        assert!(
            !text.stdout.contains("\"$ref\":\"http") && !text.stdout.contains("\"$ref\": \"http"),
            "no external $ref"
        );

        // `results.items` is the full record or the narrowed one; both defs are in the document.
        let items = &doc["properties"]["results"]["items"];
        let any_of = items["anyOf"]
            .as_array()
            .expect("results.items is an anyOf");
        let refs: Vec<&str> = any_of.iter().filter_map(|b| b["$ref"].as_str()).collect();
        assert_eq!(refs, ["#/$defs/resource", "#/$defs/resource_fields"]);
        assert!(doc["$defs"]["resource"].is_object());
        assert!(doc["$defs"]["resource_fields"].is_object());
        // The narrowed form has the same properties as the record and requires none.
        let fields = &doc["$defs"]["resource_fields"];
        let mut names: Vec<&str> = fields["properties"]
            .as_object()
            .expect("resource_fields has properties")
            .keys()
            .map(String::as_str)
            .collect();
        names.sort_unstable();
        let mut want = VALID_FIELDS.to_vec();
        want.sort_unstable();
        assert_eq!(names, want);
        assert_eq!(fields["additionalProperties"], false);
        assert!(fields.get("required").is_none());
    }

    // ---- #16: --tag and --exact over tests/fixtures/tags ------------------------------------

    /// The data dir holds tests/fixtures/tags: a hierarchy (animal <- mammal <- dog -> pet,
    /// bird -> animal from extensions.ttl alone, a loop) and nine Resources whose files do not
    /// exist (so every record is `missing`).
    fn tags_env() -> Env {
        let env = Env::new();
        std::fs::create_dir_all(env.data()).unwrap();
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tags");
        for name in ["vocabulary.ttl", "resources.ttl", "extensions.ttl"] {
            std::fs::copy(dir.join(name), env.data().join(name)).unwrap();
        }
        env
    }

    #[test]
    fn find_tag_json() {
        let env = tags_env();
        let out = env.run(["--json", "find", "--tag", "mammal"]);
        snapshot_stdout("cli_find__find_tag_json", &out);
        // The same through the label, in another case.
        let again = env.run(["--json", "find", "--tag", "  MAMMAL "]);
        assert_eq!(again.stdout, out.stdout);
    }

    #[test]
    fn find_tag_exact_json() {
        let env = tags_env();
        let out = env.run(["--json", "find", "--tag", "animal", "--exact"]);
        snapshot_stdout("cli_find__find_tag_exact_json", &out);
        // The flag may come first; without --exact the narrower Concepts count.
        let again = env.run(["--json", "find", "--exact", "--tag", "animal"]);
        assert_eq!(again.stdout, out.stdout);
        let v = ok(&env.run(["--json", "find", "--tag", "animal", "--sort", "title"]));
        assert_eq!(
            titles(&v),
            [
                "animal doc",
                "bird doc",
                "dog doc",
                "dog plant doc",
                "mammal doc"
            ]
        );
    }

    #[test]
    fn find_tag_unknown_and_cycles() {
        let env = tags_env();
        // A broader cycle terminates.
        let v = ok(&env.run(["--json", "find", "--tag", "loop-b"]));
        assert_eq!(titles(&v), ["loop doc"]);
        // The unknown Concept is an error even beside a known one.
        let out = env.run(["--json", "find", "--tag", "animal", "--tag", "nope"]);
        assert_eq!(out.code, Some(1), "{}", out.stderr);
        assert_eq!(out.stdout, "");
        let e: Value = serde_json::from_str(&out.stderr).unwrap();
        assert_eq!(e["error"]["rule"], "UNKNOWN_CONCEPT");
        assert_eq!(e["error"]["details"]["concept"], "nope");
        // --exact alone lists everything, as without it.
        let all = ok(&env.run(["--json", "find"]));
        let exact = ok(&env.run(["--json", "find", "--exact"]));
        assert_eq!(all, exact);
        assert_eq!(exact["total"], 9);
    }
}
