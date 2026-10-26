//! Goal tests for `--dry-run` through the real binary (#8).
//!
//! Assumed API: the global flag `--dry-run` (before or after the subcommand); on
//! `concept add --json` the payload gains `"dry_run":true` and `"changes":[{op,triple}]`
//! (see tests/snapshots/cli_dry_run__*.snap, hand-written from docs/spec.md §9.5 and the plan).
//!
//! Every run sets HALI_DATA_DIR / XDG_CONFIG_HOME / HOME to temp dirs and clears the rest
//! of the environment, so the real store and config are never read.

#[cfg(feature = "goal")]
mod support;

#[cfg(feature = "goal")]
use assert_cmd::Command;
#[cfg(feature = "goal")]
use std::ffi::OsStr;
#[cfg(feature = "goal")]
use std::path::{Path, PathBuf};
#[cfg(feature = "goal")]
use tempfile::TempDir;

#[cfg(feature = "goal")]
struct Env {
    _root: TempDir,
    data: PathBuf,
    cfg: PathBuf,
    home: PathBuf,
}

#[cfg(feature = "goal")]
#[derive(Debug, PartialEq)]
struct Out {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// What is in the data dir: nothing (absent), a file, or the entries with their bytes.
#[cfg(feature = "goal")]
#[derive(Debug, PartialEq)]
enum State {
    Absent,
    File(Vec<u8>),
    Dir(Vec<(String, Vec<u8>)>),
}

#[cfg(feature = "goal")]
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

#[cfg(feature = "goal")]
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[cfg(feature = "goal")]
fn json(s: &str) -> serde_json::Value {
    assert_eq!(
        s.matches('\n').count(),
        1,
        "one compact line expected: {s:?}"
    );
    serde_json::from_str(s).unwrap_or_else(|e| panic!("not JSON ({e}): {s:?}"))
}

// ---- criterion 1: the payload, byte for byte -----------------------------------------------

#[cfg(feature = "goal")]
#[test]
fn concept_add_dry_run_json() {
    let env = Env::new();
    let out = env.run(["concept", "add", "machine learning", "--dry-run", "--json"]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    assert_eq!(out.stderr, "");
    insta::assert_snapshot!("concept_add_dry_run_json", out.stdout);
}

#[cfg(feature = "goal")]
#[test]
fn concept_add_dry_run_json_existing_scheme() {
    let env = Env::with_fixture("store");
    let out = env.run(["concept", "add", "deep learning", "--dry-run", "--json"]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    assert_eq!(out.stderr, "");
    insta::assert_snapshot!("concept_add_dry_run_json_existing_scheme", out.stdout);
}

#[cfg(feature = "goal")]
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

#[cfg(feature = "goal")]
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

#[cfg(feature = "goal")]
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

#[cfg(feature = "goal")]
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
#[cfg(feature = "goal")]
enum Setup {
    Fixture(&'static str),
    BrokenVocabulary,
    DataDirIsAFile,
}

#[cfg(feature = "goal")]
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

#[cfg(feature = "goal")]
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

#[cfg(feature = "goal")]
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

#[cfg(feature = "goal")]
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

#[cfg(feature = "goal")]
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

#[cfg(feature = "goal")]
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

#[cfg(feature = "goal")]
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

#[cfg(feature = "goal")]
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

#[cfg(feature = "goal")]
#[test]
fn help_lists_dry_run() {
    let env = Env::new();
    let out = env.run(["concept", "add", "--help"]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    assert!(out.stdout.contains("--dry-run"), "{}", out.stdout);
}
