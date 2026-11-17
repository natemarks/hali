//! Goal tests added after review of #5 (rows 12, 13, 16, 17 of the goal map): the exclusivity of
//! every mutating command, the order of usage checks before the lock (spec §3.3, §9.1), the
//! permissions of what hali creates whatever the umask, and a non-regular `.lock` for readers.

mod lockkit;

mod goal {
    use crate::lockkit::*;
    use serde_json::Value;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::path::Path;
    use std::time::Duration;

    fn mode(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    /// Row 12: a SHARED lock held by another process (as a reader would) blocks every mutating
    /// command, so each one must have asked for the exclusive lock: a command left on a shared
    /// lock would pass straight through. Commands that would change nothing are included (the
    /// store is first brought to a state where a scan, `refresh --all`, a relink search and an
    /// `add` of a cataloged file are all no-ops). All are started together: one 5 s wait.
    #[test]
    fn every_mutating_command_needs_more_than_a_shared_lock() {
        let env = Env::with_resources();
        std::fs::write(env.file("four.txt"), b"four\n").unwrap();
        let files = env.root.join("files");
        let files = files.to_str().unwrap();
        for args in [vec!["scan", files], vec!["refresh", "--all"]] {
            let out = env.run(&args);
            assert_eq!(out.code, Some(0), "setup {args:?}: {}", out.stderr);
        }
        let _held = env.hold_shared();
        let one = env.file("one.txt");
        let one = one.to_str().unwrap();
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

    /// Row 13: a usage error that needs no store comes before the lock (spec §3.3 "Order"):
    /// `add a b --title x` exits 2 at once with the exclusive lock held, with the message it has
    /// without a lock.
    #[test]
    fn usage_errors_come_before_the_lock() {
        let env = Env::with_resources();
        std::fs::write(env.file("four.txt"), b"four\n").unwrap();
        std::fs::write(env.file("five.txt"), b"five\n").unwrap();
        let four = env.file("four.txt");
        let four = four.to_str().unwrap();
        let five = env.file("five.txt");
        let five = five.to_str().unwrap();
        let args = ["--json", "add", four, five, "--title", "x"];
        let free = env.run(args);
        assert_eq!(free.code, Some(2), "no lock; stderr: {}", free.stderr);
        let want = envelope(&free);
        assert!(
            want["error"]["message"]
                .as_str()
                .unwrap()
                .contains("--title takes exactly one path"),
            "{want}"
        );
        let _held = env.hold_exclusive();
        let out = env.run(args);
        assert_eq!(out.code, Some(2), "lock held; stderr: {}", out.stderr);
        assert!(
            out.elapsed < Duration::from_secs(3),
            "waited {:?} for the lock",
            out.elapsed
        );
        assert_eq!(envelope(&out), want, "same error with the lock held");
    }

    /// Row 13: `edit` keeps the order the spec gives (§9.1): the reference checks (unknown
    /// Resource, unknown Type, Types before tags) come before the "added and removed" usage
    /// error, so these identical pairs are not reported as usage errors first. No lock is held.
    #[test]
    fn edit_keeps_its_error_order() {
        let env = Env::with_resources();
        let out = env.run([
            "--json",
            "edit",
            "nosuch",
            "--add-tag",
            "ml",
            "--remove-tag",
            "ml",
        ]);
        let v = envelope(&out);
        assert_eq!(v["error"]["rule"], "UNKNOWN_RESOURCE", "{v}");
        let out = env.run([
            "--json",
            "edit",
            R1,
            "--add-type",
            "Bogus",
            "--remove-type",
            "Bogus",
        ]);
        let v = envelope(&out);
        assert_eq!(v["error"]["rule"], "UNKNOWN_TYPE", "{v}");
        let out = env.run([
            "--json",
            "edit",
            R1,
            "--add-type",
            "Paper",
            "--remove-type",
            "Paper",
            "--add-tag",
            "ml",
            "--remove-tag",
            "ml",
        ]);
        assert_eq!(out.code, Some(2), "{}", out.stderr);
        let v = envelope(&out);
        let msg = v["error"]["message"].as_str().unwrap();
        assert!(msg.contains("Type"), "type lists are checked first: {msg}");
        assert!(!msg.contains("Concept"), "{msg}");
    }

    /// Row 16: whatever the umask, the first write creates the data dir 0700 and the store files
    /// and `.lock` 0600, and so does a later publish. Under `umask 0277` the mode asked of
    /// `mkdir`/`open` comes out 0400, so only the explicit `chmod`s make these modes right.
    #[test]
    fn new_files_private_under_umask_0277() {
        let env = Env::new();
        let note = env.file("note.txt");
        std::fs::write(&note, b"hello\n").unwrap();
        let out = env.run_umask("0277", ["--json", "add", note.to_str().unwrap()]);
        assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
        assert_eq!(mode(&env.data()), 0o700, "data dir");
        for name in ["resources.ttl", ".lock"] {
            assert_eq!(mode(&env.data().join(name)), 0o600, "{name} (first write)");
        }
        let out = env.run_umask("0277", ["--json", "concept", "add", "zeta"]);
        assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
        assert_eq!(mode(&env.data().join("vocabulary.ttl")), 0o600);
        assert_eq!(mode(&env.data().join("resources.ttl")), 0o600);
        assert_eq!(mode(&env.data().join(".lock")), 0o600);
        assert_eq!(mode(&env.data()), 0o700);
        let out = env.run(["--json", "find"]);
        assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    }

    /// Row 16: a `.lock` that exists with a looser mode (0644) in a private data dir is made
    /// 0600 by the next writer.
    #[test]
    fn a_writer_tightens_a_loose_lock_file() {
        let env = Env::with_store();
        drop(env.open_lock());
        std::fs::set_permissions(env.lock_path(), std::fs::Permissions::from_mode(0o644)).unwrap();
        let out = env.run(["--json", "concept", "add", "zeta"]);
        assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
        assert_eq!(mode(&env.lock_path()), 0o600);
    }

    fn mkfifo(path: &Path) {
        let c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        // SAFETY: a valid NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    }

    /// Row 17: a `.lock` that is a symlink, a FIFO or a directory is `STORE_IO` for READERS too:
    /// no hang, nothing followed, nothing created.
    #[test]
    fn non_regular_lock_file_is_store_io_for_readers() {
        let mut running = Vec::new();
        let mut envs = Vec::new();
        for (name, args) in [
            ("dir", &["--json", "find"][..]),
            ("link", &["--json", "find"][..]),
            ("fifo", &["--json", "find"][..]),
            ("link", &["--json", "concept", "list"][..]),
            ("fifo", &["--json", "concept", "list"][..]),
        ] {
            let env = Env::with_store();
            let at = env.lock_path();
            match name {
                "dir" => std::fs::create_dir(&at).unwrap(),
                "link" => {
                    std::fs::write(env.root.join("lock-target"), b"keep\n").unwrap();
                    symlink(env.root.join("lock-target"), &at).unwrap();
                }
                _ => mkfifo(&at),
            }
            running.push((name, env.spawn(&[], args)));
            envs.push(env);
        }
        for ((name, mut r), env) in running.into_iter().zip(&envs) {
            drop(r.child.stdin.take());
            let out = r.finish_within(Duration::from_secs(10));
            assert_eq!(out.code, Some(3), "{name}: stderr: {}", out.stderr);
            assert_eq!(out.stdout, "", "{name}");
            let v: Value = serde_json::from_str(&out.stderr).unwrap();
            assert_eq!(v["error"]["rule"], "STORE_IO", "{name}: {}", out.stderr);
            let mut names: Vec<String> = std::fs::read_dir(env.data())
                .unwrap()
                .map(|e| e.unwrap().file_name().into_string().unwrap())
                .collect();
            names.sort();
            assert_eq!(
                names,
                [".lock", "extensions.ttl", "resources.ttl", "vocabulary.ttl"],
                "{name}: the reader created or removed something"
            );
            let kind = std::fs::symlink_metadata(env.lock_path())
                .unwrap()
                .file_type();
            match name {
                "dir" => assert!(kind.is_dir()),
                "link" => {
                    assert!(kind.is_symlink());
                    assert_eq!(
                        std::fs::read(env.root.join("lock-target")).unwrap(),
                        b"keep\n"
                    );
                }
                _ => assert!(!kind.is_file() && !kind.is_dir() && !kind.is_symlink()),
            }
        }
    }
}
