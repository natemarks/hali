//! Support code for the latency benchmark (#26, spec §14.5): a deterministic synthetic store,
//! nearest-rank percentiles and the verdict line. Built through hali's own `core` API, so no
//! shared-mime-info is needed and the store is SHACL-conformant by construction.

use hali::core::resource::{self, FileFacts};
use hali::core::{Context, FixedClock, SeededIdGen, concept, rfc3339, types};
use hali::storage::{self, File};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

/// The number of timed runs per size.
pub const RUNS: usize = 30;

/// The report's header line (no newline).
pub fn header(runs: usize) -> String {
    format!(
        "hali find --json: release binary, {runs} fresh processes per size, warm page cache, nearest-rank percentiles"
    )
}

/// The mtime of every synthetic file, seconds since the Unix epoch.
const MTIME: u64 = 1_700_000_000;

/// File extension and detected media type, cycled over the Resources.
const KINDS: [(&str, &str); 4] = [
    ("txt", "text/plain"),
    ("md", "text/markdown"),
    ("pdf", "application/pdf"),
    ("json", "application/json"),
];

/// The Types a group can have: none, a Type, or its subtype.
const TYPES: [&[&str]; 3] = [&[], &["Document"], &["Report"]];

/// The tags a group can have: 0 to 2 Concepts.
const TAGS: [&[&str]; 6] = [
    &[],
    &["science"],
    &["physics"],
    &["science", "notes"],
    &["chemistry", "notes"],
    &["notes"],
];

fn strings(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

/// Builds a store of `n` Resources over real files under `root` and returns its data dir
/// (`<root>/data`, mode 0700; files 0600, plus an empty `.lock`).
pub fn build_store(root: &Path, n: usize) -> PathBuf {
    let root = fs::canonicalize(root).expect("canonicalising the root");
    let files = root.join("files");
    let data = root.join("data");
    fs::create_dir_all(&files).expect("creating the files dir");
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&data)
        .expect("creating the data dir");

    let ctx = Context::new(
        "en",
        Box::new(FixedClock::new("2026-10-24T09:30:00Z")),
        Box::new(SeededIdGen::new(26)),
    );
    let mut store = storage::load(&data).expect("loading an empty store");
    types::add(&ctx, &mut store, "Document", None).expect("adding Document");
    types::add(&ctx, &mut store, "Report", Some("Document")).expect("adding Report");
    for (label, broader) in [
        ("science", None),
        ("physics", Some("science")),
        ("chemistry", Some("science")),
        ("notes", None),
    ] {
        let parents: Vec<String> = broader.iter().map(|b| b.to_string()).collect();
        concept::add(&ctx, &mut store, label, &parents).expect("adding a Concept");
    }

    let mtime = UNIX_EPOCH + Duration::from_secs(MTIME);
    let groups = TYPES.len() * TAGS.len();
    let mut grouped: Vec<Vec<FileFacts>> = vec![Vec::new(); groups];
    for i in 0..n {
        let (ext, media) = KINDS[i % KINDS.len()];
        let path = files.join(format!("note-{i:05}.{ext}"));
        let content = format!("synthetic resource {i}\n");
        let mut file = fs::File::create(&path).expect("creating a file");
        file.write_all(content.as_bytes()).expect("writing a file");
        file.set_modified(mtime).expect("setting the mtime");
        let sha256: String = Sha256::digest(content.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        grouped[i % groups].push(FileFacts {
            path: path.to_str().expect("a UTF-8 path").to_string(),
            byte_size: content.len() as u64,
            modified: rfc3339(MTIME as i64),
            sha256,
            media_type: media.to_string(),
        });
    }
    for (g, facts) in grouped.iter().enumerate() {
        if facts.is_empty() {
            continue;
        }
        let types = strings(TYPES[g / TAGS.len()]);
        let tags = strings(TAGS[g % TAGS.len()]);
        resource::add_all(&ctx, &mut store, facts, None, &types, &tags, "bench")
            .expect("adding a group of Resources");
    }

    for (name, file) in [
        ("vocabulary.ttl", File::Vocabulary),
        ("resources.ttl", File::Resources),
    ] {
        write_private(
            &data.join(name),
            storage::to_turtle(&store, file).as_bytes(),
        );
    }
    write_private(&data.join(".lock"), b"");
    data
}

fn write_private(path: &Path, bytes: &[u8]) {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .expect("creating a store file");
    file.write_all(bytes).expect("writing a store file");
}

/// The nearest-rank percentile: sample number ceil(pct * n / 100) of the sorted samples.
pub fn percentile(samples: &[Duration], pct: u32) -> Duration {
    assert!(!samples.is_empty(), "no samples");
    let mut sorted = samples.to_vec();
    sorted.sort();
    let rank = (pct as usize * sorted.len()).div_ceil(100).max(1);
    sorted[rank - 1]
}

/// The last line of the report (no newline): over 200 ms at p95 is strictly greater.
pub fn verdict(p95_ms: f64) -> String {
    // Compare the number the line prints: the value rounded to one decimal.
    let shown = format!("{p95_ms:.1}");
    let rounded: f64 = shown.parse().expect("a formatted float parses");
    let shown = shown.strip_suffix(".0").unwrap_or(&shown);
    if rounded > 200.0 {
        format!(
            "verdict: p95 {shown} ms > 200 ms at 10k Resources: the on-disk storage fallback is a v0.1 task (spec §3.2); file a follow-up issue"
        )
    } else {
        format!(
            "verdict: p95 {shown} ms <= 200 ms at 10k Resources: the in-memory store stays (spec §3.2)"
        )
    }
}
