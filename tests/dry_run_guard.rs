//! Permanent guard for `--dry-run` (#8, spec §9.5). The flag is global, so every subcommand
//! accepts it and each mutating arm must gate its own write. This test enumerates every leaf
//! subcommand of the real clap tree, requires an entry in `CASES`, and checks that
//! `hali --dry-run <subcommand>` leaves the data, config and HOME trees untouched.

use assert_cmd::Command;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::SystemTime;

/// The Resource of tests/fixtures/store.
const STORED: &str = "0192f0c4-7b1e-7cc3-9a43-6c1d2e8f4b10";

/// Resources of tests/fixtures/relations: r3 annotates r1; nothing relates r5 to r2.
const REL_R1: &str = "0192f0c4-0000-7000-8000-000000000701";
const REL_R2: &str = "0192f0c4-0000-7000-8000-000000000702";
const REL_R3: &str = "0192f0c4-0000-7000-8000-000000000703";
const REL_R5: &str = "0192f0c4-0000-7000-8000-000000000705";

/// Valid argv (after the global flags) for every leaf subcommand, keyed by its path, with the
/// tests/fixtures directory the store is copied from. Add a row here when you add a subcommand.
// One row per line: .goal/checks reads them by line.
#[rustfmt::skip]
const CASES: &[(&str, &str, &[&str])] = &[
    // Cargo.toml: a real file, relative to the package root the tests run in.
    ("add", "store", &["add", "Cargo.toml"]),
    ("concept add", "store", &["concept", "add", "guard probe"]),
    ("concept edit", "store", &["concept", "edit", "ml", "--add-alt", "guard probe"]),
    ("concept list", "store", &["concept", "list"]),
    ("concept remove", "store", &["concept", "remove", "a-first"]),
    ("concept rename", "store", &["concept", "rename", "ml", "guard probe"]),
    ("concept show", "store", &["concept", "show", "ml"]),
    ("edit", "store", &["edit", STORED, "--title", "guard probe"]),
    ("find", "store", &["find", "--limit", "1"]),
    ("forget", "store", &["forget", STORED]),
    ("relate", "relations", &["relate", REL_R5, "annotates", REL_R2]),
    ("relation add", "relations", &["relation", "add", "guard-probe", "--parent", "dcterms:relation"]),
    ("relation list", "relations", &["relation", "list"]),
    ("relation remove", "relations", &["relation", "remove", "excerptOf"]),
    ("schema", "store", &["schema"]),
    ("show", "store", &["show", STORED]),
    ("type add", "store", &["type", "add", "GuardProbe"]),
    ("type list", "store", &["type", "list"]),
    ("type remove", "store", &["type", "remove", "Paper"]),
    ("unrelate", "relations", &["unrelate", REL_R3, "annotates", REL_R1]),
    ("validate", "store", &["validate"]),
];

fn leaf_paths(cmd: &clap::Command, prefix: &str, out: &mut Vec<String>) {
    let subs: Vec<&clap::Command> = cmd
        .get_subcommands()
        .filter(|c| c.get_name() != "help")
        .collect();
    if subs.is_empty() {
        out.push(prefix.trim().to_string());
    }
    for sub in subs {
        leaf_paths(sub, &format!("{prefix} {}", sub.get_name()), out);
    }
}

#[derive(Debug, PartialEq)]
enum Entry {
    Dir(SystemTime),
    File(SystemTime, Vec<u8>),
    Other(SystemTime),
}

fn snapshot(root: &Path) -> BTreeMap<String, Entry> {
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
    walk(root, root, &mut out);
    out
}

fn run_case(name: &str, fixture: &str, argv: &[&str], with_store: bool, json: bool) {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    let cfg = root.path().join("cfg");
    let home = root.path().join("home");
    let share = root.path().join("share");
    for d in [&cfg, &home, &share] {
        std::fs::create_dir_all(d).unwrap();
    }
    if with_store {
        std::fs::create_dir_all(&data).unwrap();
        for entry in std::fs::read_dir(Path::new("tests/fixtures").join(fixture)).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(entry.path(), data.join(entry.file_name())).unwrap();
        }
    }
    let before = snapshot(root.path());
    let mut args = vec!["--dry-run"];
    if json {
        args.push("--json");
    }
    args.extend_from_slice(argv);
    let out = Command::cargo_bin("hali")
        .unwrap()
        .env_clear()
        .env("HALI_DATA_DIR", &data)
        .env("XDG_CONFIG_HOME", &cfg)
        .env("XDG_DATA_HOME", &share)
        .env("HOME", &home)
        .args(&args)
        .output()
        .unwrap();
    let ctx = format!("`{name}` {args:?} (store: {with_store}, json: {json})");
    assert_eq!(snapshot(root.path()), before, "{ctx}: a dry run wrote");
    if with_store {
        assert!(
            out.status.success(),
            "{ctx}: exit {:?}: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn every_subcommand_has_a_guard_case() {
    let mut leaves = Vec::new();
    leaf_paths(&hali::cli::command(), "", &mut leaves);
    assert!(!leaves.is_empty());
    for leaf in &leaves {
        assert!(
            CASES.iter().any(|(name, _, _)| name == leaf),
            "subcommand `{leaf}` exists in the clap tree but has no entry in CASES in \
             tests/dry_run_guard.rs; add valid argv for it so the dry-run guard covers it"
        );
    }
    for (name, _, _) in CASES {
        assert!(
            leaves.iter().any(|l| l == name),
            "CASES entry `{name}` is not a subcommand of the clap tree"
        );
    }
}

#[test]
fn dry_run_writes_nothing_for_every_subcommand() {
    for (name, fixture, argv) in CASES {
        for with_store in [false, true] {
            for json in [false, true] {
                run_case(name, fixture, argv, with_store, json);
            }
        }
    }
}
