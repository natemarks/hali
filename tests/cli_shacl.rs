//! Goal tests for SHACL_VIOLATION on `hali concept add` through the real binary (#7).
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
    /// Copies every file of tests/fixtures/<name> into the data dir.
    fn with_fixture(name: &str) -> Env {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data");
        let cfg = root.path().join("cfg");
        let home = root.path().join("home");
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        for entry in std::fs::read_dir(fixture(name)).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(entry.path(), data.join(entry.file_name())).unwrap();
        }
        Env {
            _root: root,
            data,
            cfg,
            home,
        }
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
const FIX: &str = "Fix the data or the extensions.ttl shapes the violations point at, then retry.";

#[cfg(feature = "goal")]
#[test]
fn concept_add_refused_by_extension_shape() {
    let env = Env::with_fixture("shacl/extension-shape");
    let before = env.files();
    let out = env.run(["concept", "add", "deep learning", "--json"]);
    assert_eq!(out.code, Some(1), "{}", out.stderr);
    assert_eq!(out.stdout, "");
    let err = json(&out.stderr);
    assert_eq!(err["error"]["rule"], "SHACL_VIOLATION");
    assert_eq!(
        err["error"]["details"]["violations"][0]["shape"],
        "http://example.org/shapes#AltLabelRequired"
    );
    assert_eq!(env.files(), before, "nothing is written");
}

#[cfg(feature = "goal")]
#[test]
fn shacl_violation_json_envelope() {
    let env = Env::with_fixture("shacl/extension-shape");
    let vocabulary = std::fs::read(env.data.join("vocabulary.ttl")).unwrap();
    let out = env.run(["concept", "add", "deep learning", "--json"]);
    assert_eq!(out.code, Some(1), "{}", out.stderr);
    assert_eq!(out.stdout, "");
    insta::assert_snapshot!("shacl_violation_json_envelope", out.stderr);
    assert_eq!(
        std::fs::read(env.data.join("vocabulary.ttl")).unwrap(),
        vocabulary
    );
}

#[cfg(feature = "goal")]
#[test]
fn shacl_violation_text_two_lines() {
    let env = Env::with_fixture("shacl/extension-shape");
    let out = env.run(["concept", "add", "deep learning"]);
    assert_eq!(out.code, Some(1), "{}", out.stderr);
    assert_eq!(out.stdout, "");
    assert_eq!(
        out.stderr,
        format!(
            "error: the change would leave 1 SHACL violation in the store; nothing was written\n\
             hint: {FIX} Try: hali validate\n"
        )
    );
}

#[cfg(feature = "goal")]
#[test]
fn shacl_violation_details_are_capped() {
    let env = Env::with_fixture("shacl/many-violations");
    let before = env.files();
    let out = env.run(["concept", "add", "extra", "--json"]);
    assert_eq!(out.code, Some(1), "{}", out.stderr);
    assert_eq!(out.stdout, "");
    assert!(out.stderr.len() < 10_000, "{} chars", out.stderr.len());
    let err = json(&out.stderr);
    assert_eq!(err["error"]["rule"], "SHACL_VIOLATION");
    assert_eq!(
        err["error"]["message"],
        "the change would leave 12 SHACL violations in the store; nothing was written"
    );
    let details = &err["error"]["details"];
    assert_eq!(details["violations"].as_array().unwrap().len(), 10);
    assert_eq!(details["total"], 12);
    assert_eq!(details["truncated"], true);
    assert_eq!(env.files(), before);
}

#[cfg(feature = "goal")]
#[test]
fn concept_add_with_rudof_panic_is_shapes_invalid_and_writes_nothing() {
    let env = Env::with_fixture("shacl/extension-panics");
    let vocabulary = std::fs::read(env.data.join("vocabulary.ttl")).unwrap();
    let out = env.run(["concept", "add", "brand new", "--json"]);
    assert_eq!(out.code, Some(3), "{}", out.stderr);
    assert_eq!(out.stdout, "");
    for leak in ["TermAsSubject", "panicked", "\"a\""] {
        assert!(!out.stderr.contains(leak), "{leak}: {}", out.stderr);
    }
    assert_eq!(
        std::fs::read(env.data.join("vocabulary.ttl")).unwrap(),
        vocabulary
    );
}
