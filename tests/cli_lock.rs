//! Goal tests for the store lock through the real binary (#5, spec §3.3, §9.4, §9.8, §14.4).
//! The locked golden `tests/snapshots/cli_lock__store_locked_json_envelope.snap` is hand-written
//! from the issue's contract.
//!
//! The contract: writers (a real mutating run) take an exclusive lock on `<data>/.lock` before
//! they load the store and keep it until the process ends; readers (and every `--dry-run`) take
//! a shared lock for the duration of the load; a process that cannot get its lock within 5 s
//! exits 3 with `STORE_LOCKED` (details `{"path","reason"}`). The test process plays the other
//! hali process: it holds `<data>/.lock` itself with `File::lock` / `File::lock_shared`.
//!
//! Only a few tests really wait the 5 s (the `STORE_LOCKED` ones); the rest are fast. `.lock`
//! is created by a real mutating run (empty, regular, 0600) and never by a reader or a dry
//! run; when it is absent a reader reads without a lock.

mod lockkit;

mod goal {
    use crate::lockkit::*;
    use serde_json::{Value, json};
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::time::Duration;

    const FIX: &str = "Wait for the other hali process to finish (it may be waiting at a confirmation prompt), then retry.";

    /// Row 1, 3: a shared lock held by another process blocks a writer; it gives up after 5 s
    /// with exit 3 and `STORE_LOCKED`, and no store byte changes.
    #[test]
    fn writer_blocked_by_shared_lock() {
        let env = Env::with_store();
        let _held = env.hold_shared();
        let before = env.store_bytes();
        let out = env.run(["--json", "concept", "add", "zeta"]);
        assert_store_locked(&out, "concept add under a shared lock");
        assert_eq!(env.store_bytes(), before, "a blocked writer wrote");
    }

    /// Row 1: an exclusive lock blocks a reader and a dry run (both take the shared lock).
    /// Run side by side, so the test waits the 5 s once.
    #[test]
    fn reader_and_dry_run_blocked_by_exclusive_lock() {
        let env = Env::with_store();
        let _held = env.hold_exclusive();
        let before = env.store_bytes();
        let runs: Vec<(&str, Running)> = vec![
            ("find", env.spawn(&[], ["--json", "find"])),
            (
                "--dry-run concept add",
                env.spawn(&[], ["--json", "--dry-run", "concept", "add", "zeta"]),
            ),
            (
                "concept list",
                env.spawn(&[], ["--json", "concept", "list"]),
            ),
            ("describe", env.spawn(&[], ["--json", "describe"])),
        ];
        for (what, mut running) in runs {
            drop(running.child.stdin.take());
            let out = running.finish();
            assert_store_locked(&out, what);
        }
        assert_eq!(env.store_bytes(), before);
    }

    /// Row 1: a waiter gets the lock the moment it is released and then writes.
    #[test]
    fn waiter_proceeds_when_released() {
        let env = Env::with_store();
        let held = env.hold_exclusive();
        let mut running = env.spawn(&[], ["--json", "concept", "add", "zeta"]);
        drop(running.child.stdin.take());
        // Another thread lets go of the lock after a second, while the writer waits for it.
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(1));
            drop(held);
        });
        let out = running.finish();
        releaser.join().unwrap();
        assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
        assert_eq!(out.stderr, "");
        assert!(
            out.elapsed >= Duration::from_millis(900),
            "the writer did not wait for the lock: {:?}",
            out.elapsed
        );
        assert!(out.elapsed < Duration::from_secs(5), "{:?}", out.elapsed);
        assert!(env.read("vocabulary.ttl").contains("\"zeta\"@en"));
    }

    /// Row 1: readers share the lock with each other (a shared lock held by another process does
    /// not block a reader), quickly.
    #[test]
    fn readers_share_the_lock() {
        let env = Env::with_store();
        let _held = env.hold_shared();
        let out = env.run(["--json", "concept", "list"]);
        assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
        assert_eq!(out.stderr, "");
        assert!(out.elapsed < Duration::from_secs(3), "{:?}", out.elapsed);
        let dry = env.run(["--json", "--dry-run", "concept", "add", "zeta"]);
        assert_eq!(dry.code, Some(0), "stderr: {}", dry.stderr);
    }

    /// Row 1: `.lock` is created by a real mutating run only: empty, a regular file, mode 0600,
    /// and never deleted. A reader or a dry run on an existing data dir creates nothing, and
    /// reads fine without it.
    #[test]
    fn lock_file_created_only_by_real_writes() {
        let env = Env::with_store();
        let lock = env.lock_path();
        for args in [
            vec!["--json", "find"],
            vec!["--json", "concept", "list"],
            vec!["--json", "describe"],
            vec!["--json", "--dry-run", "concept", "add", "zeta"],
            vec!["--json", "--dry-run", "type", "add", "Report"],
        ] {
            let out = env.run(&args);
            assert_eq!(out.code, Some(0), "{args:?}: {}", out.stderr);
            assert!(!lock.exists(), "{args:?} created .lock");
        }
        let out = env.run(["--json", "concept", "add", "zeta"]);
        assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
        let meta = std::fs::symlink_metadata(&lock).expect("a real write creates .lock");
        assert!(meta.file_type().is_file(), "`.lock` is not a regular file");
        assert_eq!(meta.len(), 0, "`.lock` is not empty");
        assert_eq!(meta.permissions().mode() & 0o777, 0o600);
        // Never deleted: a refused real run (the label is taken) and a reader leave it.
        let taken = env.run(["--json", "concept", "add", "zeta"]);
        assert_eq!(taken.code, Some(1), "stderr: {}", taken.stderr);
        let out = env.run(["--json", "find"]);
        assert_eq!(out.code, Some(0));
        assert!(lock.exists(), "`.lock` was deleted");
        assert_eq!(std::fs::metadata(&lock).unwrap().len(), 0);
    }

    /// Row 1: a first write that finds no data dir creates it (0700) and `.lock` (0600).
    #[test]
    fn first_write_creates_private_dir_and_lock() {
        let env = Env::new();
        assert!(!env.data().exists());
        let out = env.run(["--json", "concept", "add", "zeta"]);
        assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
        let dir = std::fs::metadata(env.data()).unwrap();
        assert_eq!(dir.permissions().mode() & 0o777, 0o700);
        let lock = std::fs::symlink_metadata(env.lock_path()).expect("`.lock` was created");
        assert!(lock.file_type().is_file());
        assert_eq!(lock.permissions().mode() & 0o777, 0o600);
        assert_eq!(lock.len(), 0);
    }

    /// Row 1: with no data dir a writer does not lock, so an unwritten run still creates
    /// nothing: a reader and a dry run on an absent data dir leave the temp root as it was.
    #[test]
    fn absent_data_dir_stays_absent_for_readers_and_dry_runs() {
        let env = Env::new();
        for args in [
            vec!["--json", "find"],
            vec!["--json", "describe"],
            vec!["--json", "--dry-run", "concept", "add", "zeta"],
        ] {
            let out = env.run(&args);
            assert_eq!(out.code, Some(0), "{args:?}: {}", out.stderr);
            assert!(!env.data().exists(), "{args:?} created the data dir");
        }
    }

    /// Row 1: every mutating command takes the exclusive lock: with another process holding
    /// it, each gives up after 5 s with `STORE_LOCKED` and writes nothing. All are started
    /// together, so the test waits 5 s once.
    #[test]
    fn every_mutating_command_takes_the_exclusive_lock() {
        let env = Env::with_resources();
        std::fs::write(env.file("four.txt"), b"four\n").unwrap();
        let _held = env.hold_exclusive();
        let one = env.file("one.txt");
        let one = one.to_str().unwrap();
        let files = env.root.join("files");
        let files = files.to_str().unwrap();
        let four = env.file("four.txt");
        let four = four.to_str().unwrap();
        let cases: Vec<Vec<&str>> = vec![
            vec!["add", four],
            vec!["edit", R1, "--title", "renamed"],
            vec!["relate", R1, "references", R2],
            vec!["unrelate", R1, "references", R2],
            vec!["forget", R3, "--yes"],
            vec!["refresh", R1],
            vec!["refresh", "--all"],
            vec!["relink", "--root", files],
            vec!["relink", R1, one],
            vec!["scan", files],
            vec!["scan"],
            vec!["concept", "add", "zeta"],
            vec!["concept", "rename", "ml", "ML"],
            vec!["concept", "edit", "ml", "--add-alt", "m-l"],
            vec!["concept", "remove", "ml", "--yes"],
            vec!["type", "add", "Report"],
            vec!["type", "remove", "Paper", "--yes"],
            vec!["relation", "add", "cites", "--parent", "references"],
            vec!["relation", "remove", "cites", "--yes"],
        ];
        let before = env.store_bytes();
        let runs: Vec<(String, Running)> = cases
            .iter()
            .map(|args| {
                let mut all = vec!["--json"];
                all.extend(args.iter().copied());
                (args.join(" "), env.spawn(&[], all))
            })
            .collect();
        for (what, mut running) in runs {
            drop(running.child.stdin.take());
            let out = running.finish();
            assert_store_locked(&out, &what);
        }
        assert_eq!(env.store_bytes(), before);
    }

    /// Row 1: the commands that need no store ignore a held lock and answer at once: `schema`,
    /// `help`, `scan --list` and `scan --forget-root` (which change only `roots.json`).
    #[test]
    fn commands_without_the_store_ignore_a_held_lock() {
        let env = Env::with_store();
        let _held = env.hold_exclusive();
        let root = env.root.join("files");
        for args in [
            vec!["schema", "error"],
            vec!["help"],
            vec!["--json", "scan", "--list"],
        ] {
            let out = env.run(&args);
            assert_eq!(out.code, Some(0), "{args:?}: {}", out.stderr);
            assert!(out.elapsed < Duration::from_secs(3), "{args:?} waited");
        }
        let out = env.run(["--json", "scan", "--forget-root", root.to_str().unwrap()]);
        assert_eq!(
            out.code,
            Some(2),
            "not remembered is a usage error: {}",
            out.stderr
        );
        assert!(
            out.elapsed < Duration::from_secs(3),
            "scan --forget-root waited"
        );
    }

    /// Row 3: the `STORE_LOCKED` envelope under `--json`, byte for byte (the locked golden).
    #[test]
    fn store_locked_json_envelope() {
        let env = Env::with_store();
        let _held = env.hold_exclusive();
        let out = env.run(["--json", "concept", "add", "zeta"]);
        assert_store_locked(&out, "concept add");
        assert_eq!(out.stdout, "");
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!("cli_lock__store_locked_json_envelope", out.stderr);
        });
    }

    /// Row 3: the text form is the `error:` line and the `hint:` line, no `Try:`.
    #[test]
    fn store_locked_text() {
        let env = Env::with_store();
        let _held = env.hold_exclusive();
        let out = env.run(["concept", "add", "zeta"]);
        assert_eq!(out.code, Some(3), "stderr: {}", out.stderr);
        assert_eq!(out.stdout, "");
        assert_eq!(
            out.stderr,
            format!(
                "error: another hali process holds the store lock /TMP/data/.lock; gave up after 5 s\nhint: {FIX}\n"
            )
        );
        assert!(
            out.elapsed >= Duration::from_millis(4900),
            "{:?}",
            out.elapsed
        );
        assert!(out.elapsed < Duration::from_secs(10), "{:?}", out.elapsed);
    }

    fn error_validator() -> jsonschema::Validator {
        let env = Env::new();
        let out = env.run(["schema", "error"]);
        assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
        let schema: Value = serde_json::from_str(&out.stdout).unwrap();
        jsonschema::draft202012::new(&schema).unwrap()
    }

    fn locked(details: Value) -> Value {
        json!({"schema_version": 1, "error": {
            "rule": "STORE_LOCKED",
            "message": "another hali process holds the store lock /TMP/data/.lock; gave up after 5 s",
            "details": details,
            "suggested_fix": FIX,
            "suggested_command": null}})
    }

    /// Row 3: `hali schema error` has a `STORE_LOCKED` branch: `details` carries the lock file
    /// `path` and a `reason` of `timeout` or `changed`.
    #[test]
    fn schema_has_store_locked_branch() {
        let v = error_validator();
        let accepts = |details: Value| v.is_valid(&locked(details));
        assert!(accepts(
            json!({"path": "/TMP/data/.lock", "reason": "timeout"})
        ));
        assert!(accepts(
            json!({"path": "/TMP/data/.lock", "reason": "changed"})
        ));
        assert!(
            !accepts(json!({"reason": "timeout"})),
            "details.path is required"
        );
        assert!(
            !accepts(json!({"path": "/TMP/data/.lock"})),
            "details.reason is required"
        );
        assert!(
            !accepts(json!({"path": 3, "reason": "timeout"})),
            "path is a string"
        );
        assert!(
            !accepts(json!({"path": "/TMP/data/.lock", "reason": "later"})),
            "reason is timeout or changed"
        );
        assert!(!accepts(json!([])), "details is an object");
    }

    /// Row 10: a confirmation prompt holds the lock. `forget` at a terminal prompt blocks a
    /// concurrent `concept add` (exit 3 after 5 s); after the answer `y` forget succeeds, and
    /// then the add goes through.
    #[test]
    fn forget_prompt_holds_lock() {
        use std::io::{Read, Write};
        use std::sync::mpsc;

        let env = Env::with_resources();
        let mut forget = env.spawn(&[("HALI_TEST_TTY", "1")], ["forget", R1]);
        // Read stderr as it comes, to see the prompt (`... [y/N] `, no newline) appear.
        let mut stderr = forget.child.stderr.take().unwrap();
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        let reader = std::thread::spawn(move || {
            let mut buf = [0u8; 256];
            loop {
                match stderr.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        let mut seen = Vec::new();
        while !String::from_utf8_lossy(&seen).contains("[y/N] ") {
            let chunk = rx
                .recv_timeout(Duration::from_secs(30))
                .expect("forget never showed its prompt");
            seen.extend(chunk);
        }

        let before = env.store_bytes();
        let blocked = env.run(["--json", "concept", "add", "zeta"]);
        assert_store_locked(&blocked, "concept add while forget is at its prompt");
        assert_eq!(
            env.store_bytes(),
            before,
            "nothing changed while the prompt was open"
        );

        let mut stdin = forget.child.stdin.take().unwrap();
        stdin.write_all(b"y\n").unwrap();
        drop(stdin);
        let out = forget.finish();
        reader.join().unwrap();
        assert_eq!(out.code, Some(0), "stdout: {}", out.stdout);
        assert!(
            !env.read("resources.ttl").contains(R1),
            "forget did not write"
        );

        let retry = env.run(["--json", "concept", "add", "zeta"]);
        assert_eq!(retry.code, Some(0), "stderr: {}", retry.stderr);
        assert!(env.read("vocabulary.ttl").contains("\"zeta\"@en"));
        assert!(!env.read("resources.ttl").contains(R1));
    }

    /// Row 10: no terminal, no wait: a `--yes` run never keeps the lock waiting on a prompt, and
    /// the lock is free again the moment it exits (a second writer is not delayed).
    #[test]
    fn lock_is_released_when_the_process_ends() {
        let env = Env::with_resources();
        let first = env.run(["--json", "forget", R3, "--yes"]);
        assert_eq!(first.code, Some(0), "stderr: {}", first.stderr);
        // The test can take the exclusive lock at once: nothing still holds it.
        let held = env.open_lock();
        held.try_lock()
            .expect("a finished hali run still holds the lock");
        drop(held);
        let second = env.run(["--json", "concept", "add", "zeta"]);
        assert_eq!(second.code, Some(0), "stderr: {}", second.stderr);
        assert!(
            second.elapsed < Duration::from_secs(3),
            "{:?}",
            second.elapsed
        );
    }

    /// Row 1: the kernel frees the lock when its holder dies: a `forget` waiting at its prompt
    /// holds it (the test cannot take it), and after the process is killed the test can.
    #[test]
    fn lock_is_released_when_the_process_is_killed() {
        let env = Env::with_resources();
        let mut running = env.spawn(&[("HALI_TEST_TTY", "1")], ["forget", R1]);
        let mut stderr = running.child.stderr.take().unwrap();
        let mut seen = Vec::new();
        let mut buf = [0u8; 64];
        while !String::from_utf8_lossy(&seen).contains("[y/N] ") {
            let n = std::io::Read::read(&mut stderr, &mut buf).unwrap();
            assert!(n > 0, "forget exited before its prompt");
            seen.extend_from_slice(&buf[..n]);
        }
        // It holds the lock at its prompt: the test cannot take it.
        let probe = env.open_lock();
        assert!(
            probe.try_lock().is_err(),
            "forget does not hold the lock at its prompt"
        );
        running.child.kill().unwrap();
        let _ = running.child.wait();
        probe
            .try_lock()
            .expect("the lock outlived the process that held it");
        assert_eq!(
            std::fs::metadata(env.lock_path()).unwrap().mode() & 0o777,
            0o600
        );
    }
}
