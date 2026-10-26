//! Goal tests for `hali concept add` / `hali concept list` through the real binary (#3).
//!
//! Every run sets HALI_DATA_DIR / XDG_CONFIG_HOME / HOME to temp dirs and clears the rest
//! of the environment, so the real store and config are never read.

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

struct Out {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

impl Env {
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

    /// Copies tests/fixtures/store into the data dir.
    fn with_store() -> Env {
        let env = Env::new();
        std::fs::create_dir_all(&env.data).unwrap();
        for name in ["vocabulary.ttl", "resources.ttl", "extensions.ttl"] {
            std::fs::copy(fixture("store").join(name), env.data.join(name)).unwrap();
        }
        env
    }

    /// No HALI_DATA_DIR: the caller sets it, or leaves it to XDG_DATA_HOME / HOME.
    fn bare(&self) -> Command {
        let mut cmd = Command::cargo_bin("hali").unwrap();
        cmd.env_clear()
            .env("XDG_CONFIG_HOME", &self.cfg)
            .env("HOME", &self.home);
        cmd
    }

    fn cmd(&self) -> Command {
        let mut cmd = self.bare();
        cmd.env("HALI_DATA_DIR", &self.data);
        cmd
    }

    fn run<I, S>(&self, args: I) -> Out
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut cmd = self.cmd();
        cmd.args(args);
        self.finish(cmd)
    }

    fn finish(&self, mut cmd: Command) -> Out {
        let out = cmd.output().unwrap();
        Out {
            code: out.status.code(),
            stdout: String::from_utf8(out.stdout).unwrap(),
            stderr: String::from_utf8(out.stderr)
                .unwrap()
                .replace(self.data.to_str().unwrap(), "[DATA_DIR]"),
        }
    }

    fn vocabulary(&self) -> String {
        std::fs::read_to_string(self.data.join("vocabulary.ttl")).unwrap()
    }

    fn write_config(&self, toml: &str) {
        let dir = self.cfg.join("hali");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.toml"), toml).unwrap();
    }
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn fixture_text(name: &str) -> String {
    std::fs::read_to_string(fixture(name)).unwrap()
}

fn json(s: &str) -> serde_json::Value {
    assert_eq!(
        s.matches('\n').count(),
        1,
        "one compact line expected: {s:?}"
    );
    serde_json::from_str(s).unwrap_or_else(|e| panic!("not JSON ({e}): {s:?}"))
}

fn dir_entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

// ---- criterion 1: data dir, store created on first write -------------------------------

#[test]
fn add_creates_store_under_hali_data_dir() {
    let env = Env::new();
    assert!(!env.data.exists());
    let out = env.run(["concept", "add", "machine learning"]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    assert_eq!(
        dir_entries(&env.data),
        ["vocabulary.ttl"],
        "only changed files are written"
    );

    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(&env.data).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700, "data dir is private");
}

#[test]
fn add_creates_store_under_xdg_data_home() {
    let env = Env::new();
    let xdg = env.home.join("xdg-data");
    let mut cmd = env.bare();
    cmd.env("XDG_DATA_HOME", &xdg)
        .args(["concept", "add", "machine learning"]);
    let out = env.finish(cmd);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    assert!(xdg.join("hali/vocabulary.ttl").is_file());
}

#[test]
fn add_creates_store_under_home() {
    let env = Env::new();
    let mut cmd = env.bare();
    cmd.args(["concept", "add", "machine learning"]);
    let out = env.finish(cmd);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    assert!(env.home.join(".local/share/hali/vocabulary.ttl").is_file());
}

#[test]
fn list_on_missing_store_is_empty_and_creates_nothing() {
    let env = Env::new();
    let out = env.run(["concept", "list"]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    assert_eq!(out.stdout, "");
    assert_eq!(out.stderr, "");
    assert!(!env.data.exists(), "reads never create anything");
}

// ---- criterion 3: deterministic Turtle -------------------------------------------------

#[test]
fn first_add_writes_canonical_vocabulary() {
    let env = Env::new();
    let out = env.run(["concept", "add", "machine learning"]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    assert_eq!(env.vocabulary(), fixture_text("first-add/vocabulary.ttl"));
}

#[test]
fn adds_in_any_order_serialize_sorted() {
    let forward = Env::new();
    for label in ["alpha", "machine learning", "zeta"] {
        assert_eq!(forward.run(["concept", "add", label]).code, Some(0));
    }
    let backward = Env::new();
    for label in ["zeta", "machine learning", "alpha"] {
        assert_eq!(backward.run(["concept", "add", label]).code, Some(0));
    }
    assert_eq!(forward.vocabulary(), backward.vocabulary());
    assert_eq!(
        forward.vocabulary(),
        fixture_text("three-adds/vocabulary.ttl")
    );
}

// ---- criterion 4: language -------------------------------------------------------------

#[test]
fn config_language_tags_labels() {
    let env = Env::new();
    env.write_config("[vocabulary]\nlanguage = \"de\"\n");
    let out = env.run(["concept", "add", "Maschinelles Lernen"]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    assert!(
        env.vocabulary()
            .contains("skos:prefLabel \"Maschinelles Lernen\"@de"),
        "{}",
        env.vocabulary()
    );
    // Listing shows labels in the configured language.
    let list = env.run(["concept", "list"]);
    assert_eq!(list.stdout, "maschinelles-lernen\tMaschinelles Lernen\n");
}

#[test]
fn invalid_config_is_config_invalid() {
    for bad in [
        "this is = = not toml",
        "[vocabulary]\nlanguage = \"not a tag!\"\n",
        "[vocabulary]\nlanguage = 5\n",
    ] {
        let env = Env::new();
        env.write_config(bad);
        let out = env.run(["concept", "add", "machine learning", "--json"]);
        assert_eq!(out.code, Some(3), "{bad:?}: {}", out.stderr);
        assert_eq!(out.stdout, "");
        assert_eq!(
            json(&out.stderr)["error"]["rule"],
            "CONFIG_INVALID",
            "{bad:?}"
        );
        assert!(!env.data.exists(), "nothing is written on a config error");
    }
}

// ---- criterion 6: output ---------------------------------------------------------------

#[test]
fn text_output_add_and_list() {
    let env = Env::new();
    let add = env.run(["concept", "add", "machine learning"]);
    assert_eq!(add.code, Some(0), "{}", add.stderr);
    assert_eq!(
        add.stdout,
        "added concept machine-learning \"machine learning\"\n"
    );
    assert_eq!(add.stderr, "");

    assert_eq!(env.run(["concept", "add", "alpha"]).code, Some(0));

    let list = env.run(["concept", "list"]);
    assert_eq!(list.code, Some(0));
    assert_eq!(
        list.stdout,
        "alpha\talpha\nmachine-learning\tmachine learning\n"
    );
    assert_eq!(list.stderr, "");

    let page = env.run(["concept", "list", "--limit", "1"]);
    assert_eq!(page.code, Some(0));
    assert_eq!(page.stdout, "alpha\talpha\n");
    assert_eq!(page.stderr, "note: showing 1 of 2; next: --offset 1\n");

    let rest = env.run(["concept", "list", "--limit", "1", "--offset", "1"]);
    assert_eq!(rest.stdout, "machine-learning\tmachine learning\n");
    assert_eq!(rest.stderr, "");
}

#[test]
fn concept_add_json() {
    let env = Env::new();
    let out = env.run(["concept", "add", "machine learning", "--json"]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    assert_eq!(out.stderr, "");
    insta::assert_snapshot!("concept_add_json", out.stdout);
}

#[test]
fn concept_list_json() {
    let env = Env::with_store();
    let out = env.run(["concept", "list", "--json"]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    assert_eq!(out.stderr, "");
    insta::assert_snapshot!("concept_list_json", out.stdout);
}

#[test]
fn concept_list_json_empty() {
    let env = Env::new();
    let out = env.run(["concept", "list", "--json"]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    insta::assert_snapshot!("concept_list_json_empty", out.stdout);
}

#[test]
fn concept_list_json_paged() {
    let env = Env::with_store();
    // --json is global: it may come before the subcommand.
    let out = env.run(["--json", "concept", "list", "--offset", "1", "--limit", "2"]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    assert_eq!(out.stderr, "", "no text note under --json");
    insta::assert_snapshot!("concept_list_json_paged", out.stdout);
}

// ---- criteria 7, 8: errors -------------------------------------------------------------

#[test]
fn label_taken_json_envelope() {
    let env = Env::with_store();
    let out = env.run(["concept", "add", "machine learning", "--json"]);
    assert_eq!(out.code, Some(1));
    assert_eq!(out.stdout, "");
    insta::assert_snapshot!("label_taken_json_envelope", out.stderr);
}

#[test]
fn label_taken_text_two_lines() {
    let env = Env::with_store();
    let out = env.run(["concept", "add", "machine learning"]);
    assert_eq!(out.code, Some(1));
    assert_eq!(out.stdout, "");
    assert_eq!(
        out.stderr,
        "error: label \"machine learning\"@en is already used by Concept ml\n\
         hint: Use the existing Concept, or choose a different label. Try: hali concept list\n"
    );
}

#[test]
fn label_taken_leaves_vocabulary_unchanged() {
    let env = Env::with_store();
    let before = std::fs::read(env.data.join("vocabulary.ttl")).unwrap();
    for label in ["machine learning", "  Machine   Learning ", "ld"] {
        let out = env.run(["concept", "add", label]);
        assert_eq!(out.code, Some(1), "{label:?}: {}", out.stderr);
        assert_eq!(
            std::fs::read(env.data.join("vocabulary.ttl")).unwrap(),
            before,
            "{label:?}"
        );
    }
}

#[test]
fn usage_errors_exit_2() {
    let env = Env::new();
    let long_bell = "a\u{7}b";
    let cases: Vec<(Vec<&str>, &str)> = vec![
        (vec!["concept", "frobnicate"], "frobnicate"),
        (vec!["concept", "list", "--limit", "abc"], "abc"),
        (vec!["concept", "list", "--offset", "-3x"], "-3x"),
        (vec!["concept", "add", "!!!"], "!!!"),
        (vec!["concept", "add", ""], "label"),
        (vec!["concept", "add", "   "], "label"),
        (vec!["concept", "add", long_bell], "label"),
        (vec!["concept", "add"], "label"),
    ];
    for (args, mention) in cases {
        let out = env.run(&args);
        assert_eq!(out.code, Some(2), "{args:?}: {}", out.stderr);
        assert_eq!(out.stdout, "", "{args:?}");
        assert!(
            out.stderr.to_lowercase().contains(mention),
            "{args:?}: stderr must mention {mention:?}: {}",
            out.stderr
        );
    }
    assert!(!env.data.exists(), "usage errors write nothing");

    // Usage errors keep clap's plain text under --json.
    let out = env.run(["--json", "concept", "frobnicate"]);
    assert_eq!(out.code, Some(2));
    assert!(!out.stderr.trim_start().starts_with('{'), "{}", out.stderr);
    assert!(out.stderr.contains("frobnicate"), "{}", out.stderr);
}

// ---- criteria 2, 9: broken stores ------------------------------------------------------

/// Line 3 is the syntax error. Line 2 carries a canary that must never be echoed.
const BROKEN_TURTLE: &str = "@prefix hali: <https://w3id.org/hali#> .\n\
    hali:x hali:y \"SECRET-CANARY-7731\" .\n\
    hali:z hali:y .\n";

fn assert_unparseable(file: &str) {
    let env = Env::new();
    std::fs::create_dir_all(&env.data).unwrap();
    std::fs::write(env.data.join(file), BROKEN_TURTLE).unwrap();

    let out = env.run(["concept", "list", "--json"]);
    assert_eq!(out.code, Some(3), "{file}: {}", out.stderr);
    assert_eq!(out.stdout, "");
    assert!(!out.stderr.contains("SECRET-CANARY-7731"), "{}", out.stderr);
    let err = json(&out.stderr);
    assert_eq!(err["schema_version"], 1);
    assert_eq!(err["error"]["rule"], "STORE_UNPARSEABLE");
    assert_eq!(err["error"]["details"]["file"], file);
    assert_eq!(err["error"]["details"]["line"], 3);
    assert!(err["error"]["details"]["column"].as_u64().unwrap() >= 1);

    // A mutation fails the same way and writes nothing.
    let add = env.run(["concept", "add", "machine learning"]);
    assert_eq!(add.code, Some(3), "{file}: {}", add.stderr);
    assert!(add.stderr.starts_with("error: "), "{}", add.stderr);
    assert!(!add.stderr.contains("SECRET-CANARY-7731"), "{}", add.stderr);
    assert_eq!(
        std::fs::read_to_string(env.data.join(file)).unwrap(),
        BROKEN_TURTLE
    );
    assert!(!env.data.join("vocabulary.ttl").exists() || file == "vocabulary.ttl");
}

#[test]
fn unparseable_vocabulary_exits_3() {
    assert_unparseable("vocabulary.ttl");
}

#[test]
fn unparseable_resources_exits_3() {
    assert_unparseable("resources.ttl");
}

#[test]
fn unparseable_extensions_exits_3() {
    assert_unparseable("extensions.ttl");
}

#[test]
fn invalid_utf8_is_unparseable() {
    let env = Env::new();
    std::fs::create_dir_all(&env.data).unwrap();
    std::fs::write(env.data.join("vocabulary.ttl"), [0xff, 0xfe, 0x00, 0x41]).unwrap();
    let out = env.run(["concept", "list", "--json"]);
    assert_eq!(out.code, Some(3), "{}", out.stderr);
    assert_eq!(json(&out.stderr)["error"]["rule"], "STORE_UNPARSEABLE");
}

#[test]
fn unusable_data_dir_is_store_io() {
    // The data dir is a regular file: it can be neither read nor created.
    let env = Env::new();
    std::fs::write(&env.data, "not a directory").unwrap();
    let cases: Vec<Vec<&str>> = vec![
        vec!["concept", "list", "--json"],
        vec!["concept", "add", "--json", "machine learning"],
    ];
    for args in cases {
        let out = env.run(&args);
        assert_eq!(out.code, Some(3), "{args:?}: {}", out.stderr);
        assert_eq!(out.stdout, "");
        let err = json(&out.stderr);
        assert_eq!(err["error"]["rule"], "STORE_IO", "{args:?}");
        assert_eq!(err["error"]["details"]["path"], "[DATA_DIR]", "{args:?}");
    }
    assert_eq!(
        std::fs::read_to_string(&env.data).unwrap(),
        "not a directory"
    );
}

// ---- the scheme IRI is reserved ----------------------------------------------------------

#[test]
fn add_topics_does_not_turn_the_scheme_into_a_concept() {
    let env = Env::new();
    let out = env.run(["concept", "add", "topics"]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);

    let list = env.run(["concept", "list"]);
    assert_eq!(list.code, Some(0), "{}", list.stderr);
    assert_eq!(list.stdout, "topics-2\ttopics\n");

    let ttl = env.vocabulary();
    assert!(!ttl.contains("hali:topics a skos:Concept "), "{ttl}");
    assert!(!ttl.contains("hali:topics a skos:Concept,"), "{ttl}");
    assert_eq!(
        ttl.matches("hali:topics a skos:ConceptScheme").count(),
        1,
        "{ttl}"
    );
}

#[test]
fn list_limit_zero_is_a_usage_error() {
    let env = Env::new();
    for args in [
        vec!["concept", "list", "--limit", "0"],
        vec!["concept", "list", "--limit", "0", "--json"],
    ] {
        let out = env.run(&args);
        assert_eq!(out.code, Some(2), "{args:?}: {}", out.stderr);
        assert_eq!(out.stdout, "", "{args:?}");
        assert!(out.stderr.contains("limit"), "{args:?}: {}", out.stderr);
    }
}

#[test]
fn list_limit_one_and_offset_zero_still_work() {
    let env = Env::with_store();
    let out = env.run(["concept", "list", "--limit", "1", "--offset", "0"]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    assert_eq!(out.stdout.lines().count(), 1, "{}", out.stdout);
}

const HOSTILE: &str = "x\u{1b}]0;pwned\u{7}\nfake-id\tfake label";

/// A store whose extensions.ttl holds a Concept with a hostile prefLabel.
fn hostile_store() -> Env {
    let env = Env::with_store();
    let path = env.data.join("extensions.ttl");
    let mut ttl = std::fs::read_to_string(&path).unwrap();
    ttl.push_str(
        "\nhali:evil a skos:Concept ;\n\tskos:inScheme hali:topics ;\n\
         \tskos:prefLabel \"x\\u001b]0;pwned\\u0007\\nfake-id\\tfake label\"@en .\n",
    );
    std::fs::write(&path, ttl).unwrap();
    env
}

#[test]
fn list_text_escapes_control_characters_in_labels() {
    let env = hostile_store();
    let json = env.run(["concept", "list", "--json"]);
    assert_eq!(json.code, Some(0), "{}", json.stderr);
    let v: serde_json::Value = serde_json::from_str(&json.stdout).unwrap();
    let results = v["results"].as_array().unwrap();
    assert!(
        results.iter().any(|c| c["label"] == HOSTILE),
        "JSON keeps the raw label: {}",
        json.stdout
    );
    assert!(json.stdout.contains("\\u001b"), "{}", json.stdout);

    let out = env.run(["concept", "list"]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    assert_eq!(
        out.stdout.lines().count(),
        results.len(),
        "{:?}",
        out.stdout
    );
    for line in out.stdout.lines() {
        assert_eq!(line.matches('\t').count(), 1, "{line:?}");
    }
    assert!(
        out.stdout
            .bytes()
            .all(|b| b >= 0x20 || b == b'\t' || b == b'\n'),
        "{:?}",
        out.stdout
    );
    assert!(out.stdout.contains("\\u{1b}"), "{:?}", out.stdout);
    assert!(out.stdout.contains("\\u{a}"), "{:?}", out.stdout);
}
