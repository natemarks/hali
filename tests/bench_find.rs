//! The latency benchmark (#26, spec §14.5): `hali find --json` over synthetic stores of 1,000
//! and 10,000 Resources. Run it with `make bench`; without `HALI_BENCH=1` it skips, which is
//! what `make test-external` sees. The report goes to stdout, so `--nocapture` is needed to
//! see it.

mod bench_support;

use bench_support::{RUNS, build_store, header, percentile, verdict};
use serde_json::Value;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// One fresh `hali find --json`, timed around spawn, wait and the stdout read.
fn find(root: &Path, data: &Path) -> (Duration, Value) {
    let start = Instant::now();
    let out = Command::new(env!("CARGO_BIN_EXE_hali"))
        .env_clear()
        .env("HALI_DATA_DIR", data)
        .env("HOME", root.join("home"))
        .env("XDG_CONFIG_HOME", root.join("cfg"))
        .args(["find", "--json"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .expect("spawning hali");
    let elapsed = start.elapsed();
    assert_eq!(out.status.code(), Some(0), "hali find --json failed");
    let json = serde_json::from_slice(&out.stdout).expect("find --json prints JSON");
    (elapsed, json)
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

#[test]
#[ignore = "benchmark: run with make bench"]
fn bench_find_latency() {
    if std::env::var("HALI_BENCH").as_deref() != Ok("1") {
        println!("skipped: run with make bench");
        return;
    }
    if cfg!(debug_assertions) || cfg!(feature = "test-hooks") {
        panic!("make bench needs a release build without test-hooks");
    }

    let tmp = tempfile::tempdir().expect("creating a temp dir");
    let mut rows = Vec::new();
    let mut p95_10k = 0.0;
    // 10000 must be the last size: `p95_10k` is the last iteration's value.
    for n in [1000usize, 10000] {
        let root = tmp.path().join(format!("n{n}"));
        for d in ["home", "cfg"] {
            std::fs::create_dir_all(root.join(d)).expect("creating a temp dir");
        }
        let data = build_store(&root, n);
        let root = std::fs::canonicalize(&root).expect("canonicalising the root");

        let (_, warm) = find(&root, &data);
        assert_eq!(warm["total"], n, "the warm-up finds every Resource");

        let samples: Vec<Duration> = (0..RUNS).map(|_| find(&root, &data).0).collect();
        let p50 = ms(percentile(&samples, 50));
        let p95 = ms(percentile(&samples, 95));
        rows.push(format!("{n}\t{p50:.1}\t{p95:.1}"));
        p95_10k = p95;
    }
    drop(tmp);

    // The harness has printed "test bench_find_latency ... " without a newline.
    println!();
    println!("{}", header(RUNS));
    println!("resources\tp50_ms\tp95_ms");
    for row in rows {
        println!("{row}");
    }
    println!("{}", verdict(p95_10k));
    std::io::stdout().flush().expect("flushing stdout");
    // The verdict is the last line of the run: exit before the harness prints its summary.
    std::process::exit(0);
}
