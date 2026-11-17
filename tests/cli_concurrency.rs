//! Goal test for spec §14.4, the concurrency test (#5): about 8 overlapping reader and writer
//! processes on one store, each writer adding a unique Concept. No reader fails to parse, no
//! writer fails, no write is lost.
//!
//! Overlap is arranged, not slept for: the 8 workers of a round wait on one barrier and start
//! together; the readers keep reading (at least twice) until every writer of the round has
//! exited. 5 rounds, so 20 writes in all. Every run uses the real binary and temp dirs.

#[cfg(feature = "goal")]
mod lockkit;

#[cfg(feature = "goal")]
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
}
