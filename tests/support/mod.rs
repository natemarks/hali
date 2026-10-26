//! Shared test support for `--dry-run` (#8). A plain module, not a test target: callers
//! declare `mod support;`. It exposes exactly one `pub fn`, so every private helper below is
//! reachable from it and the module is dead-code clean in any caller.

#[cfg(feature = "goal")]
use assert_cmd::Command;
#[cfg(feature = "goal")]
use oxrdf::{NamedOrBlankNode, Term, Triple};
#[cfg(feature = "goal")]
use std::collections::{BTreeMap, HashSet};
#[cfg(feature = "goal")]
use std::path::Path;
#[cfg(feature = "goal")]
use tempfile::TempDir;

/// Names -> bytes of the entries in a data dir (a subdirectory is recorded as `name/`).
#[cfg(feature = "goal")]
type Listing = BTreeMap<String, Vec<u8>>;

#[cfg(feature = "goal")]
struct RunEnv {
    _root: TempDir,
    data: std::path::PathBuf,
    stdout: String,
    stderr: String,
    code: Option<i32>,
    before: Option<Listing>,
}

#[cfg(feature = "goal")]
fn listing(dir: &Path) -> Option<Listing> {
    if !dir.exists() {
        return None;
    }
    let mut map = Listing::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().into_string().unwrap();
        if entry.file_type().unwrap().is_dir() {
            map.insert(format!("{name}/"), Vec::new());
        } else {
            map.insert(name, std::fs::read(entry.path()).unwrap());
        }
    }
    Some(map)
}

#[cfg(feature = "goal")]
fn run_in_fresh_env(store: Option<&Path>, args: &[&str]) -> RunEnv {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    let cfg = root.path().join("cfg");
    let home = root.path().join("home");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    if let Some(store) = store {
        std::fs::create_dir_all(&data).unwrap();
        for entry in std::fs::read_dir(store).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(entry.path(), data.join(entry.file_name())).unwrap();
        }
    }
    let before = listing(&data);
    let out = Command::cargo_bin("hali")
        .unwrap()
        .env_clear()
        .env("XDG_CONFIG_HOME", &cfg)
        .env("HOME", &home)
        .env("HALI_DATA_DIR", &data)
        .env("HALI_TEST_NOW", "2026-10-24T09:30:00Z")
        .env("HALI_TEST_ID_SEED", "1")
        .args(args)
        .output()
        .unwrap();
    RunEnv {
        _root: root,
        data,
        stdout: String::from_utf8(out.stdout).unwrap(),
        stderr: String::from_utf8(out.stderr).unwrap(),
        code: out.status.code(),
        before,
    }
}

/// Asserts exit 0, empty stderr and one compact JSON line; returns the parsed JSON.
#[cfg(feature = "goal")]
fn json_line(run: &RunEnv, what: &str, args: &[&str]) -> serde_json::Value {
    assert_eq!(run.code, Some(0), "{what} {args:?}: {}", run.stderr);
    assert_eq!(run.stderr, "", "{what} {args:?}");
    assert_eq!(
        run.stdout.matches('\n').count(),
        1,
        "{what} {args:?}: one JSON line expected: {:?}",
        run.stdout
    );
    assert!(run.stdout.ends_with('\n'), "{what} {args:?}");
    serde_json::from_str(&run.stdout)
        .unwrap_or_else(|e| panic!("{what} {args:?}: not JSON ({e}): {:?}", run.stdout))
}

/// All triples of a tool-managed Turtle file (empty when absent). Panics on a blank node.
#[cfg(feature = "goal")]
fn file_triples(listing: &Option<Listing>, name: &str) -> HashSet<Triple> {
    let Some(bytes) = listing.as_ref().and_then(|l| l.get(name)) else {
        return HashSet::new();
    };
    let mut set = HashSet::new();
    for t in oxttl::TurtleParser::new().for_slice(bytes) {
        let t = t.unwrap_or_else(|e| panic!("{name} is not Turtle: {e}"));
        assert!(
            !matches!(t.subject, NamedOrBlankNode::BlankNode(_))
                && !matches!(t.object, Term::BlankNode(_)),
            "{name}: blank node in {t}"
        );
        set.insert(t);
    }
    set
}

#[cfg(feature = "goal")]
fn managed_triples(listing: &Option<Listing>) -> HashSet<Triple> {
    let mut all = file_triples(listing, "vocabulary.ttl");
    all.extend(file_triples(listing, "resources.ttl"));
    all
}

/// Parses an N-Triples line as exactly one triple whose `format!("{t} .")` is the string.
#[cfg(feature = "goal")]
fn parse_change(triple: &str) -> Triple {
    let parsed: Vec<Triple> = oxttl::NTriplesParser::new()
        .for_slice(triple.as_bytes())
        .map(|t| t.unwrap_or_else(|e| panic!("not N-Triples ({e}): {triple:?}")))
        .collect();
    assert_eq!(parsed.len(), 1, "exactly one triple in {triple:?}");
    let t = parsed.into_iter().next().unwrap();
    assert_eq!(format!("{t} ."), triple, "canonical form");
    assert!(
        !matches!(t.subject, NamedOrBlankNode::BlankNode(_))
            && !matches!(t.object, Term::BlankNode(_)),
        "blank node in {triple:?}"
    );
    t
}

/// Runs `hali --json --dry-run <args>` and `hali --json <args>`, each in a fresh temp env
/// (data dir = copy of the files in `store`, or absent for None; XDG_CONFIG_HOME/HOME empty
/// temp dirs; env_clear; HALI_TEST_NOW=2026-10-24T09:30:00Z, HALI_TEST_ID_SEED=1), and asserts:
/// both exit 0 with stderr empty and one JSON line; the dry run left the data dir as it was
/// (absent stays absent; same names and bytes); dry JSON == real JSON + `dry_run: true` +
/// `changes`, these two being the last keys in that order; each change has exactly keys
/// op ("add"|"remove") and triple; `changes` is strictly sorted by (op, triple); each triple
/// parses (oxttl NTriplesParser) as exactly one triple whose `format!("{t} .")` is the string;
/// the add/remove sets equal the union over vocabulary.ttl and resources.ttl of
/// (after - before) / (before - after), comparing parsed `oxrdf::Triple` sets (so a
/// formatting-only rewrite counts as no change); every other file is byte-identical after the
/// real run; no blank node occurs in either file set or in `changes` (panics if one does).
/// Returns the dry run's JSON.
#[cfg(feature = "goal")]
pub fn assert_dry_run_matches_real_run(store: Option<&Path>, args: &[&str]) -> serde_json::Value {
    let mut dry_args = vec!["--json", "--dry-run"];
    dry_args.extend_from_slice(args);
    let mut real_args = vec!["--json"];
    real_args.extend_from_slice(args);

    let dry = run_in_fresh_env(store, &dry_args);
    let dry_json = json_line(&dry, "dry run", &dry_args);
    assert_eq!(
        listing(&dry.data),
        dry.before,
        "{dry_args:?}: the dry run changed the data dir"
    );

    let real = run_in_fresh_env(store, &real_args);
    let real_json = json_line(&real, "real run", &real_args);
    let after = listing(&real.data);

    // Payload: same keys in the same order, then dry_run and changes.
    let dry_obj = dry_json.as_object().expect("dry run JSON is an object");
    let real_obj = real_json.as_object().expect("real run JSON is an object");
    assert!(!real_obj.contains_key("dry_run") && !real_obj.contains_key("changes"));
    let dry_keys: Vec<&String> = dry_obj.keys().collect();
    let real_keys: Vec<&String> = real_obj.keys().collect();
    assert_eq!(dry_keys.len(), real_keys.len() + 2, "{dry_keys:?}");
    assert_eq!(dry_keys[..real_keys.len()], real_keys[..]);
    assert_eq!(dry_keys[real_keys.len()], "dry_run");
    assert_eq!(dry_keys[real_keys.len() + 1], "changes");
    assert_eq!(dry_obj["dry_run"], serde_json::Value::Bool(true));
    for key in &real_keys {
        assert_eq!(dry_obj[*key], real_obj[*key], "payload key {key}");
    }

    // Changes: shape, order, canonical form.
    let changes = dry_obj["changes"].as_array().expect("changes is an array");
    let mut keyed: Vec<(u8, String)> = Vec::new();
    let mut added: HashSet<Triple> = HashSet::new();
    let mut removed: HashSet<Triple> = HashSet::new();
    for change in changes {
        let obj = change.as_object().expect("a change is an object");
        let keys: Vec<&String> = obj.keys().collect();
        assert_eq!(keys, ["op", "triple"], "{change}");
        let op = obj["op"].as_str().expect("op is a string");
        let triple = obj["triple"].as_str().expect("triple is a string");
        let parsed = parse_change(triple);
        match op {
            "add" => {
                keyed.push((0, triple.to_string()));
                added.insert(parsed);
            }
            "remove" => {
                keyed.push((1, triple.to_string()));
                removed.insert(parsed);
            }
            other => panic!("unknown op {other:?}"),
        }
    }
    assert!(
        keyed.windows(2).all(|w| w[0] < w[1]),
        "changes are strictly sorted by (op, triple): {keyed:?}"
    );
    assert_eq!(added.len() + removed.len(), keyed.len(), "no duplicates");

    // Changes equal what the real run wrote.
    let before_set = managed_triples(&real.before);
    let after_set = managed_triples(&after);
    let wrote_added: HashSet<Triple> = after_set.difference(&before_set).cloned().collect();
    let wrote_removed: HashSet<Triple> = before_set.difference(&after_set).cloned().collect();
    assert_eq!(added, wrote_added, "{args:?}: added triples");
    assert_eq!(removed, wrote_removed, "{args:?}: removed triples");

    // Every other file is byte-identical.
    let other = |l: &Option<Listing>| -> Listing {
        l.clone()
            .unwrap_or_default()
            .into_iter()
            .filter(|(name, _)| name != "vocabulary.ttl" && name != "resources.ttl")
            .collect()
    };
    assert_eq!(other(&real.before), other(&after), "{args:?}: other files");

    dry_json
}
