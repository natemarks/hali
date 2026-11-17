//! Goal tests for the safety of the store files and the data dir through the real binary
//! (#5, from the review of PR #35, spec §3.3, §9.4).
//!
//! The contract: a store file (`vocabulary.ttl`, `resources.ttl`, `extensions.ttl`) that is not
//! a regular file is `STORE_IO` (a symlink included, `extensions.ttl` too; a FIFO never hangs);
//! one over 256 MiB is `STORE_IO` before it is loaded; a data dir that is not owned by the user
//! or is group- or world-writable is `STORE_IO` whose fix is `chmod go-w <dir>` (read bits are
//! fine); new dirs are created 0700 and new files 0600 whatever the umask; a `.lock` that is
//! not a regular file is `STORE_IO`. All exit 3.

#[cfg(feature = "goal")]
mod lockkit;

#[cfg(feature = "goal")]
mod goal {
    use crate::lockkit::*;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
    use std::path::Path;
    use std::time::Duration;

    fn assert_store_io(out: &Out, what: &str) {
        assert_eq!(
            out.code,
            Some(3),
            "{what}: exit; stdout: {}; stderr: {}",
            out.stdout,
            out.stderr
        );
        assert_eq!(out.stdout, "", "{what}");
        assert_eq!(
            envelope(out)["error"]["rule"],
            "STORE_IO",
            "{what}: {}",
            out.stderr
        );
    }

    fn mkfifo(path: &Path) {
        let c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        // SAFETY: a valid NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    }

    /// Row 7: a symlinked `vocabulary.ttl` or `resources.ttl` is refused on a write and on a
    /// read, and `extensions.ttl` on a read; the target and the link are unchanged. The target
    /// holds valid Turtle, so a followed link would work.
    #[test]
    fn symlinked_store_files() {
        let cases: [(&str, &[&str]); 3] = [
            ("vocabulary.ttl", &["concept", "add", "zeta"]),
            ("resources.ttl", &["concept", "add", "zeta"]),
            ("extensions.ttl", &["concept", "add", "zeta"]),
        ];
        for (name, write) in cases {
            for (what, args) in [("write", write.to_vec()), ("read", vec!["find"])] {
                // `extensions.ttl` is only ever read; a write loads it too, so both are refused.
                let env = Env::with_store();
                let target = env.root.join("elsewhere.ttl");
                std::fs::copy(env.data().join(name), &target).unwrap();
                std::fs::remove_file(env.data().join(name)).unwrap();
                symlink(&target, env.data().join(name)).unwrap();
                let (target_before, others) = (std::fs::read(&target).unwrap(), env.store_bytes());
                let mut all = vec!["--json"];
                all.extend(args);
                let out = env.run(&all);
                assert_store_io(&out, &format!("{name} symlinked, {what}"));
                assert_eq!(
                    std::fs::read(&target).unwrap(),
                    target_before,
                    "{name} {what}"
                );
                assert_eq!(
                    std::fs::read_link(env.data().join(name)).unwrap(),
                    target,
                    "{name} {what}: the link was replaced"
                );
                assert_eq!(env.store_bytes(), others, "{name} {what}");
            }
        }
    }

    /// Row 7: a symlink to a directory or a dangling one is refused too.
    #[test]
    fn dangling_and_directory_symlinks_are_refused() {
        for dangling in [true, false] {
            let env = Env::with_store();
            let target = env.root.join("target");
            if !dangling {
                std::fs::create_dir(&target).unwrap();
            }
            std::fs::remove_file(env.data().join("vocabulary.ttl")).unwrap();
            symlink(&target, env.data().join("vocabulary.ttl")).unwrap();
            let out = env.run(["--json", "concept", "add", "zeta"]);
            assert_store_io(&out, "a symlink in place of vocabulary.ttl");
            assert!(
                !dangling || !target.exists(),
                "a dangling link was written through"
            );
        }
    }

    /// Row 8: a data dir that is group- or world-writable is `STORE_IO` (exit 3) for a write and
    /// for a read, with the fix `chmod go-w <dir>`; nothing is written. 0755 and 0750 (read bits)
    /// are fine.
    #[test]
    fn loose_data_dir() {
        for (mode, refused) in [
            (0o777, true),
            (0o770, true),
            (0o707, true),
            (0o722, true),
            (0o755, false),
            (0o750, false),
            (0o700, false),
        ] {
            for args in [
                vec!["--json", "concept", "add", "zeta"],
                vec!["--json", "find"],
                vec!["--json", "describe"],
            ] {
                let env = Env::with_store();
                std::fs::set_permissions(env.data(), std::fs::Permissions::from_mode(mode))
                    .unwrap();
                let before = env.store_bytes();
                let out = env.run(&args);
                if refused {
                    assert_store_io(&out, &format!("{mode:o} {args:?}"));
                    let fix = envelope(&out)["error"]["suggested_fix"]
                        .as_str()
                        .unwrap()
                        .to_string();
                    assert!(fix.contains("chmod go-w"), "{mode:o}: fix {fix:?}");
                    assert_eq!(env.store_bytes(), before, "{mode:o} {args:?}: wrote");
                } else {
                    assert_eq!(
                        out.code,
                        Some(0),
                        "{mode:o} {args:?}: stderr: {}",
                        out.stderr
                    );
                }
            }
        }
    }

    /// Row 8: the text form says so on one `error:` line and a `hint:` line with `chmod go-w`.
    #[test]
    fn loose_data_dir_text() {
        let env = Env::with_store();
        std::fs::set_permissions(env.data(), std::fs::Permissions::from_mode(0o777)).unwrap();
        let out = env.run(["concept", "add", "zeta"]);
        assert_eq!(out.code, Some(3), "stderr: {}", out.stderr);
        assert_eq!(out.stdout, "");
        let lines: Vec<&str> = out.stderr.lines().collect();
        assert_eq!(lines.len(), 2, "{:?}", out.stderr);
        assert!(lines[0].starts_with("error: "), "{:?}", out.stderr);
        assert!(lines[1].starts_with("hint: "), "{:?}", out.stderr);
        assert!(lines[1].contains("chmod go-w"), "{:?}", out.stderr);
    }

    /// Row 8: a data dir that is a symlink to a private directory is followed.
    #[test]
    fn symlinked_data_dir_is_followed() {
        let env = Env::with_store();
        let real = env.root.join("real-data");
        std::fs::rename(env.data(), &real).unwrap();
        symlink(&real, env.data()).unwrap();
        let out = env.run(["--json", "concept", "add", "zeta"]);
        assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
        assert!(
            std::fs::read_to_string(real.join("vocabulary.ttl"))
                .unwrap()
                .contains("\"zeta\"@en")
        );
    }

    /// Row 8: new files are private whatever the umask: under `umask 000` a first write creates
    /// the data dir 0700 and `resources.ttl` (through `add`) and `.lock`
    /// 0600; a later publish of `vocabulary.ttl` into the existing dir is 0600 too.
    #[test]
    fn new_files_private_under_umask_000() {
        let env = Env::new();
        let note = env.file("note.txt");
        std::fs::write(&note, b"hello\n").unwrap();
        let out = env.run_umask("000", ["--json", "add", note.to_str().unwrap()]);
        assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&env.data()), 0o700, "data dir");
        for name in ["resources.ttl", ".lock"] {
            assert_eq!(mode(&env.data().join(name)), 0o600, "{name} (first write)");
        }
        let out = env.run_umask("000", ["--json", "concept", "add", "zeta"]);
        assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
        assert_eq!(
            mode(&env.data().join("vocabulary.ttl")),
            0o600,
            "second publish"
        );
        assert_eq!(mode(&env.data()), 0o700);
    }

    /// Row 9: a FIFO named like a store file fails with `STORE_IO` instead of hanging, on a read
    /// of any of the three files and on a write whose target is one.
    #[test]
    fn fifo_store_file() {
        let cases: [(&str, &[&str]); 4] = [
            ("extensions.ttl", &["find"]),
            ("vocabulary.ttl", &["find"]),
            ("resources.ttl", &["find"]),
            ("vocabulary.ttl", &["concept", "add", "zeta"]),
        ];
        let mut running = Vec::new();
        let mut envs = Vec::new();
        for (name, args) in cases {
            let env = Env::with_store();
            std::fs::remove_file(env.data().join(name)).unwrap();
            mkfifo(&env.data().join(name));
            let mut all = vec!["--json"];
            all.extend(args);
            running.push((format!("{name} FIFO, {args:?}"), env.spawn(&[], all)));
            envs.push(env);
        }
        for (what, mut r) in running {
            drop(r.child.stdin.take());
            let out = r.finish_within(Duration::from_secs(10));
            assert_store_io(&out, &what);
            assert!(
                out.elapsed < Duration::from_secs(10),
                "{what}: {:?}",
                out.elapsed
            );
        }
    }

    fn sparse(path: &Path, len: u64) {
        let f = std::fs::File::create(path).unwrap();
        f.set_len(len).unwrap();
    }

    /// Row 9: a store file over 256 MiB is `STORE_IO`, rejected by size before it is read into
    /// memory (a sparse file, so the test is cheap); it is not `STORE_UNPARSEABLE`. A file of
    /// exactly 256 MiB is within the cap: it is read (and, being zeros, does not parse).
    #[test]
    fn oversized_store_file() {
        const CAP: u64 = 256 * 1024 * 1024;
        for name in ["extensions.ttl", "vocabulary.ttl", "resources.ttl"] {
            let env = Env::with_store();
            sparse(&env.data().join(name), CAP + 1);
            let out = env.run(["--json", "find"]);
            assert_store_io(&out, &format!("{name} of 256 MiB + 1"));
            assert!(out.elapsed < Duration::from_secs(20), "{:?}", out.elapsed);
        }
        let env = Env::with_store();
        sparse(&env.data().join("extensions.ttl"), CAP);
        let out = env.run(["--json", "find"]);
        assert_eq!(out.code, Some(3), "stderr: {}", out.stderr);
        assert_eq!(
            envelope(&out)["error"]["rule"],
            "STORE_UNPARSEABLE",
            "a file of exactly 256 MiB is within the cap"
        );
    }

    /// Row 1: a `.lock` that is not a regular file (a directory, a symlink, a FIFO) is `STORE_IO`
    /// for a writer, which neither hangs on it nor follows it.
    #[test]
    fn non_regular_lock_file() {
        enum Kind {
            Dir,
            Link,
            Fifo,
        }
        let mut running = Vec::new();
        let mut envs = Vec::new();
        for (name, kind) in [
            ("dir", Kind::Dir),
            ("link", Kind::Link),
            ("fifo", Kind::Fifo),
        ] {
            let env = Env::with_store();
            let at = env.lock_path();
            match kind {
                Kind::Dir => std::fs::create_dir(&at).unwrap(),
                Kind::Link => {
                    std::fs::write(env.root.join("lock-target"), b"keep\n").unwrap();
                    symlink(env.root.join("lock-target"), &at).unwrap();
                }
                Kind::Fifo => mkfifo(&at),
            }
            running.push((name, env.spawn(&[], ["--json", "concept", "add", "zeta"])));
            envs.push(env);
        }
        for ((name, mut r), env) in running.into_iter().zip(&envs) {
            drop(r.child.stdin.take());
            let out = r.finish_within(Duration::from_secs(10));
            assert_store_io(&out, &format!("`.lock` as a {name}"));
            assert!(
                !env.read("vocabulary.ttl").contains("zeta"),
                "{name}: wrote"
            );
        }
        assert_eq!(
            std::fs::read(envs[1].root.join("lock-target")).unwrap(),
            b"keep\n",
            "a symlinked `.lock` was written through"
        );
        assert!(
            std::fs::symlink_metadata(envs[1].lock_path())
                .unwrap()
                .is_symlink()
        );
    }

    /// Row 8: an owned, private data dir with a read-only store file is not a data dir problem:
    /// the dir check looks at the dir only (a read of a 0400 store file works).
    #[test]
    fn read_only_store_file_in_private_dir_is_readable() {
        let env = Env::with_store();
        for name in ["vocabulary.ttl", "resources.ttl", "extensions.ttl"] {
            std::fs::set_permissions(
                env.data().join(name),
                std::fs::Permissions::from_mode(0o400),
            )
            .unwrap();
        }
        let out = env.run(["--json", "find"]);
        assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
        assert_eq!(std::fs::metadata(env.data()).unwrap().mode() & 0o777, 0o700);
    }
}
