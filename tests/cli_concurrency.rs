//! Goal test for spec §14.4, the concurrency test (#5): about 8 overlapping reader and writer
//! processes on one store, each writer adding a unique Concept. No reader fails to parse, no
//! writer fails, no write is lost.
//!
//! Overlap is arranged, not slept for: the 8 workers of a round wait on one barrier and start
//! together; the readers keep reading (at least twice) until every writer of the round has
//! exited. 5 rounds, so 20 writes in all. Every run uses the real binary and temp dirs.

mod lockkit;

mod goal {
    use crate::lockkit::*;
    use serde_json::Value;
    use std::collections::BTreeSet;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Barrier, Mutex};

    const ROUNDS: usize = 5;
    const WRITERS: usize = 4;
    const READERS: usize = 4;

    /// Row 4.
    #[test]
    fn overlapping_readers_and_writers() {
        let env = Env::with_store();
        let failures = Mutex::new(Vec::<String>::new());
        let mut expected = BTreeSet::new();
        let mut reads = 0usize;

        for round in 0..ROUNDS {
            let gate = Barrier::new(WRITERS + READERS);
            let finished = AtomicUsize::new(0);
            let reads_this_round = AtomicUsize::new(0);
            std::thread::scope(|scope| {
                for w in 0..WRITERS {
                    let label = format!("writer {round} {w}");
                    expected.insert(label.clone());
                    let (env, gate, finished, failures) = (&env, &gate, &finished, &failures);
                    scope.spawn(move || {
                        gate.wait();
                        let out = env.run(["--json", "concept", "add", &label]);
                        if out.code != Some(0) {
                            failures.lock().unwrap().push(format!(
                                "writer `{label}` exited {:?}: {}",
                                out.code, out.stderr
                            ));
                        }
                        finished.fetch_add(1, Ordering::SeqCst);
                    });
                }
                for r in 0..READERS {
                    let (env, gate, finished, failures, reads) =
                        (&env, &gate, &finished, &failures, &reads_this_round);
                    scope.spawn(move || {
                        gate.wait();
                        let mut mine = 0;
                        while mine < 2 || finished.load(Ordering::SeqCst) < WRITERS {
                            let out = env.run(["--json", "concept", "list"]);
                            mine += 1;
                            reads.fetch_add(1, Ordering::SeqCst);
                            let parsed = serde_json::from_str::<Value>(&out.stdout);
                            if out.code != Some(0) || parsed.is_err() {
                                failures.lock().unwrap().push(format!(
                                    "reader {r} of round {round}: exit {:?}, stdout {:?}, stderr {}",
                                    out.code, out.stdout, out.stderr
                                ));
                            }
                        }
                    });
                }
            });
            reads += reads_this_round.load(Ordering::SeqCst);
        }

        let failures = failures.into_inner().unwrap();
        assert!(
            failures.is_empty(),
            "{} failures:\n{}",
            failures.len(),
            failures.join("\n")
        );
        assert!(reads >= ROUNDS * READERS * 2, "only {reads} reads ran");

        // No write is lost: every Concept the writers added is in the store, and it still parses.
        let out = env.run(["--json", "concept", "list", "--limit", "1000"]);
        assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
        let list: Value = serde_json::from_str(&out.stdout).unwrap();
        let labels: BTreeSet<String> = list["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["label"].as_str().unwrap().to_string())
            .collect();
        let lost: Vec<&String> = expected.iter().filter(|l| !labels.contains(*l)).collect();
        assert!(
            lost.is_empty(),
            "{} of {} writes were lost: {lost:?}",
            lost.len(),
            expected.len()
        );
        // Only the lock and the three store files remain: no temp file is left behind.
        let mut names: Vec<String> = std::fs::read_dir(env.data())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [".lock", "extensions.ttl", "resources.ttl", "vocabulary.ttl"]
        );
    }

    /// Row 15: the lock covers the load, not only the write, for every writer of `resources.ttl`.
    /// Each round starts 4 `edit --title` runs (each on its own pre-added Resource), a `scan` of
    /// a root with new files and a `relink` search that has one moved file to find, all waiting
    /// on a lock the test holds, then lets them race for it. A writer that loaded before it got
    /// the exclusive lock would publish a stale `resources.ttl` and lose someone's write: every
    /// edit, every scanned file and every relink must be there.
    #[test]
    fn edits_scan_and_relink_lose_nothing() {
        const EDITS: usize = 4;
        const ROUNDS: usize = 4;
        let env = Env::new();
        let files = env.root.join("files");
        let fresh = env.root.join("fresh");
        std::fs::create_dir_all(files.join("moved")).unwrap();
        std::fs::create_dir_all(&fresh).unwrap();
        let mut paths = Vec::new();
        for r in 0..ROUNDS {
            for i in 0..EDITS {
                paths.push(files.join(format!("e{r}x{i}.txt")));
            }
            paths.push(files.join(format!("m{r}.txt")));
        }
        for (n, p) in paths.iter().enumerate() {
            std::fs::write(p, format!("content {n}\n")).unwrap();
        }
        let out = env.run(
            ["--json", "add"]
                .into_iter()
                .map(String::from)
                .chain(paths.iter().map(|p| p.display().to_string())),
        );
        assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
        let added: Value = serde_json::from_str(&out.stdout).unwrap();
        let id_of = |path: &std::path::Path| -> String {
            added["added"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| {
                    // The output shows the temp root as `/TMP`.
                    r["path"]
                        .as_str()
                        .is_some_and(|p| p.ends_with(path.file_name().unwrap().to_str().unwrap()))
                })
                .unwrap_or_else(|| panic!("{path:?} not in {added}"))["id"]
                .as_str()
                .unwrap()
                .to_string()
        };

        let failures = Mutex::new(Vec::<String>::new());
        for round in 0..ROUNDS {
            std::fs::rename(
                files.join(format!("m{round}.txt")),
                files.join("moved").join(format!("m{round}.txt")),
            )
            .unwrap();
            for i in 0..3 {
                std::fs::write(
                    fresh.join(format!("new{round}x{i}.txt")),
                    format!("fresh {round} {i}\n"),
                )
                .unwrap();
            }
            // The test holds the shared lock, as a reader would, while the round's processes
            // start: every writer waits for the exclusive lock. A writer that loaded before it
            // had that lock has a stale copy by the time it gets it, whenever another writer
            // published first, and then loses that write. The lock is released after 0.6 s and
            // all of them race for it.
            let held = env.hold_shared();
            let mut jobs: Vec<(String, Vec<String>)> = (0..EDITS)
                .map(|i| {
                    let id = id_of(&files.join(format!("e{round}x{i}.txt")));
                    (
                        format!("edit {round} {i}"),
                        vec![
                            "edit".into(),
                            id,
                            "--title".into(),
                            format!("retitled-{round}-{i}"),
                        ],
                    )
                })
                .collect();
            jobs.push((
                "scan".into(),
                vec!["scan".into(), fresh.display().to_string()],
            ));
            jobs.push((
                "relink".into(),
                vec![
                    "relink".into(),
                    "--root".into(),
                    files.display().to_string(),
                ],
            ));
            // Ids are minted from the seed: a seed of its own keeps the scan's new Resources
            // apart from those the first `add` made.
            let seed = (100 + round).to_string();
            let running: Vec<(String, Running)> = jobs
                .into_iter()
                .map(|(what, args)| {
                    let mut all = vec!["--json".to_string()];
                    all.extend(args);
                    let mut r = env.spawn(&[("HALI_TEST_ID_SEED", seed.as_str())], all);
                    drop(r.child.stdin.take());
                    (what, r)
                })
                .collect();
            std::thread::sleep(std::time::Duration::from_millis(600));
            drop(held);
            for (what, r) in running {
                let out = r.finish();
                if out.code != Some(0) {
                    failures.lock().unwrap().push(format!(
                        "{what} of round {round} exited {:?}: {}",
                        out.code, out.stderr
                    ));
                }
            }
        }
        let failures = failures.into_inner().unwrap();
        assert!(failures.is_empty(), "{}", failures.join("\n"));

        let find = env.run(["--json", "find", "--limit", "1000"]);
        assert_eq!(
            find.code,
            Some(0),
            "the store no longer loads: {}",
            find.stderr
        );
        serde_json::from_str::<Value>(&find.stdout).unwrap();
        let text = env.read("resources.ttl");
        let mut lost = Vec::new();
        for round in 0..ROUNDS {
            for i in 0..EDITS {
                let title = format!("\"retitled-{round}-{i}\"");
                if !text.contains(&title) {
                    lost.push(format!("edit {title}"));
                }
            }
            for i in 0..3 {
                let name = format!("new{round}x{i}.txt");
                if !text.contains(&name) {
                    lost.push(format!("scanned {name}"));
                }
            }
            let moved = format!("/moved/m{round}.txt");
            if !text.contains(&moved) {
                lost.push(format!("relinked {moved}"));
            }
        }
        assert!(lost.is_empty(), "{} writes lost: {lost:?}", lost.len());
    }
}
