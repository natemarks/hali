//! Goal tests for `hali schema` through the real binary (#6, spec §9.1).
//!
//! Every run sets HALI_DATA_DIR / XDG_CONFIG_HOME / HOME to temp dirs and clears the rest
//! of the environment, so the real store and config are never read.

mod goal {
    use assert_cmd::Command;
    use std::collections::BTreeMap;
    use std::ffi::OsStr;
    use std::path::Path;
    use tempfile::TempDir;

    const NAMES: [&str; 6] = [
        "concept add",
        "concept list",
        "error",
        "find",
        "schema",
        "validate",
    ];

    struct Out {
        code: Option<i32>,
        stdout: String,
        stderr: String,
    }

    struct Env {
        root: TempDir,
    }

    impl Env {
        fn new() -> Env {
            let root = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(root.path().join("cfg")).unwrap();
            std::fs::create_dir_all(root.path().join("home")).unwrap();
            Env { root }
        }

        fn data(&self) -> std::path::PathBuf {
            self.root.path().join("data")
        }

        fn run<I, S>(&self, args: I) -> Out
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            let out = Command::cargo_bin("hali")
                .unwrap()
                .env_clear()
                .env("XDG_CONFIG_HOME", self.root.path().join("cfg"))
                .env("HOME", self.root.path().join("home"))
                .env("HALI_DATA_DIR", self.data())
                .args(args)
                .output()
                .unwrap();
            Out {
                code: out.status.code(),
                stdout: String::from_utf8(out.stdout).unwrap(),
                stderr: String::from_utf8(out.stderr).unwrap(),
            }
        }
    }

    /// Every entry under `root` with its bytes (None for directories).
    fn tree(root: &Path) -> BTreeMap<String, Option<Vec<u8>>> {
        fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<String, Option<Vec<u8>>>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                let name = path.strip_prefix(root).unwrap().display().to_string();
                if path.is_dir() {
                    out.insert(name, None);
                    walk(&path, root, out);
                } else {
                    out.insert(name, Some(std::fs::read(&path).unwrap()));
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(root, root, &mut out);
        out
    }

    fn one_line_json(s: &str) -> serde_json::Value {
        assert_eq!(
            s.matches('\n').count(),
            1,
            "one compact line expected: {s:?}"
        );
        assert!(s.ends_with('\n'), "{s:?}");
        serde_json::from_str(s).unwrap_or_else(|e| panic!("not JSON ({e}): {s:?}"))
    }

    /// Every `$ref` in the document that does not point inside it.
    fn external_refs(v: &serde_json::Value, found: &mut Vec<String>) {
        match v {
            serde_json::Value::Object(map) => {
                for (k, val) in map {
                    if k == "$ref" {
                        let r = val.as_str().unwrap_or("<non-string>");
                        if !r.starts_with('#') {
                            found.push(r.to_string());
                        }
                    }
                    external_refs(val, found);
                }
            }
            serde_json::Value::Array(items) => items.iter().for_each(|i| external_refs(i, found)),
            _ => {}
        }
    }

    // ---- criterion 1: list ----------------------------------------------------------------

    #[test]
    fn schema_lists_names_json() {
        let env = Env::new();
        let out = env.run(["schema", "--json"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        // The locked snapshot is named without the `goal__` module segment.
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!("cli_schema__schema_list_json", out.stdout);
        });
    }

    #[test]
    fn schema_lists_names_text() {
        let env = Env::new();
        let out = env.run(["schema"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        assert_eq!(
            out.stdout,
            "add\nconcept add\nconcept list\nedit\nerror\nfind\nforget\nschema\nshow\nvalidate\n"
        );
    }

    /// #10: the three new commands each have a bare draft 2020-12 schema, the same in text
    /// and `--json` mode, with no `$id` and no external `$ref`.
    #[test]
    fn new_command_schemas_are_documents() {
        let env = Env::new();
        for name in ["add", "show", "edit"] {
            let text = env.run(["schema", name]);
            let json = env.run(["schema", "--json", name]);
            for (mode, out) in [("text", &text), ("--json", &json)] {
                assert_eq!(out.code, Some(0), "{name} {mode}: {}", out.stderr);
                assert_eq!(out.stderr, "", "{name} {mode}");
            }
            assert_eq!(text.stdout, json.stdout, "{name}");
            let doc = one_line_json(&text.stdout);
            assert_eq!(
                doc["$schema"], "https://json-schema.org/draft/2020-12/schema",
                "{name}"
            );
            assert!(doc.get("$id").is_none(), "{name}");
            let mut refs = Vec::new();
            external_refs(&doc, &mut refs);
            assert!(refs.is_empty(), "{name}: external $ref {refs:?}");
        }
    }

    // ---- criterion 2: one schema is a bare draft 2020-12 document ---------------------------

    #[test]
    fn each_schema_is_a_valid_document() {
        let env = Env::new();
        for name in NAMES {
            let words: Vec<&str> = name.split(' ').collect();
            let mut text_args = vec!["schema"];
            text_args.extend(&words);
            let mut json_args = vec!["schema", "--json"];
            json_args.extend(&words);

            let text = env.run(&text_args);
            let json = env.run(&json_args);
            for (mode, out) in [("text", &text), ("--json", &json)] {
                assert_eq!(out.code, Some(0), "{name} {mode}: {}", out.stderr);
                assert_eq!(out.stderr, "", "{name} {mode}");
            }
            assert_eq!(
                text.stdout, json.stdout,
                "{name}: same document in both modes"
            );

            let doc = one_line_json(&text.stdout);
            let obj = doc
                .as_object()
                .unwrap_or_else(|| panic!("{name}: not an object"));
            assert_eq!(
                obj.get("$schema").and_then(|s| s.as_str()),
                Some("https://json-schema.org/draft/2020-12/schema"),
                "{name}"
            );
            assert!(!obj.contains_key("$id"), "{name}: no $id");
            assert!(
                !obj.contains_key("schema_version") && !obj.contains_key("schemas"),
                "{name}: the bare document is not wrapped in the envelope"
            );
            let mut refs = Vec::new();
            external_refs(&doc, &mut refs);
            assert!(refs.is_empty(), "{name}: external $ref {refs:?}");
        }
    }

    // ---- criterion 3: unknown name --------------------------------------------------------

    #[test]
    fn unknown_name_is_usage_error() {
        let env = Env::new();
        // The command must exist: a known name works.
        let ok = env.run(["schema", "error"]);
        assert_eq!(ok.code, Some(0), "{}", ok.stderr);
        for args in [
            vec!["schema", "nope"],
            vec!["schema", "--json", "nope"],
            vec!["schema", "concept"],
            vec!["schema", "concept", "add", "extra"],
            vec!["schema", "concept", "nope"],
        ] {
            let out = env.run(&args);
            assert_eq!(out.code, Some(2), "{args:?}: {}", out.stderr);
            assert_eq!(out.stdout, "", "{args:?}");
            for name in NAMES {
                assert!(
                    out.stderr.contains(name),
                    "{args:?} lacks {name}: {}",
                    out.stderr
                );
            }
        }
    }

    // ---- criterion 4: no store, no config, no writes ---------------------------------------

    #[test]
    fn schema_needs_no_store() {
        let env = Env::new();
        // A broken store and a broken config: `schema` must read neither.
        std::fs::create_dir_all(env.data()).unwrap();
        std::fs::write(env.data().join("vocabulary.ttl"), "this is not turtle\n").unwrap();
        std::fs::create_dir_all(env.root.path().join("cfg/hali")).unwrap();
        std::fs::write(env.root.path().join("cfg/hali/config.toml"), "= = =\n").unwrap();
        let before = tree(env.root.path());
        for args in [
            vec!["schema"],
            vec!["schema", "--json"],
            vec!["schema", "validate"],
            vec!["--dry-run", "schema"],
            vec!["--dry-run", "schema", "concept", "add"],
            vec!["schema", "--dry-run"],
        ] {
            let out = env.run(&args);
            assert_eq!(out.code, Some(0), "{args:?}: {}", out.stderr);
            assert_eq!(out.stderr, "", "{args:?}");
            assert_eq!(tree(env.root.path()), before, "{args:?} wrote");
        }

        // With no data dir at all it still works and creates nothing.
        let fresh = Env::new();
        let out = fresh.run(["schema", "--json"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert!(
            !fresh.data().exists(),
            "schema must not create the data dir"
        );
    }
}
