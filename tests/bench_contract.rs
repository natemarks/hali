//! Goal tests for the latency benchmark's support code (#26, spec §14.5). The benchmark itself
//! (`tests/bench_find.rs`) and `tests/bench_support/mod.rs` are implementer-owned. This file
//! includes the support module, so until it exists the file fails to compile (E0583): the
//! intended red.
//!
//! Assumed API of `tests/bench_support/mod.rs`:
//! - `build_store(root: &Path, n: usize) -> PathBuf`: builds a deterministic synthetic store
//!   of `n` Resources (real files under `root`) and returns the data dir (mode 0700; files
//!   0600, plus an empty `.lock`).
//! - `percentile(samples: &[Duration], pct: u32) -> Duration`: nearest rank, sample number
//!   ceil(pct * n / 100) of the sorted samples; the input may be unsorted.
//! - `verdict(p95_ms: f64) -> String`: the exact `verdict: ...` line (no newline): "stays" up
//!   to and including 200.0 ms, the v0.1 task line strictly above.
//!
//! - `pub const RUNS: usize`: the number of timed runs per size (moved here from
//!   `tests/bench_find.rs`, which uses it).
//! - `header(runs: usize) -> String`: the report's header line (no newline), saying
//!   "{runs} fresh processes per size"; `bench_find.rs` prints `header(RUNS)`.
//!
//! Spawned processes get a cleared environment holding only HALI_DATA_DIR, HOME and
//! XDG_CONFIG_HOME, all under temp dirs.

#[cfg(feature = "goal")]
#[allow(dead_code)]
#[rustfmt::skip]
mod bench_support;

#[cfg(feature = "goal")]
mod goal {
    use super::bench_support::{RUNS, build_store, header, percentile, verdict};
    use serde_json::Value;
    use std::collections::BTreeSet;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::Duration;

    fn hali(root: &Path, data: &Path, args: &[&str]) -> (Option<i32>, String, String) {
        let out = Command::new(env!("CARGO_BIN_EXE_hali"))
            .env_clear()
            .env("HALI_DATA_DIR", data)
            .env("HOME", root.join("home"))
            .env("XDG_CONFIG_HOME", root.join("cfg"))
            .args(args)
            .output()
            .unwrap();
        (
            out.status.code(),
            String::from_utf8(out.stdout).unwrap(),
            String::from_utf8(out.stderr).unwrap(),
        )
    }

    fn root() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        for d in ["home", "cfg"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        (tmp, root)
    }

    #[test]
    fn store_is_conformant_and_realistic() {
        let (_tmp, root) = root();
        let data = build_store(&root, 24);

        let mode = std::fs::metadata(&data).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "data dir mode");

        let (code, stdout, stderr) = hali(&root, &data, &["validate", "--json"]);
        assert_eq!(code, Some(0), "validate: {stdout} {stderr}");
        let report: Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(report["conforms"], true, "{stdout}");

        let (code, stdout, stderr) = hali(&root, &data, &["find", "--json", "--limit", "100"]);
        assert_eq!(code, Some(0), "find: {stderr}");
        let found: Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(found["total"], 24);
        let results = found["results"].as_array().unwrap();
        assert_eq!(results.len(), 24);

        let media: BTreeSet<&str> = results
            .iter()
            .map(|r| r["media_type"].as_str().unwrap())
            .collect();
        assert!(media.len() >= 3, "media types: {media:?}");

        let types: BTreeSet<&str> = results
            .iter()
            .flat_map(|r| r["types"].as_array().unwrap())
            .map(|t| t.as_str().unwrap())
            .collect();
        assert!(types.len() >= 2, "types: {types:?}");

        let tagged = results
            .iter()
            .filter(|r| !r["tags"].as_array().unwrap().is_empty())
            .count();
        assert!(tagged > 0, "no tagged Resource");
        assert!(tagged < 24, "no untagged Resource");

        for r in results {
            assert_eq!(r["missing"], false, "{r}");
            assert_eq!(r["stale"], false, "{r}");
        }
    }

    #[test]
    fn store_is_deterministic() {
        let (_a, root_a) = root();
        let (_b, root_b) = root();
        let data_a = build_store(&root_a, 24);
        let data_b = build_store(&root_b, 24);
        for name in ["resources.ttl", "vocabulary.ttl"] {
            let a = std::fs::read_to_string(data_a.join(name))
                .unwrap()
                .replace(root_a.to_str().unwrap(), "/ROOT");
            let b = std::fs::read_to_string(data_b.join(name))
                .unwrap()
                .replace(root_b.to_str().unwrap(), "/ROOT");
            assert!(!a.is_empty(), "{name} is empty");
            assert_eq!(a, b, "{name} differs between two builds");
        }
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn percentile_nearest_rank() {
        let sorted: Vec<Duration> = (1..=30).map(ms).collect();
        assert_eq!(percentile(&sorted, 50), ms(15));
        assert_eq!(percentile(&sorted, 95), ms(29));

        let mut unsorted = sorted.clone();
        unsorted.reverse();
        unsorted.swap(3, 20);
        assert_eq!(percentile(&unsorted, 50), ms(15));
        assert_eq!(percentile(&unsorted, 95), ms(29));

        assert_eq!(percentile(&[ms(7)], 50), ms(7));
        assert_eq!(percentile(&[ms(7)], 95), ms(7));
    }

    #[test]
    fn verdict_threshold() {
        let stays = verdict(200.0);
        assert!(stays.starts_with("verdict: p95 200"), "{stays}");
        assert!(stays.contains("<= 200 ms at 10k Resources"), "{stays}");
        assert!(
            stays.ends_with("the in-memory store stays (spec §3.2)"),
            "{stays}"
        );

        let over = verdict(200.1);
        assert!(
            over.starts_with("verdict: p95 200.1 ms > 200 ms at 10k Resources"),
            "{over}"
        );
        assert!(
            over.ends_with(
                "the on-disk storage fallback is a v0.1 task (spec §3.2); file a follow-up issue"
            ),
            "{over}"
        );
        assert!(!stays.contains('\n') && !over.contains('\n'));
    }

    #[test]
    fn verdict_agrees_with_what_it_prints() {
        for v in [200.04, 200.0, 199.96] {
            let line = verdict(v);
            assert!(
                line.starts_with("verdict: p95 200 ms <= 200 ms"),
                "{v}: {line}"
            );
            assert!(line.contains("stays"), "{v}: {line}");
        }
        for v in [200.06, 200.1] {
            let line = verdict(v);
            assert!(
                line.starts_with("verdict: p95 200.1 ms > 200 ms"),
                "{v}: {line}"
            );
            assert!(line.contains("v0.1 task"), "{v}: {line}");
        }
    }

    #[test]
    fn store_has_hierarchy_and_private_files() {
        let (_tmp, root) = root();
        let data = build_store(&root, 24);

        let vocab = std::fs::read_to_string(data.join("vocabulary.ttl")).unwrap();
        assert!(
            vocab.contains("skos:broader hali:science"),
            "no Concept under science in\n{vocab}"
        );
        assert!(
            vocab.contains("rdfs:subClassOf hali:Document"),
            "no Type under Document in\n{vocab}"
        );

        for name in ["resources.ttl", "vocabulary.ttl", ".lock"] {
            let mode = std::fs::metadata(data.join(name))
                .unwrap_or_else(|e| panic!("{name}: {e}"))
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600, "{name} mode");
        }
        assert_eq!(std::fs::metadata(data.join(".lock")).unwrap().len(), 0);
    }

    #[test]
    fn header_names_the_sample_count() {
        let line = header(RUNS);
        assert!(
            line.contains(&format!("{RUNS} fresh processes per size")),
            "{line}"
        );
        assert!(header(7).contains("7 fresh processes per size"));
        assert!(!header(RUNS).contains('\n'));
        let bench =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/bench_find.rs"))
                .unwrap();
        assert!(
            bench.contains("header(RUNS)"),
            "bench_find.rs must print header(RUNS)"
        );
        assert!(!bench.contains("const RUNS"), "RUNS lives in bench_support");
    }
}
