//! Goal tests for `hali describe` through the real binary (#19, spec §9.1 **Describe**, §9.4,
//! §9.5, §14.1). Locked golden snapshots sit in tests/snapshots/ (hand-written from the issue's
//! contract): `cli_describe__describe_json`, `cli_describe__describe_brief_json` and
//! `cli_describe__describe_json_empty` (stdout).
//!
//! Every run sets HALI_DATA_DIR (unless a test says otherwise) / XDG_CONFIG_HOME / HOME to temp
//! dirs and clears the rest of the environment, so the real store and config are never read.
//! The temp root is canonicalised and shown as `/TMP` in captured output. The store is
//! tests/fixtures/relations (see tests/core_relation.rs), copied into `<root>/data`; the
//! snapshots show its path as `/TMP/data`. `describe` mints no id, so one HALI_TEST_ID_SEED
//! serves every run.
//!
//! Wording the contract leaves open and these tests fix (change them, with the human, if the
//! wording should differ):
//! - the text line of a Type or a relation kind with no parent ends in a tab (an empty parent
//!   list leaves nothing after the last tab), as `type list` and `relation list` print it;
//! - a `concept` text line carries its label even when empty (`concept<TAB>x<TAB>`);
//! - the text values are escaped by `printable`, so an ESC in a label prints as `\u{1b}`, and
//!   the store path is escaped the same way in text mode (JSON keeps the raw string);
//! - a data dir that cannot be resolved (no `HALI_DATA_DIR`, `XDG_DATA_HOME` or `HOME`) is
//!   `STORE_IO` with `details.path` null, exit 3, as for every other command;
//! - a non-UTF-8 `HALI_DATA_DIR` is not testable: the existing `cli::env` ignores such a value
//!   (it would fall back to XDG), and `src/paths` is out of scope.
//!
//! The `hali schema` list is covered here too (tests/cli_schema.rs is not goal-owned): the
//! implementer must also update `NAMES` and the text list in tests/cli_schema.rs, the name
//! list of tests/core_schema.rs and the snapshot tests/snapshots/cli_schema__schema_list_json.snap
//! to include `describe`; the test `schema_lists_describe` below covers the new list exactly.

#[cfg(feature = "goal")]
mod goal {
    use assert_cmd::Command;
    use serde_json::{Value, json};
    use std::collections::BTreeMap;
    use std::ffi::OsStr;
    use std::os::unix::fs::symlink;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime};
    use tempfile::TempDir;

    const NOW: &str = "2026-10-24T09:30:00Z";
    const KEYS: [&str; 7] = [
        "schema_version",
        "core_version",
        "store",
        "counts",
        "concepts",
        "types",
        "relation_kinds",
    ];

    struct Env {
        _root: TempDir,
        root: PathBuf,
        data: PathBuf,
    }

    struct Out {
        code: Option<i32>,
        stdout: String,
        stderr: String,
    }

    #[derive(Debug, PartialEq)]
    enum Entry {
        Dir(SystemTime),
        File(SystemTime, Vec<u8>),
        Other(SystemTime),
    }

    impl Env {
        fn base() -> Env {
            let tmp = tempfile::tempdir().unwrap();
            let root = std::fs::canonicalize(tmp.path()).unwrap();
            for d in ["cfg", "home"] {
                std::fs::create_dir_all(root.join(d)).unwrap();
            }
            let data = root.join("data");
            Env {
                _root: tmp,
                root,
                data,
            }
        }

        /// The `relations` fixture in the data dir.
        fn new() -> Env {
            let env = Env::empty();
            let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/relations");
            for entry in std::fs::read_dir(&fixture).unwrap() {
                let entry = entry.unwrap();
                std::fs::copy(entry.path(), env.data.join(entry.file_name())).unwrap();
            }
            env
        }

        /// An empty data dir.
        fn empty() -> Env {
            let env = Env::base();
            std::fs::create_dir_all(&env.data).unwrap();
            env
        }

        /// No data dir at all.
        fn absent() -> Env {
            Env::base()
        }

        /// Appends Turtle text to a file of the data dir (the fixture's prefixes apply).
        fn append(&self, file: &str, text: &str) {
            let path = self.data.join(file);
            let mut old = std::fs::read_to_string(&path).unwrap();
            old.push_str(text);
            std::fs::write(path, old).unwrap();
        }

        fn write_config(&self, toml: &str) {
            let dir = self.root.join("cfg/hali");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("config.toml"), toml).unwrap();
        }

        /// A command with HALI_DATA_DIR set to `data` and nothing else from the environment.
        fn cmd_with(&self, data: Option<&OsStr>) -> Command {
            let mut cmd = Command::cargo_bin("hali").unwrap();
            cmd.env_clear()
                .env("XDG_CONFIG_HOME", self.root.join("cfg"))
                .env("HOME", self.root.join("home"))
                .env("HALI_TEST_NOW", NOW)
                .env("HALI_TEST_ID_SEED", "1")
                .timeout(Duration::from_secs(30));
            if let Some(data) = data {
                cmd.env("HALI_DATA_DIR", data);
            }
            cmd
        }

        fn clean(&self, bytes: Vec<u8>) -> String {
            String::from_utf8(bytes)
                .unwrap()
                .replace(self.root.to_str().unwrap(), "/TMP")
        }

        fn out(&self, out: std::process::Output) -> Out {
            Out {
                code: out.status.code(),
                stdout: self.clean(out.stdout),
                stderr: self.clean(out.stderr),
            }
        }

        fn run<I, S>(&self, args: I) -> Out
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            self.run_in(self.data.as_os_str(), args)
        }

        fn run_in<I, S>(&self, data: &OsStr, args: I) -> Out
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            let out = self.cmd_with(Some(data)).args(args).output().unwrap();
            self.out(out)
        }

        /// Everything under the root: names, kinds, mtimes and file bytes.
        fn tree(&self) -> BTreeMap<String, Entry> {
            fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<String, Entry>) {
                for entry in std::fs::read_dir(dir).unwrap() {
                    let path = entry.unwrap().path();
                    let meta = std::fs::symlink_metadata(&path).unwrap();
                    let name = path.strip_prefix(root).unwrap().display().to_string();
                    let mtime = meta.modified().unwrap();
                    if meta.is_dir() {
                        out.insert(name, Entry::Dir(mtime));
                        walk(&path, root, out);
                    } else if meta.is_file() {
                        out.insert(name, Entry::File(mtime, std::fs::read(&path).unwrap()));
                    } else {
                        out.insert(name, Entry::Other(mtime));
                    }
                }
            }
            let mut out = BTreeMap::new();
            walk(&self.root, &self.root, &mut out);
            out
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

    /// The error envelope of a failed run: exit 3, nothing on stdout.
    fn failed(out: &Out, rule: &str) -> Value {
        assert_eq!(out.code, Some(3), "stderr: {}", out.stderr);
        assert_eq!(out.stdout, "");
        let v = json(&out.stderr);
        assert_eq!(v["error"]["rule"], rule, "{}", out.stderr);
        v
    }

    fn snapshot_stdout(name: &str, out: &Out) {
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        // The locked snapshots are named without the `goal__` module segment.
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!(name, out.stdout);
        });
    }

    fn keys(v: &Value) -> Vec<&str> {
        v.as_object().unwrap().keys().map(String::as_str).collect()
    }

    /// The `results` of a list command run with no paging.
    fn listed(env: &Env, words: &[&str]) -> Value {
        let mut args: Vec<&str> = words.to_vec();
        args.extend(["list", "--json", "--limit", "1000"]);
        let v = ok(&env.run(args));
        assert_eq!(v["truncated"], false);
        v["results"].clone()
    }

    const FULL_TEXT: &str = "core_version\t0.1.0\n\
        store\t/TMP/data\n\
        resources\t5\n\
        concepts\t1\n\
        types\t1\n\
        relation_kinds\t10\n\
        relations\t6\n";
    const LISTS_TEXT: &str = "concept\tml\tml\n\
        type\tNote\tNote\t\n\
        relation_kind\tannotates\tannotates\tdcterms:references\n\
        relation_kind\tdigestOf\tdigestOf\tdcterms:relation\n\
        relation_kind\texcerptOf\texcerpt of\tprov:wasDerivedFrom\n\
        relation_kind\tdcterms:isPartOf\tis part of\tdcterms:relation\n\
        relation_kind\tquotes\tquotes\tdcterms:references\n\
        relation_kind\tdcterms:references\treferences\tdcterms:relation\n\
        relation_kind\tdcterms:relation\trelation\t\n\
        relation_kind\tsketch\tsketch\tdcterms:isPartOf\n\
        relation_kind\tsummarises\tsummarises\tannotates\n\
        relation_kind\tprov:wasDerivedFrom\twas derived from\t\n";

    // ---- criterion 1: the full object --------------------------------------------------------

    #[test]
    fn describe_json() {
        let env = Env::new();
        let out = env.run(["describe", "--json"]);
        snapshot_stdout("cli_describe__describe_json", &out);
    }

    #[test]
    fn describe_lists_match_list_commands() {
        let env = Env::new();
        let v = ok(&env.run(["describe", "--json"]));
        assert_eq!(keys(&v), KEYS);
        assert_eq!(v["schema_version"], 1);
        assert_eq!(v["core_version"], "0.1.0");
        assert_eq!(v["store"], "/TMP/data");
        assert_eq!(
            keys(&v["counts"]),
            [
                "resources",
                "concepts",
                "types",
                "relation_kinds",
                "relations"
            ]
        );
        assert_eq!(v["concepts"], listed(&env, &["concept"]));
        assert_eq!(v["types"], listed(&env, &["type"]));
        assert_eq!(v["relation_kinds"], listed(&env, &["relation"]));
        // The counts are the totals of the lists, and `find`'s unfiltered total.
        assert_eq!(
            v["counts"]["concepts"],
            v["concepts"].as_array().unwrap().len()
        );
        assert_eq!(v["counts"]["types"], v["types"].as_array().unwrap().len());
        assert_eq!(
            v["counts"]["relation_kinds"],
            v["relation_kinds"].as_array().unwrap().len()
        );
        let found = ok(&env.run(["find", "--json"]));
        assert_eq!(v["counts"]["resources"], found["total"]);
    }

    // ---- criterion 2: --brief ----------------------------------------------------------------

    #[test]
    fn describe_brief_json() {
        let env = Env::new();
        let out = env.run(["describe", "--brief", "--json"]);
        snapshot_stdout("cli_describe__describe_brief_json", &out);
    }

    #[test]
    fn brief_is_full_minus_lists() {
        let env = Env::new();
        let mut full = ok(&env.run(["describe", "--json"]));
        let brief = ok(&env.run(["describe", "--json", "--brief"]));
        for gone in ["concepts", "types", "relation_kinds"] {
            assert!(brief.get(gone).is_none(), "{gone} in brief");
            full.as_object_mut().unwrap().remove(gone);
        }
        assert_eq!(brief, full);
        assert_eq!(keys(&brief), keys(&full));
        // No paging keys, in either form.
        for k in ["limit", "offset", "truncated", "total", "next_offset"] {
            assert!(brief.get(k).is_none(), "{k}");
        }
    }

    // ---- criterion 3: an absent or empty store -----------------------------------------------

    #[test]
    fn describe_absent_store() {
        let env = Env::absent();
        let before = env.tree();
        let out = env.run(["describe", "--json"]);
        snapshot_stdout("cli_describe__describe_json_empty", &out);
        assert!(!env.data.exists(), "the data dir was created");
        assert_eq!(env.tree(), before);
        // The brief form of an absent store, and the text form: exit 0, empty stderr.
        let brief = ok(&env.run(["describe", "--brief", "--json"]));
        assert_eq!(
            brief["counts"],
            json!({"resources": 0, "concepts": 0, "types": 0, "relation_kinds": 4, "relations": 0})
        );
        assert!(brief.get("concepts").is_none());
        let text = env.run(["describe"]);
        assert_eq!(text.code, Some(0), "{}", text.stderr);
        assert_eq!(text.stderr, "");
        assert!(text.stdout.starts_with(
            "core_version\t0.1.0\nstore\t/TMP/data\nresources\t0\nconcepts\t0\ntypes\t0\n\
             relation_kinds\t4\nrelations\t0\n"
        ));
        assert!(!env.data.exists(), "the data dir was created");
        assert_eq!(env.tree(), before);
    }

    #[test]
    fn describe_empty_dir_matches_absent() {
        let absent = Env::absent();
        let empty = Env::empty();
        let before = empty.tree();
        for args in [
            &["describe", "--json"][..],
            &["describe", "--brief", "--json"][..],
            &["describe"][..],
            &["describe", "--brief"][..],
        ] {
            let a = absent.run(args);
            let e = empty.run(args);
            assert_eq!(a.code, Some(0), "{}", a.stderr);
            assert_eq!(e.code, Some(0), "{}", e.stderr);
            assert_eq!(a.stderr, "");
            assert_eq!(e.stderr, "");
            assert_eq!(a.stdout, e.stdout, "{args:?}");
        }
        assert_eq!(empty.tree(), before, "an empty dir gained a file");
        assert_eq!(
            std::fs::read_dir(&empty.data).unwrap().count(),
            0,
            "an empty dir gained a file"
        );
        let snap = empty.run(["describe", "--json"]);
        snapshot_stdout("cli_describe__describe_json_empty", &snap);
    }

    #[test]
    fn describe_writes_nothing() {
        let env = Env::new();
        let before = env.tree();
        for args in [
            &["describe", "--json"][..],
            &["describe", "--brief"][..],
            &["describe"][..],
        ] {
            let out = env.run(args);
            assert_eq!(out.code, Some(0), "{}", out.stderr);
        }
        assert_eq!(env.tree(), before, "describe changed the tree");
        let names: Vec<String> = std::fs::read_dir(&env.data)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names.len(), 3, "{names:?}");
    }

    #[test]
    fn describe_hand_edited_store_is_described_as_it_is() {
        // A Concept without a label would fail SHACL; describe runs none, and keeps the empty
        // label `concept list` gives it.
        let env = Env::new();
        env.append(
            "vocabulary.ttl",
            "\nhali:nolabel a skos:Concept ; skos:inScheme hali:topics .\n",
        );
        let v = ok(&env.run(["describe", "--json"]));
        assert_eq!(v["counts"]["concepts"], 2);
        assert!(
            v["concepts"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["id"] == "nolabel" && c["label"] == ""),
            "{v}"
        );
        assert_eq!(v["concepts"], listed(&env, &["concept"]));
    }

    // ---- criterion 6: the store path ---------------------------------------------------------

    #[test]
    fn store_is_hali_data_dir() {
        // As given: relative, with a trailing slash, a symlink, an absent path.
        let env = Env::new();
        std::fs::create_dir_all(env.root.join("elsewhere")).unwrap();
        symlink(&env.data, env.root.join("link")).unwrap();
        let abs = env.data.to_str().unwrap().to_string();
        let cases = [
            (
                "relative/./data/../data".to_string(),
                "relative/./data/../data".to_string(),
            ),
            (format!("{abs}/"), "/TMP/data/".to_string()),
            (
                env.root.join("link").to_str().unwrap().to_string(),
                "/TMP/link".to_string(),
            ),
            (
                env.root
                    .join("elsewhere/nope")
                    .to_str()
                    .unwrap()
                    .to_string(),
                "/TMP/elsewhere/nope".to_string(),
            ),
        ];
        for (given, shown) in cases {
            let v = ok(&env.run_in(OsStr::new(&given), ["describe", "--json", "--brief"]));
            assert_eq!(v["store"], shown, "HALI_DATA_DIR={given}");
        }
        // The relative one names no existing dir (the tests run in the package root): empty.
        assert!(!Path::new("relative").exists());
        let v = ok(&env.run_in(
            OsStr::new("relative/data"),
            ["describe", "--json", "--brief"],
        ));
        assert_eq!(v["counts"]["resources"], 0);
        // The symlink reaches the fixture.
        let v = ok(&env.run_in(env.root.join("link").as_os_str(), ["describe", "--json"]));
        assert_eq!(v["counts"]["resources"], 5);
    }

    #[test]
    fn store_falls_back_to_xdg_data_home() {
        let env = Env::absent();
        let xdg = env.root.join("xdg");
        let out = env
            .cmd_with(None)
            .env("XDG_DATA_HOME", &xdg)
            .args(["describe", "--json", "--brief"])
            .output()
            .unwrap();
        let v = ok(&env.out(out));
        assert_eq!(v["store"], "/TMP/xdg/hali");
        // ...and with neither, $HOME/.local/share/hali.
        let out = env
            .cmd_with(None)
            .args(["describe", "--json", "--brief"])
            .output()
            .unwrap();
        let v = ok(&env.out(out));
        assert_eq!(v["store"], "/TMP/home/.local/share/hali");
        assert!(!xdg.exists() && !env.root.join("home/.local").exists());
    }

    // ---- criterion 7: text -------------------------------------------------------------------

    #[test]
    fn describe_text() {
        let env = Env::new();
        let out = env.run(["describe"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        assert_eq!(out.stdout, format!("{FULL_TEXT}{LISTS_TEXT}"));
    }

    #[test]
    fn describe_brief_text() {
        let env = Env::new();
        let out = env.run(["describe", "--brief"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        assert_eq!(out.stdout, FULL_TEXT);
    }

    #[test]
    fn describe_text_escapes_controls() {
        let env = Env::new();
        env.append(
            "extensions.ttl",
            "\nhali:bad a owl:ObjectProperty ;\n    rdfs:label \"bad\\u001Blabel\"@en ;\n    \
             rdfs:subPropertyOf dcterms:references .\n",
        );
        let out = env.run(["describe"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert!(!out.stdout.contains('\u{1b}'), "{:?}", out.stdout);
        assert!(
            out.stdout
                .contains("relation_kind\tbad\tbad\\u{1b}label\tdcterms:references\n"),
            "{:?}",
            out.stdout
        );
        assert!(out.stdout.contains("relation_kinds\t11\n"));
        // JSON keeps the raw character, escaped as JSON does.
        let v = ok(&env.run(["describe", "--json"]));
        assert!(
            v["relation_kinds"]
                .as_array()
                .unwrap()
                .iter()
                .any(|k| k["id"] == "bad" && k["label"] == "bad\u{1b}label")
        );
        assert_eq!(v["relation_kinds"], listed(&env, &["relation"]));
    }

    #[test]
    fn describe_text_escapes_store_path() {
        let env = Env::absent();
        let odd = env.root.join("d\u{1b}[31mata");
        let out = env.run_in(odd.as_os_str(), ["describe", "--brief"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert!(!out.stdout.contains('\u{1b}'), "{:?}", out.stdout);
        assert!(
            out.stdout.contains("store\t/TMP/d\\u{1b}[31mata\n"),
            "{:?}",
            out.stdout
        );
        assert!(!odd.exists());
    }

    // ---- criterion 8: the configured language ------------------------------------------------

    #[test]
    fn describe_uses_configured_language() {
        let env = Env::new();
        env.write_config("[vocabulary]\nlanguage = \"fr\"\n");
        let v = ok(&env.run(["describe", "--json"]));
        let label = |id: &str| {
            v["relation_kinds"]
                .as_array()
                .unwrap()
                .iter()
                .find(|k| k["id"] == id)
                .unwrap()["label"]
                .clone()
        };
        assert_eq!(label("annotates"), "annote");
        // No @fr label: the id.
        assert_eq!(label("summarises"), "summarises");
        assert_eq!(label("dcterms:isPartOf"), "dcterms:isPartOf");
        assert_eq!(v["concepts"][0]["id"], "ml");
        assert_eq!(v["concepts"][0]["label"], "");
        assert_eq!(v["types"][0]["label"], "Note");
        assert_eq!(v["concepts"], listed(&env, &["concept"]));
        assert_eq!(v["types"], listed(&env, &["type"]));
        assert_eq!(v["relation_kinds"], listed(&env, &["relation"]));
        assert_eq!(v["counts"]["relation_kinds"], 10);
        let text = env.run(["describe"]);
        assert!(
            text.stdout
                .contains("relation_kind\tannotates\tannote\tdcterms:references\n"),
            "{}",
            text.stdout
        );
        assert!(text.stdout.contains("concept\tml\t\n"), "{}", text.stdout);
    }

    // ---- criterion 9: read-only --------------------------------------------------------------

    #[test]
    fn describe_dry_run_and_yes_identical() {
        let env = Env::new();
        let before = env.tree();
        for extra in [&[][..], &["--brief"][..]] {
            for json_flag in [&["--json"][..], &[][..]] {
                let mut plain = vec!["describe"];
                plain.extend_from_slice(extra);
                plain.extend_from_slice(json_flag);
                let base = env.run(plain.clone());
                assert_eq!(base.code, Some(0), "{}", base.stderr);
                assert_eq!(base.stderr, "");
                let mut variants: Vec<Vec<&str>> = Vec::new();
                for flags in [
                    &["--dry-run"][..],
                    &["--yes"][..],
                    &["--dry-run", "--yes"][..],
                ] {
                    let mut front = flags.to_vec();
                    front.extend_from_slice(&plain);
                    variants.push(front);
                    let mut back = plain.clone();
                    back.extend_from_slice(flags);
                    variants.push(back);
                }
                for args in variants {
                    let out = env.run(args.clone());
                    assert_eq!(out.code, base.code, "{args:?}");
                    assert_eq!(out.stdout, base.stdout, "{args:?}");
                    assert_eq!(out.stderr, "", "{args:?}");
                    if out.stdout.contains("\"schema_version\"") {
                        assert!(!out.stdout.contains("dry_run"), "{args:?}");
                        assert!(!out.stdout.contains("\"changes\""), "{args:?}");
                    }
                }
            }
        }
        assert_eq!(env.tree(), before);
    }

    // ---- criterion 10: existing errors, no new rule ------------------------------------------

    #[test]
    fn describe_unparseable_store() {
        for file in ["vocabulary.ttl", "resources.ttl", "extensions.ttl"] {
            let env = Env::new();
            std::fs::write(env.data.join(file), "this is not turtle {{{\n").unwrap();
            let before = env.tree();
            let out = env.run(["describe", "--json"]);
            let v = failed(&out, "STORE_UNPARSEABLE");
            assert_eq!(v["schema_version"], 1);
            assert_eq!(v["error"]["details"]["file"], file);
            assert_eq!(env.tree(), before);
            // Text mode: the same exit code, one `error:` line on stderr.
            let out = env.run(["describe"]);
            assert_eq!(out.code, Some(3), "{}", out.stderr);
            assert_eq!(out.stdout, "");
            assert!(out.stderr.starts_with("error: "), "{}", out.stderr);
        }
    }

    #[test]
    fn describe_data_dir_is_a_file() {
        let env = Env::absent();
        std::fs::write(&env.data, "not a directory").unwrap();
        let before = env.tree();
        let v = failed(&env.run(["describe", "--json"]), "STORE_IO");
        assert_eq!(v["error"]["details"]["path"], "/TMP/data");
        let out = env.run(["describe", "--brief"]);
        assert_eq!(out.code, Some(3), "{}", out.stderr);
        assert_eq!(out.stdout, "");
        assert_eq!(env.tree(), before);
    }

    #[test]
    fn describe_data_dir_unresolvable() {
        let env = Env::absent();
        let out = Command::cargo_bin("hali")
            .unwrap()
            .env_clear()
            .env("HALI_TEST_NOW", NOW)
            .env("HALI_TEST_ID_SEED", "1")
            .args(["describe", "--json"])
            .output()
            .unwrap();
        let v = failed(&env.out(out), "STORE_IO");
        assert!(v["error"]["details"]["path"].is_null(), "{v}");
    }

    #[test]
    fn describe_invalid_config() {
        let env = Env::new();
        env.write_config("this is = = not toml");
        let before = env.tree();
        failed(&env.run(["describe", "--json"]), "CONFIG_INVALID");
        assert_eq!(env.tree(), before);
    }

    #[test]
    fn describe_usage_errors() {
        let env = Env::new();
        // The command exists, and takes no paging flags and no argument.
        assert_eq!(env.run(["describe"]).code, Some(0));
        for args in [
            &["describe", "--limit", "3"][..],
            &["describe", "--offset", "1"][..],
            &["describe", "extra"][..],
        ] {
            let out = env.run(args);
            assert_eq!(out.code, Some(2), "{args:?}: {}", out.stderr);
            assert_eq!(out.stdout, "");
            assert!(out.stderr.contains("unexpected argument"), "{}", out.stderr);
        }
    }

    // ---- criterion 11: the schema list -------------------------------------------------------

    #[test]
    fn schema_lists_describe() {
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
        let out = env.run(["schema", "describe"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        let doc = json(&out.stdout);
        assert!(doc.is_object());
        assert!(doc.get("$id").is_none());
        // The same document with --json.
        let out2 = env.run(["schema", "describe", "--json"]);
        assert_eq!(out2.stdout, out.stdout);
    }
}
