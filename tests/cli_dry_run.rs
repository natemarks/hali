//! Goal tests for `--dry-run` through the real binary (#8).
//!
//! Assumed API: the global flag `--dry-run` (before or after the subcommand); on
//! `concept add --json` the payload gains `"dry_run":true` and `"changes":[{op,triple}]`
//! (see tests/snapshots/cli_dry_run__*.snap, hand-written from docs/spec.md §9.5 and the plan).
//!
//! Every run sets HALI_DATA_DIR / XDG_CONFIG_HOME / HOME to temp dirs and clears the rest
//! of the environment, so the real store and config are never read.

mod support;

use assert_cmd::Command;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

struct Env {
    _root: TempDir,
    data: PathBuf,
    cfg: PathBuf,
    home: PathBuf,
}

#[derive(Debug, PartialEq)]
struct Out {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// What is in the data dir: nothing (absent), a file, or the entries with their bytes.
#[derive(Debug, PartialEq)]
enum State {
    Absent,
    File(Vec<u8>),
    Dir(Vec<(String, Vec<u8>)>),
}

impl Env {
    /// The data dir does not exist yet.
    fn new() -> Env {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data");
        let cfg = root.path().join("cfg");
        let home = root.path().join("home");
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        Env {
            _root: root,
            data,
            cfg,
            home,
        }
    }

    /// Copies every file of tests/fixtures/<name> into the data dir.
    fn with_fixture(name: &str) -> Env {
        let env = Env::new();
        std::fs::create_dir_all(&env.data).unwrap();
        for entry in std::fs::read_dir(fixture(name)).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(entry.path(), env.data.join(entry.file_name())).unwrap();
        }
        env
    }

    fn run<I, S>(&self, args: I) -> Out
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let out = Command::cargo_bin("hali")
            .unwrap()
            .env_clear()
            .env("XDG_CONFIG_HOME", &self.cfg)
            .env("HOME", &self.home)
            .env("HALI_DATA_DIR", &self.data)
            .args(args)
            .output()
            .unwrap();
        Out {
            code: out.status.code(),
            stdout: String::from_utf8(out.stdout).unwrap(),
            stderr: String::from_utf8(out.stderr)
                .unwrap()
                .replace(self.data.to_str().unwrap(), "[DATA_DIR]"),
        }
    }

    fn state(&self) -> State {
        if !self.data.exists() {
            return State::Absent;
        }
        if self.data.is_file() {
            return State::File(std::fs::read(&self.data).unwrap());
        }
        let mut entries: Vec<(String, Vec<u8>)> = std::fs::read_dir(&self.data)
            .unwrap()
            .map(|e| {
                let e = e.unwrap();
                let name = e.file_name().into_string().unwrap();
                if e.file_type().unwrap().is_dir() {
                    (format!("{name}/"), Vec::new())
                } else {
                    (name, std::fs::read(e.path()).unwrap())
                }
            })
            .collect();
        entries.sort();
        State::Dir(entries)
    }

    /// The temp root listing: the data dir, config and home, and nothing else.
    fn root_entries(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.data.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn json(s: &str) -> serde_json::Value {
    assert_eq!(
        s.matches('\n').count(),
        1,
        "one compact line expected: {s:?}"
    );
    serde_json::from_str(s).unwrap_or_else(|e| panic!("not JSON ({e}): {s:?}"))
}

// ---- criterion 1: the payload, byte for byte -----------------------------------------------

#[test]
fn concept_add_dry_run_json() {
    let env = Env::new();
    let out = env.run(["concept", "add", "machine learning", "--dry-run", "--json"]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    assert_eq!(out.stderr, "");
    insta::assert_snapshot!("concept_add_dry_run_json", out.stdout);
}

#[test]
fn concept_add_dry_run_json_existing_scheme() {
    let env = Env::with_fixture("store");
    let out = env.run(["concept", "add", "deep learning", "--dry-run", "--json"]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    assert_eq!(out.stderr, "");
    insta::assert_snapshot!("concept_add_dry_run_json_existing_scheme", out.stdout);
}

#[test]
fn concept_add_dry_run_json_suffix_order() {
    // The scheme owns `topics`, so the Concept is `topics-2`; its triples sort before the
    // scheme's because '-' (0x2D) < '>' (0x3E) in the whole N-Triples string.
    let env = Env::new();
    let out = env.run(["concept", "add", "topics", "--dry-run", "--json"]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    assert_eq!(out.stderr, "");
    insta::assert_snapshot!("concept_add_dry_run_json_suffix_order", out.stdout);
}

// ---- criterion 2: nothing is written -------------------------------------------------------

#[test]
fn dry_run_creates_no_data_dir() {
    for mode in [Some("--json"), None] {
        let env = Env::new();
        let roots = env.root_entries();
        let mut args = vec!["concept", "add", "machine learning", "--dry-run"];
        args.extend(mode);
        let out = env.run(&args);
        assert_eq!(out.code, Some(0), "{args:?}: {}", out.stderr);
        assert!(!env.data.exists(), "{args:?}: the data dir was created");
        assert_eq!(env.root_entries(), roots, "{args:?}");
    }
}

#[test]
fn dry_run_leaves_store_bytes_unchanged() {
    for name in ["store", "noncanonical"] {
        let env = Env::with_fixture(name);
        let before = env.state();
        let args = ["concept", "add", "deep learning", "--dry-run", "--json"];
        let first = env.run(args);
        assert_eq!(first.code, Some(0), "{name}: {}", first.stderr);
        assert_eq!(env.state(), before, "{name}: first dry run");
        let second = env.run(args);
        assert_eq!(second, first, "{name}: the same dry run twice");
        assert_eq!(env.state(), before, "{name}: second dry run");
        let text = env.run(["concept", "add", "deep learning", "--dry-run"]);
        assert_eq!(text.code, Some(0), "{name}: {}", text.stderr);
        assert_eq!(env.state(), before, "{name}: text dry run");
    }
}

// ---- criterion 3: errors are the real run's ------------------------------------------------

#[test]
fn shacl_violation_reported_in_dry_run() {
    let dry_env = Env::with_fixture("shacl/extension-shape");
    let real_env = Env::with_fixture("shacl/extension-shape");
    let before = dry_env.state();
    let dry = dry_env.run(["concept", "add", "deep learning", "--dry-run", "--json"]);
    let real = real_env.run(["concept", "add", "deep learning", "--json"]);
    assert_eq!(real.code, Some(1), "{}", real.stderr);
    assert_eq!(dry.code, Some(1), "{}", dry.stderr);
    assert_eq!(dry.stdout, "");
    assert_eq!(dry.stderr, real.stderr, "byte-identical stderr");
    let err = json(&dry.stderr);
    assert_eq!(err["error"]["rule"], "SHACL_VIOLATION");
    assert!(err.get("dry_run").is_none() && err.get("changes").is_none());
    assert!(err["error"].get("dry_run").is_none() && err["error"].get("changes").is_none());
    assert_eq!(dry_env.state(), before, "nothing is written");
}

/// How to set up the data dir of an error case.
enum Setup {
    Fixture(&'static str),
    BrokenVocabulary,
    DataDirIsAFile,
}

impl Setup {
    fn env(&self) -> Env {
        match self {
            Setup::Fixture(name) => Env::with_fixture(name),
            Setup::BrokenVocabulary => {
                let env = Env::new();
                std::fs::create_dir_all(&env.data).unwrap();
                std::fs::write(
                    env.data.join("vocabulary.ttl"),
                    "@prefix hali: <https://w3id.org/hali#> .\nhali:x hali:y \"canary\" .\nhali:z hali:y .\n",
                )
                .unwrap();
                env
            }
            Setup::DataDirIsAFile => {
                let env = Env::new();
                std::fs::write(&env.data, "not a directory").unwrap();
                env
            }
        }
    }
}

#[test]
fn dry_run_errors_match_the_real_run() {
    let cases: Vec<(&str, Setup, &str, i32)> = vec![
        (
            "LABEL_TAKEN",
            Setup::Fixture("store"),
            "machine learning",
            1,
        ),
        (
            "SHAPES_INVALID",
            Setup::Fixture("shacl/extension-panics"),
            "brand new",
            3,
        ),
        (
            "SHAPES_INVALID",
            Setup::Fixture("shacl/extension-node-core-shape"),
            "brand new",
            3,
        ),
        (
            "STORE_UNPARSEABLE",
            Setup::BrokenVocabulary,
            "machine learning",
            3,
        ),
        ("STORE_IO", Setup::DataDirIsAFile, "machine learning", 3),
    ];
    for (rule, setup, label, code) in &cases {
        for as_json in [true, false] {
            let dry_env = setup.env();
            let real_env = setup.env();
            let dry_before = dry_env.state();
            let real_before = real_env.state();
            let mut real_args = vec!["concept", "add", label];
            if as_json {
                real_args.push("--json");
            }
            let mut dry_args = real_args.clone();
            dry_args.push("--dry-run");
            let real = real_env.run(&real_args);
            let dry = dry_env.run(&dry_args);
            let what = format!("{rule} json={as_json}");
            assert_eq!(real.code, Some(*code), "{what}: {}", real.stderr);
            assert_eq!(dry, real, "{what}");
            assert_eq!(dry.stdout, "", "{what}");
            if as_json {
                let err = json(&dry.stderr);
                assert_eq!(err["error"]["rule"], *rule, "{what}");
                assert!(err.get("dry_run").is_none() && err.get("changes").is_none());
            } else {
                assert!(dry.stderr.starts_with("error: "), "{what}: {}", dry.stderr);
            }
            assert_eq!(dry_env.state(), dry_before, "{what}: dry run wrote");
            assert_eq!(real_env.state(), real_before, "{what}: real run wrote");
        }
    }
}

// ---- criterion 4: the changes equal what a real run writes ---------------------------------

#[test]
fn dry_run_changes_equal_real_run_writes() {
    let cases: Vec<(Option<PathBuf>, &str)> = vec![
        (None, "machine learning"),
        (Some(fixture("store")), "deep learning"),
        (Some(fixture("noncanonical")), "deep learning"),
        (
            Some(fixture("dry-run/scheme-in-extensions")),
            "machine learning",
        ),
        (None, "topics"),
        (None, "say \"hi\" \\ now"),
        (None, "Straße 日本"),
    ];
    for (store, label) in &cases {
        let dry =
            support::assert_dry_run_matches_real_run(store.as_deref(), &["concept", "add", label]);
        assert_eq!(dry["concept"]["label"], *label);
    }
}

// ---- criterion 6: text mode ----------------------------------------------------------------

#[test]
fn concept_add_dry_run_text() {
    let env = Env::new();
    let out = env.run(["concept", "add", "machine learning", "--dry-run"]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    assert_eq!(
        out.stdout,
        "would add concept machine-learning \"machine learning\"\n\
         + <https://w3id.org/hali#machine-learning> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.w3.org/2004/02/skos/core#Concept> .\n\
         + <https://w3id.org/hali#machine-learning> <http://www.w3.org/2004/02/skos/core#inScheme> <https://w3id.org/hali#topics> .\n\
         + <https://w3id.org/hali#machine-learning> <http://www.w3.org/2004/02/skos/core#prefLabel> \"machine learning\"@en .\n\
         + <https://w3id.org/hali#topics> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.w3.org/2004/02/skos/core#ConceptScheme> .\n"
    );
    assert_eq!(out.stderr, "note: dry run; nothing was written\n");
    assert_eq!(env.state(), State::Absent);
}

#[test]
fn dry_run_text_escapes_labels() {
    let label = "abc\u{202E}def";
    let env = Env::new();
    let out = env.run(["concept", "add", label, "--dry-run"]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    assert!(!out.stdout.contains('\u{202E}'), "{:?}", out.stdout);
    let lines: Vec<&str> = out.stdout.lines().collect();
    assert!(
        lines[0].ends_with("\"abc\\u{202e}def\""),
        "summary line: {:?}",
        lines[0]
    );
    assert_eq!(
        out.stdout.matches("\\u{202e}").count(),
        2,
        "{:?}",
        out.stdout
    );
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("+ ")
                && l.contains("skos/core#prefLabel> \"abc\\u{202e}def\"@en .")),
        "{:?}",
        out.stdout
    );

    // JSON keeps the character raw.
    let out = env.run(["concept", "add", label, "--dry-run", "--json"]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    assert!(out.stdout.contains('\u{202E}'), "{:?}", out.stdout);
    let value = json(&out.stdout);
    assert_eq!(value["concept"]["label"], label);
    assert_eq!(env.state(), State::Absent);
}

// ---- criterion 7: the flag ----------------------------------------------------------------

#[test]
fn dry_run_flag_position_is_free() {
    let variants: Vec<Vec<&str>> = vec![
        vec!["--dry-run", "concept", "add", "x", "--json"],
        vec!["concept", "add", "x", "--dry-run", "--json"],
        vec!["concept", "add", "x", "--json", "--dry-run"],
        vec!["concept", "--dry-run", "add", "x", "--json"],
    ];
    let mut outs = Vec::new();
    for args in &variants {
        let env = Env::new();
        let out = env.run(args);
        assert_eq!(out.code, Some(0), "{args:?}: {}", out.stderr);
        assert_eq!(env.state(), State::Absent, "{args:?}");
        outs.push(out);
    }
    for out in &outs[1..] {
        assert_eq!(out, &outs[0]);
    }
    assert_eq!(json(&outs[0].stdout)["dry_run"], true);
}

#[test]
fn dry_run_on_read_only_commands_changes_nothing() {
    let commands: Vec<Vec<&str>> = vec![
        vec!["concept", "list", "--json"],
        vec!["concept", "list"],
        vec!["validate", "--json"],
        vec!["validate"],
    ];
    for fixture_name in [Some("store"), None] {
        for args in &commands {
            let env = match fixture_name {
                Some(name) => Env::with_fixture(name),
                None => Env::new(),
            };
            let before = env.state();
            let plain = env.run(args);
            let mut with_flag = args.clone();
            with_flag.push("--dry-run");
            let dry = env.run(&with_flag);
            let mut flag_first = vec!["--dry-run"];
            flag_first.extend(args);
            let dry_first = env.run(&flag_first);
            let what = format!("{fixture_name:?} {args:?}");
            assert!(
                matches!(plain.code, Some(0) | Some(1)),
                "{what}: {}",
                plain.stderr
            );
            assert_eq!(dry, plain, "{what}");
            assert_eq!(dry_first, plain, "{what}");
            if args.contains(&"--json") && plain.code == Some(0) {
                let value = json(&dry.stdout);
                assert!(value.get("dry_run").is_none(), "{what}");
                assert!(value.get("changes").is_none(), "{what}");
            }
            assert_eq!(env.state(), before, "{what}");
        }
    }
}

#[test]
fn dry_run_usage_errors_exit_2() {
    // A guard: these exit 2 with and without the flag.
    let env = Env::new();
    let plain = env.run(["concept", "add", "!!!", "--json"]);
    let dry = env.run(["concept", "add", "!!!", "--dry-run", "--json"]);
    assert_eq!(plain.code, Some(2), "{}", plain.stderr);
    assert_eq!(dry.code, Some(2), "{}", dry.stderr);
    assert_eq!(dry.stdout, "");

    let text = env.run(["concept", "add", "!!!", "--dry-run"]);
    assert_eq!(text.code, Some(2), "{}", text.stderr);
    assert_eq!(text.stdout, "");

    let bare = env.run(["--dry-run"]);
    assert_eq!(bare.code, Some(2), "{}", bare.stderr);
    assert_eq!(bare.stdout, "");
    assert_eq!(env.state(), State::Absent);
}

#[test]
fn help_lists_dry_run() {
    let env = Env::new();
    let out = env.run(["concept", "add", "--help"]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    assert!(out.stdout.contains("--dry-run"), "{}", out.stdout);
}

/// #10: `add` and `edit` under the dry-run equality helper (spec §9.5, §14.3).
mod resource {
    use super::{fixture, support};
    use std::path::{Path, PathBuf};

    const EXISTING: &str = "urn:uuid:0192f0c4-7b1e-7cc3-9a43-6c1d2e8f4b10";

    fn sample(dir: &tempfile::TempDir, name: &str, contents: &[u8]) -> PathBuf {
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let path = root.join(name);
        std::fs::write(&path, contents).unwrap();
        path
    }

    /// A store whose resources.ttl already catalogs `path` (a complete Resource).
    fn store_cataloging(path: &Path) -> tempfile::TempDir {
        let store = tempfile::tempdir().unwrap();
        let ttl = format!(
            "@prefix hali: <https://w3id.org/hali#> .\n\
             @prefix dcat: <http://www.w3.org/ns/dcat#> .\n\
             @prefix dcterms: <http://purl.org/dc/terms/> .\n\
             @prefix foaf: <http://xmlns.com/foaf/0.1/> .\n\
             @prefix prov: <http://www.w3.org/ns/prov#> .\n\
             @prefix spdx: <http://spdx.org/rdf/terms#> .\n\
             @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .\n\
             <urn:uuid:0192f0c4-7b1e-7cc3-9a43-6c1d2e8f4b10> a hali:Resource ;\n\
             \tdcterms:title \"Cataloged\" ;\n\
             \tdcat:downloadURL <file://{}> ;\n\
             \tdcat:mediaType <http://www.iana.org/assignments/media-types/text/plain> ;\n\
             \tdcat:byteSize 6 ;\n\
             \tdcterms:modified \"2026-10-20T14:02:11Z\"^^xsd:dateTime ;\n\
             \tspdx:checksum <urn:uuid:0192f0c4-7b1e-7cc3-9a43-6c1d2e8f4b10#sha256> .\n\
             <urn:uuid:0192f0c4-7b1e-7cc3-9a43-6c1d2e8f4b10#sha256> \
             spdx:algorithm spdx:checksumAlgorithm_sha256 ;\n\
             \tspdx:checksumValue \"5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03\"^^xsd:hexBinary .\n\
             <urn:uuid:0192f0c4-7b20-7a01-8d2e-1f0b9c3a5e77> a dcat:CatalogRecord ;\n\
             \tfoaf:primaryTopic <urn:uuid:0192f0c4-7b1e-7cc3-9a43-6c1d2e8f4b10> ;\n\
             \tdcterms:issued \"2026-10-21T08:00:00Z\"^^xsd:dateTime ;\n\
             \tdcterms:modified \"2026-10-21T08:00:00Z\"^^xsd:dateTime ;\n\
             \tprov:wasAttributedTo <urn:uuid:0192f0c4-0000-7000-8000-000000000001> .\n\
             <urn:uuid:0192f0c4-0000-7000-8000-000000000001> a prov:Person ;\n\
             \tfoaf:name \"nate\" .\n",
            path.display()
        );
        std::fs::write(store.path().join("resources.ttl"), ttl).unwrap();
        store
    }

    #[test]
    fn add_dry_run_matches_real_run() {
        let dir = tempfile::tempdir().unwrap();
        let a = sample(&dir, "a.txt", b"hello\n");
        let b = sample(&dir, "b.txt", b"second file\n");
        let (a, b) = (a.to_str().unwrap(), b.to_str().unwrap());
        let fixture_store = fixture("store");
        // A fresh store (the Person is minted) and one that already has a Person.
        for store in [None, Some(fixture_store.as_path())] {
            let dry = support::assert_dry_run_matches_real_run(store, &["add", a]);
            assert_eq!(dry["added"].as_array().unwrap().len(), 1);
            assert_eq!(dry["existing"], serde_json::json!([]));
            assert!(!dry["changes"].as_array().unwrap().is_empty());

            let dry = support::assert_dry_run_matches_real_run(store, &["add", a, b, a]);
            assert_eq!(dry["added"].as_array().unwrap().len(), 2);

            let dry = support::assert_dry_run_matches_real_run(store, &["add", "--title", "T", a]);
            assert_eq!(dry["added"][0]["title"], "T");
        }
    }

    #[test]
    fn edit_dry_run_matches_real_run() {
        let store = fixture("store");
        for args in [
            vec![
                "edit",
                EXISTING,
                "--title",
                "Linked Data Patterns, 2nd edition",
            ],
            vec![
                "edit",
                "0192F0C4-7B1E-7CC3-9A43-6C1D2E8F4B10",
                "--title",
                "  Padded  ",
            ],
        ] {
            let dry = support::assert_dry_run_matches_real_run(Some(&store), &args);
            assert_eq!(
                dry["resource"]["id"],
                "0192f0c4-7b1e-7cc3-9a43-6c1d2e8f4b10"
            );
            let changes = dry["changes"].as_array().unwrap();
            // Title out, title in, catalog record modified out, modified in.
            assert_eq!(changes.len(), 4, "{dry}");
        }
    }

    #[test]
    fn re_add_dry_run_no_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = sample(&dir, "a.txt", b"hello\n");
        let store = store_cataloging(&path);
        let dry = support::assert_dry_run_matches_real_run(
            Some(store.path()),
            &["add", path.to_str().unwrap()],
        );
        assert_eq!(dry["added"], serde_json::json!([]));
        assert_eq!(dry["existing"].as_array().unwrap().len(), 1);
        assert_eq!(dry["changes"], serde_json::json!([]));

        // The same Resource under `edit` with its own title: no changes either.
        let dry = support::assert_dry_run_matches_real_run(
            Some(store.path()),
            &["edit", EXISTING, "--title", "Cataloged"],
        );
        assert_eq!(dry["changes"], serde_json::json!([]));
    }
}

/// #16: `--tag`, `--add-tag`/`--remove-tag` and `--broader` under the dry-run equality helper
/// (spec §9.5, §14.3), over tests/fixtures/store (Concepts `ml`, `semantic-web`, `linked-data`
/// with altLabel "LD"; one Resource tagged `linked-data` and `ml`).
mod tags {
    use super::{fixture, support};
    use serde_json::{Value, json};

    const EXISTING: &str = "urn:uuid:0192f0c4-7b1e-7cc3-9a43-6c1d2e8f4b10";
    const SUBJECT: &str = "<http://purl.org/dc/terms/subject>";
    const BROADER: &str = "<http://www.w3.org/2004/02/skos/core#broader>";

    fn changes_with(dry: &Value, op: &str, needle: &str) -> usize {
        dry["changes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["op"] == op && c["triple"].as_str().unwrap().contains(needle))
            .count()
    }

    fn tag_ids(record: &Value) -> Vec<&str> {
        record["tags"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["id"].as_str().unwrap())
            .collect()
    }

    #[test]
    fn add_tag_dry_run_matches() {
        let dir = tempfile::tempdir().unwrap();
        let path = std::fs::canonicalize(dir.path()).unwrap().join("a.txt");
        std::fs::write(&path, b"hello\n").unwrap();
        let path = path.to_str().unwrap();
        let store = fixture("store");

        // By slug and by label.
        let dry = support::assert_dry_run_matches_real_run(
            Some(&store),
            &["add", path, "--tag", "ml", "--tag", "Linked  Data"],
        );
        assert_eq!(tag_ids(&dry["added"][0]), ["linked-data", "ml"]);
        assert_eq!(changes_with(&dry, "add", SUBJECT), 2, "{dry}");
        assert_eq!(changes_with(&dry, "remove", SUBJECT), 0, "{dry}");
    }

    #[test]
    fn edit_tags_dry_run_matches() {
        let store = fixture("store");

        // One tag out, one in: subject remove, subject add, modified remove and add.
        let dry = support::assert_dry_run_matches_real_run(
            Some(&store),
            &[
                "edit",
                EXISTING,
                "--add-tag",
                "Semantic Web",
                "--remove-tag",
                "ml",
            ],
        );
        assert_eq!(tag_ids(&dry["resource"]), ["linked-data", "semantic-web"]);
        assert_eq!(dry["changes"].as_array().unwrap().len(), 4, "{dry}");
        assert_eq!(changes_with(&dry, "add", SUBJECT), 1);
        assert_eq!(changes_with(&dry, "remove", SUBJECT), 1);

        // Adding only.
        let dry = support::assert_dry_run_matches_real_run(
            Some(&store),
            &["edit", EXISTING, "--add-tag", "semantic-web"],
        );
        assert_eq!(dry["changes"].as_array().unwrap().len(), 3, "{dry}");

        // With a title: one mutation, one `modified` bump.
        let dry = support::assert_dry_run_matches_real_run(
            Some(&store),
            &[
                "edit",
                EXISTING,
                "--title",
                "T",
                "--add-tag",
                "semantic-web",
            ],
        );
        assert_eq!(dry["changes"].as_array().unwrap().len(), 5, "{dry}");

        // A present tag added (also by altLabel), an absent one removed: no changes at all.
        let dry = support::assert_dry_run_matches_real_run(
            Some(&store),
            &[
                "edit",
                EXISTING,
                "--add-tag",
                "LD",
                "--remove-tag",
                "semantic-web",
            ],
        );
        assert_eq!(dry["changes"], json!([]));
    }

    #[test]
    fn concept_add_broader_dry_run_matches() {
        let store = fixture("store");
        // The scheme is in the store already: three Concept triples and one broader.
        let dry = support::assert_dry_run_matches_real_run(
            Some(&store),
            &["concept", "add", "deep learning", "--broader", "ml"],
        );
        assert_eq!(dry["concept"]["broader"], json!(["ml"]));
        assert_eq!(dry["changes"].as_array().unwrap().len(), 4, "{dry}");
        assert_eq!(changes_with(&dry, "add", BROADER), 1);

        // By slug and label, repeated: sorted, once each.
        let dry = support::assert_dry_run_matches_real_run(
            Some(&store),
            &[
                "concept",
                "add",
                "deep learning",
                "--broader",
                "semantic-web",
                "--broader",
                "Machine Learning",
                "--broader",
                "ml",
            ],
        );
        assert_eq!(dry["concept"]["broader"], json!(["ml", "semantic-web"]));
        assert_eq!(changes_with(&dry, "add", BROADER), 2, "{dry}");
    }
}

/// #14: `forget` under the dry-run equality helper (spec §9.5, §14.3), over tests/fixtures/forget
/// (R1 IANA and tagged, R2 and R3 sharing a `hali` format, R3 `dcterms:relation` R2, the Person).
/// The real run gets `--yes`, as it has no terminal; a dry run ignores it.
#[cfg(feature = "goal")]
mod forget {
    use super::{fixture, support};
    use serde_json::Value;

    const R1: &str = "0192f0c4-0000-7000-8000-000000000201";
    const R2: &str = "0192f0c4-0000-7000-8000-000000000202";
    const R3: &str = "0192f0c4-0000-7000-8000-000000000203";

    fn removes(dry: &Value) -> Vec<&str> {
        dry["changes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                assert_eq!(c["op"], "remove", "forget only removes: {c}");
                c["triple"].as_str().unwrap()
            })
            .collect()
    }

    #[test]
    fn forget_dry_run_matches() {
        let store = fixture("forget");
        for (id, title) in [(R1, "one doc"), (R2, "two doc"), (R3, "three doc")] {
            let dry =
                support::assert_dry_run_matches_real_run(Some(&store), &["forget", id, "--yes"]);
            assert_eq!(dry["resource"]["id"], id);
            assert_eq!(dry["resource"]["title"], title);
            // Resource, checksum node and catalog record: 15 triples each (R2's include R3's
            // inbound relation; R3's own relation is one of R3's).
            assert_eq!(removes(&dry).len(), 15, "{id}: {dry}");
            // The Person stays.
            assert!(
                removes(&dry)
                    .iter()
                    .all(|t| !t.starts_with("<urn:uuid:0192f0c4-0000-7000-8000-000000000001>")),
                "{id}: {dry}"
            );
        }

        // R2's inbound triple from R3 is listed, and the format node (R3 still uses it) is not.
        let dry = support::assert_dry_run_matches_real_run(Some(&store), &["forget", R2, "--yes"]);
        let removed = removes(&dry);
        assert!(removed.iter().any(|t| t.starts_with(&format!(
            "<urn:uuid:{R3}> <http://purl.org/dc/terms/relation> <urn:uuid:{R2}>"
        ))));
        assert!(
            !removed
                .iter()
                .any(|t| t.contains("<http://purl.org/dc/terms/MediaType>")),
            "{dry}"
        );
    }
}
