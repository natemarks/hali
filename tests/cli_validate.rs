//! Goal tests for `hali validate` through the real binary (#7).
//!
//! Every run sets HALI_DATA_DIR / XDG_CONFIG_HOME / HOME to temp dirs and clears the rest
//! of the environment, so the real store and config are never read.

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
struct Out {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

#[cfg(feature = "goal")]
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
        let mut cmd = Command::cargo_bin("hali").unwrap();
        cmd.env_clear()
            .env("XDG_CONFIG_HOME", &self.cfg)
            .env("HOME", &self.home)
            .env("HALI_DATA_DIR", &self.data)
            .args(args);
        let out = cmd.output().unwrap();
        Out {
            code: out.status.code(),
            stdout: String::from_utf8(out.stdout).unwrap(),
            stderr: String::from_utf8(out.stderr)
                .unwrap()
                .replace(self.data.to_str().unwrap(), "[DATA_DIR]"),
        }
    }

    /// Every file in the data dir with its bytes, sorted by name.
    fn files(&self) -> Vec<(String, Vec<u8>)> {
        let mut files: Vec<(String, Vec<u8>)> = std::fs::read_dir(&self.data)
            .unwrap()
            .map(|e| {
                let e = e.unwrap();
                (
                    e.file_name().into_string().unwrap(),
                    std::fs::read(e.path()).unwrap(),
                )
            })
            .collect();
        files.sort();
        files
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

#[cfg(feature = "goal")]
const MIXED_ROWS: &str = "hali:nolabel\thali:ConceptPrefLabelShape\t-\tA Concept needs exactly one skos:prefLabel per language\n\
    hali:orphan\thali:ConceptInSchemeShape\t-\tA Concept must be in the hali:topics scheme\n\
    hali:topics\thali:LabelUniqueShape\thali:ml2\tlabel \"Machine  Learning\" is used by more than one Concept\n\
    hali:topics\thali:LabelUniqueShape\thali:ml\tlabel \"machine learning\" is used by more than one Concept\n\
    hali:twolabels\thali:ConceptPrefLabelShape\t-\tA Concept needs exactly one skos:prefLabel per language\n";

#[cfg(feature = "goal")]
const FIX: &str = "Fix the data or the extensions.ttl shapes the violations point at, then retry.";

// ---- criteria 9, 10: conformance and violations ----------------------------------------

#[cfg(feature = "goal")]
#[test]
fn validate_json_conforming() {
    let env = Env::with_fixture("store");
    let out = env.run(["validate", "--json"]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    assert_eq!(out.stderr, "");
    insta::assert_snapshot!("validate_json_conforming", out.stdout);
}

#[cfg(feature = "goal")]
#[test]
fn validate_text_conforming() {
    let env = Env::with_fixture("store");
    let out = env.run(["validate"]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    assert_eq!(out.stdout, "the store conforms to the SHACL shapes\n");
    assert_eq!(out.stderr, "");
}

#[cfg(feature = "goal")]
#[test]
fn validate_json_violations() {
    let env = Env::with_fixture("shacl/mixed");
    let out = env.run(["validate", "--json"]);
    assert_eq!(out.code, Some(1), "{}", out.stderr);
    insta::assert_snapshot!("validate_json_violations", out.stdout);
    insta::assert_snapshot!("validate_json_violations_error", out.stderr);
}

#[cfg(feature = "goal")]
#[test]
fn validate_text_violations() {
    let env = Env::with_fixture("shacl/mixed");
    let out = env.run(["validate"]);
    assert_eq!(out.code, Some(1), "{}", out.stderr);
    assert_eq!(out.stdout, MIXED_ROWS);
    assert_eq!(
        out.stderr,
        format!("error: the store has 5 SHACL violations\nhint: {FIX}\n")
    );
}

#[cfg(feature = "goal")]
#[test]
fn validate_json_paged() {
    let env = Env::with_fixture("shacl/mixed");
    let out = env.run(["validate", "--json", "--limit", "2", "--offset", "1"]);
    assert_eq!(out.code, Some(1), "{}", out.stderr);
    let report = json(&out.stdout);
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["conforms"], false);
    assert_eq!(report["total"], 5);
    assert_eq!(report["truncated"], true);
    assert_eq!(report["next_offset"], 3);
    let results = report["results"].as_array().unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["focus_node"], "<https://w3id.org/hali#orphan>");
    assert_eq!(results[1]["focus_node"], "<https://w3id.org/hali#topics>");
    assert_eq!(results[1]["value"], "<https://w3id.org/hali#ml2>");

    let err = json(&out.stderr);
    assert_eq!(err["error"]["rule"], "SHACL_VIOLATION");
    assert_eq!(err["error"]["message"], "the store has 5 SHACL violations");
    assert_eq!(err["error"]["details"]["total"], 5);
}

#[cfg(feature = "goal")]
#[test]
fn validate_text_paged_prints_a_note() {
    let env = Env::with_fixture("shacl/mixed");
    let out = env.run(["validate", "--limit", "2", "--offset", "1"]);
    assert_eq!(out.code, Some(1), "{}", out.stderr);
    let rows: Vec<&str> = MIXED_ROWS.lines().collect();
    assert_eq!(out.stdout, format!("{}\n{}\n", rows[1], rows[2]));
    assert!(
        out.stderr
            .contains("note: showing 2 of 5; next: --offset 3\n"),
        "{}",
        out.stderr
    );
}

#[cfg(feature = "goal")]
#[test]
fn validate_limit_zero_is_a_usage_error() {
    let env = Env::with_fixture("store");
    // The command must exist: a valid limit works.
    let ok = env.run(["validate", "--limit", "1"]);
    assert_eq!(ok.code, Some(0), "{}", ok.stderr);
    for args in [
        vec!["validate", "--limit", "0"],
        vec!["validate", "--limit", "0", "--json"],
    ] {
        let out = env.run(&args);
        assert_eq!(out.code, Some(2), "{args:?}: {}", out.stderr);
        assert_eq!(out.stdout, "", "{args:?}");
        assert!(out.stderr.contains("limit"), "{args:?}: {}", out.stderr);
    }
}

// ---- criterion 11: read-only, store problems -------------------------------------------

#[cfg(feature = "goal")]
#[test]
fn validate_on_missing_store_conforms_and_creates_nothing() {
    let env = Env::new();
    assert!(!env.data.exists());
    let out = env.run(["validate"]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
    assert_eq!(out.stdout, "the store conforms to the SHACL shapes\n");
    assert_eq!(out.stderr, "");
    assert!(!env.data.exists(), "reads never create anything");
}

#[cfg(feature = "goal")]
#[test]
fn validate_writes_nothing() {
    for name in ["shacl/mixed", "store"] {
        let env = Env::with_fixture(name);
        let before = env.files();
        let expected = if name == "store" { Some(0) } else { Some(1) };
        for args in [vec!["validate"], vec!["validate", "--json"]] {
            let out = env.run(&args);
            assert_eq!(out.code, expected, "{name} {args:?}: {}", out.stderr);
            assert_eq!(env.files(), before, "{name} {args:?}");
        }
    }
}

#[cfg(feature = "goal")]
#[test]
fn validate_unparseable_store_exits_3() {
    let broken = "@prefix hali: <https://w3id.org/hali#> .\n\
        hali:x hali:y \"SECRET-CANARY-7731\" .\n\
        hali:z hali:y .\n";
    for file in ["vocabulary.ttl", "resources.ttl", "extensions.ttl"] {
        let env = Env::new();
        std::fs::create_dir_all(&env.data).unwrap();
        std::fs::write(env.data.join(file), broken).unwrap();
        let out = env.run(["validate", "--json"]);
        assert_eq!(out.code, Some(3), "{file}: {}", out.stderr);
        assert_eq!(out.stdout, "");
        assert!(!out.stderr.contains("SECRET-CANARY-7731"), "{}", out.stderr);
        let err = json(&out.stderr);
        assert_eq!(err["error"]["rule"], "STORE_UNPARSEABLE", "{file}");
        assert_eq!(err["error"]["details"]["file"], file);
    }
}

// ---- criterion 3: SHAPES_INVALID ---------------------------------------------------------

#[cfg(feature = "goal")]
#[test]
fn invalid_extension_shapes_exit_3() {
    for name in ["shacl/extension-invalid", "shacl/extension-sparql"] {
        let env = Env::with_fixture(name);
        let before = env.files();

        let out = env.run(["validate", "--json"]);
        assert_eq!(out.code, Some(3), "{name}: {}", out.stderr);
        assert_eq!(out.stdout, "", "{name}");
        assert!(!out.stderr.contains("SECRET-CANARY-4242"), "{}", out.stderr);
        let err = json(&out.stderr);
        assert_eq!(err["error"]["rule"], "SHAPES_INVALID", "{name}");
        assert_eq!(
            err["error"]["message"],
            "extensions.ttl has SHACL shapes that hali cannot use"
        );
        assert_eq!(
            err["error"]["details"],
            serde_json::json!({"file": "extensions.ttl"})
        );

        let text = env.run(["validate"]);
        assert_eq!(text.code, Some(3), "{name}: {}", text.stderr);
        assert_eq!(text.stdout, "");
        assert_eq!(
            text.stderr,
            "error: extensions.ttl has SHACL shapes that hali cannot use\n\
             hint: Fix or remove the shapes in extensions.ttl; hali applies SHACL Core shapes from it.\n"
        );

        // Every mutation is refused and writes nothing.
        let add = env.run(["concept", "add", "brand new"]);
        assert_eq!(add.code, Some(3), "{name}: {}", add.stderr);
        assert_eq!(add.stdout, "");
        assert!(!add.stderr.contains("SECRET-CANARY-4242"), "{}", add.stderr);
        assert_eq!(env.files(), before, "{name}");

        // Reads that do not compile shapes keep working.
        let list = env.run(["concept", "list"]);
        assert_eq!(list.code, Some(0), "{name}: {}", list.stderr);
        assert_eq!(list.stdout, "ml\tmachine learning\n");
    }
}

// ---- criterion 12: stored control characters are escaped ---------------------------------

#[cfg(feature = "goal")]
#[test]
fn validate_text_escapes_control_characters() {
    let env = Env::with_fixture("shacl/hostile-duplicate");
    let json_out = env.run(["validate", "--json"]);
    assert_eq!(json_out.code, Some(1), "{}", json_out.stderr);
    let report = json(&json_out.stdout);
    let results = report["results"].as_array().unwrap();
    assert_eq!(results.len(), 2);
    assert!(json_out.stdout.contains("\\u001b"), "{}", json_out.stdout);

    let out = env.run(["validate"]);
    assert_eq!(out.code, Some(1), "{}", out.stderr);
    assert_eq!(
        out.stdout.lines().count(),
        results.len(),
        "{:?}",
        out.stdout
    );
    for line in out.stdout.lines() {
        assert_eq!(line.matches('\t').count(), 3, "{line:?}");
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
    assert!(
        out.stderr.bytes().all(|b| b >= 0x20 || b == b'\n'),
        "{:?}",
        out.stderr
    );
}

// ---- round 1 review: reachable SPARQL, core-shape overrides, rudof panics ---------------

#[cfg(feature = "goal")]
#[test]
fn refused_extensions_exit_3_without_echoing_the_file() {
    for name in [
        "shacl/extension-reuses-core-sparql",
        "shacl/extension-core-prefixes",
        "shacl/extension-declare",
        "shacl/extension-prefix",
        "shacl/extension-namespace",
        "shacl/extension-deactivates-core-shape",
    ] {
        let env = Env::with_fixture(name);
        let before = env.files();

        let out = env.run(["validate", "--json"]);
        assert_eq!(out.code, Some(3), "{name}: {}", out.stderr);
        assert_eq!(out.stdout, "", "{name}");
        assert!(!out.stderr.contains("SECRET-CANARY-4242"), "{}", out.stderr);
        assert_eq!(
            json(&out.stderr)["error"]["rule"],
            "SHAPES_INVALID",
            "{name}"
        );

        let add = env.run(["concept", "add", "brand new"]);
        assert_eq!(add.code, Some(3), "{name}: {}", add.stderr);
        assert_eq!(add.stdout, "");
        assert_eq!(env.files(), before, "{name}");

        let list = env.run(["concept", "list"]);
        assert_eq!(list.code, Some(0), "{name}: {}", list.stderr);
    }
}

#[cfg(feature = "goal")]
#[test]
fn rudof_panic_is_shapes_invalid_with_no_leak() {
    let env = Env::with_fixture("shacl/extension-panics");
    for args in [vec!["validate", "--json"], vec!["validate"]] {
        let out = env.run(args);
        assert_eq!(out.code, Some(3), "{}", out.stderr);
        assert_eq!(out.stdout, "");
        for leak in ["TermAsSubject", "panicked", "\"a\"", "disjoint.rs"] {
            assert!(!out.stderr.contains(leak), "{leak}: {}", out.stderr);
        }
    }
}

#[cfg(feature = "goal")]
#[test]
fn extension_alignment_validates_clean() {
    let env = Env::with_fixture("shacl/extension-alignment");
    let out = env.run(["validate", "--json"]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
}

#[cfg(feature = "goal")]
#[test]
fn core_sh_extension_shape_still_validates() {
    let env = Env::with_fixture("shacl/extension-shape");
    let out = env.run(["validate", "--json"]);
    assert_eq!(out.code, Some(0), "{}", out.stderr);
}
