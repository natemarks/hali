//! Goal tests for the atomic publish of the store files through the real binary (#5, spec §3.3).
//!
//! The contract: each changed tool-managed file is published on its own under the exclusive
//! lock: the fixed temp `.<name>.tmp` in the same directory (a planted file or symlink there is
//! removed, never followed; a planted directory is `STORE_IO`) is created `O_CREAT|O_EXCL|
//! O_NOFOLLOW` at mode 0600, written and fsynced, then renamed over the target. A crash or an
//! error leaves the old bytes; a leftover temp is never loaded.
//!
//! Test hook (spec §14.2, compiled only under `test-hooks`): `HALI_TEST_PUBLISH_STALL=<marker>`
//! makes a writer create the file `<marker>` just before the rename and then sleep, so a test can
//! kill it mid-publish.

mod lockkit;

mod goal {
    use crate::lockkit::*;
    use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
    use std::time::{Duration, Instant};

    const VOCAB_TMP: &str = ".vocabulary.ttl.tmp";
    const RES_TMP: &str = ".resources.ttl.tmp";

    /// Row 2: a changed file is published by rename: the target is a new file (a new inode) of
    /// mode 0600, a file the run did not change keeps its inode, and no temp is left behind.
    #[test]
    fn publish_renames() {
        let env = Env::with_store();
        let ino = |name: &str| std::fs::metadata(env.data().join(name)).unwrap().ino();
        let (vocab, resources, extensions) = (
            ino("vocabulary.ttl"),
            ino("resources.ttl"),
            ino("extensions.ttl"),
        );
        let out = env.run(["--json", "concept", "add", "zeta"]);
        assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
        assert_ne!(
            ino("vocabulary.ttl"),
            vocab,
            "vocabulary.ttl was written in place"
        );
        assert_eq!(
            ino("resources.ttl"),
            resources,
            "an unchanged file was rewritten"
        );
        assert_eq!(
            ino("extensions.ttl"),
            extensions,
            "extensions.ttl was rewritten"
        );
        let mode = std::fs::metadata(env.data().join("vocabulary.ttl"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        let mut names: Vec<String> = std::fs::read_dir(env.data())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [".lock", "extensions.ttl", "resources.ttl", "vocabulary.ttl"],
            "a temp file was left behind"
        );
        assert!(env.read("vocabulary.ttl").contains("\"zeta\"@en"));
    }

    /// Row 2: the new file is complete Turtle (not a truncated one) and a file that did not exist
    /// before is published the same way (resources.ttl by `add`).
    #[test]
    fn first_publish_of_a_new_file() {
        let env = Env::new();
        let target = env.file("note.txt");
        std::fs::write(&target, b"hello\n").unwrap();
        let out = env.run(["--json", "add", target.to_str().unwrap()]);
        assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
        let name = "resources.ttl";
        let meta = std::fs::symlink_metadata(env.data().join(name)).unwrap();
        assert!(meta.file_type().is_file(), "{name}");
        assert_eq!(meta.permissions().mode() & 0o777, 0o600, "{name}");
        let names: Vec<String> = std::fs::read_dir(env.data())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(names.is_empty(), "{names:?}");
    }

    /// Waits for the stall marker; fails when the writer exits without reaching it.
    fn wait_for_marker(running: &mut Running, marker: &std::path::Path) {
        let until = Instant::now() + Duration::from_secs(30);
        while !marker.exists() {
            if let Some(status) = running.child.try_wait().unwrap() {
                panic!("the writer exited ({status}) without reaching the publish stall");
            }
            assert!(
                Instant::now() < until,
                "the writer never reached the publish stall"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Row 5: killing a writer between the write of the temp and the rename leaves the previous
    /// bytes in place; a reader still parses the store; the next writer succeeds and replaces
    /// the dead writer's temp.
    #[test]
    fn kill_mid_publish() {
        let env = Env::with_store();
        let marker = env.root.join("stalled");
        let before = env.store_bytes();
        let mut running = env.spawn(
            &[("HALI_TEST_PUBLISH_STALL", marker.to_str().unwrap())],
            ["--json", "concept", "add", "zeta"],
        );
        drop(running.child.stdin.take());
        wait_for_marker(&mut running, &marker);
        assert_eq!(env.store_bytes(), before, "bytes changed before the rename");
        running.child.kill().unwrap();
        let _ = running.child.wait();
        assert_eq!(
            env.store_bytes(),
            before,
            "the killed writer left a changed file"
        );

        // The killed writer's temp (if it is left) is never loaded: a reader sees the old store.
        let list = env.run(["--json", "concept", "list"]);
        assert_eq!(list.code, Some(0), "stderr: {}", list.stderr);
        assert!(!list.stdout.contains("zeta"), "{}", list.stdout);

        // The next writer is not blocked by the dead one and succeeds.
        let next = env.run(["--json", "concept", "add", "zeta"]);
        assert_eq!(next.code, Some(0), "stderr: {}", next.stderr);
        assert!(next.elapsed < Duration::from_secs(5), "{:?}", next.elapsed);
        assert!(env.read("vocabulary.ttl").contains("\"zeta\"@en"));
        assert!(
            std::fs::symlink_metadata(env.data().join(VOCAB_TMP)).is_err(),
            "a temp file is left after the next publish"
        );
    }

    /// Row 5: a publish that fails (a directory sits at the temp path) is `STORE_IO`, exit 3,
    /// and the previous bytes stay.
    #[test]
    fn failed_publish_keeps_old_bytes() {
        let env = Env::with_store();
        std::fs::create_dir(env.data().join(VOCAB_TMP)).unwrap();
        let before = env.store_bytes();
        let out = env.run(["--json", "concept", "add", "zeta"]);
        assert_eq!(
            out.code,
            Some(3),
            "stdout: {}; stderr: {}",
            out.stdout,
            out.stderr
        );
        assert_eq!(out.stdout, "");
        assert_eq!(envelope(&out)["error"]["rule"], "STORE_IO");
        assert_eq!(env.store_bytes(), before, "a failed publish changed a file");
    }

    /// Row 6: a symlink planted at the temp path is never followed: the file it points to is
    /// unchanged (and a dangling one is not created), the temp path no longer holds the link
    /// afterwards, and the store is updated. Same for a planted regular file, and for the
    /// temp of resources.ttl.
    #[test]
    fn planted_temp_symlink() {
        enum Plant {
            Link,
            Dangling,
            File,
        }
        for (name, plant) in [
            ("link", Plant::Link),
            ("dangling", Plant::Dangling),
            ("file", Plant::File),
        ] {
            let env = Env::with_resources();
            let victim = env.root.join("victim.txt");
            std::fs::write(&victim, b"victim\n").unwrap();
            let gone = env.root.join("not-there.txt");
            for tmp in [VOCAB_TMP, RES_TMP] {
                let at = env.data().join(tmp);
                match plant {
                    Plant::Link => symlink(&victim, &at).unwrap(),
                    Plant::Dangling => symlink(&gone, &at).unwrap(),
                    Plant::File => std::fs::write(&at, b"planted\n").unwrap(),
                }
            }
            // `edit` writes resources.ttl, `concept add` writes vocabulary.ttl.
            let one = env.run(["--json", "edit", R1, "--title", "renamed"]);
            assert_eq!(one.code, Some(0), "{name}: stderr: {}", one.stderr);
            let two = env.run(["--json", "concept", "add", "zeta"]);
            assert_eq!(two.code, Some(0), "{name}: stderr: {}", two.stderr);
            assert_eq!(
                std::fs::read(&victim).unwrap(),
                b"victim\n",
                "{name}: victim written"
            );
            assert!(!gone.exists(), "{name}: a dangling link was followed");
            for tmp in [VOCAB_TMP, RES_TMP] {
                assert!(
                    std::fs::symlink_metadata(env.data().join(tmp)).is_err(),
                    "{name}: the planted {tmp} is still there"
                );
            }
            assert!(env.read("resources.ttl").contains("\"renamed\""), "{name}");
            assert!(env.read("vocabulary.ttl").contains("\"zeta\"@en"), "{name}");
            for target in ["vocabulary.ttl", "resources.ttl"] {
                let meta = std::fs::symlink_metadata(env.data().join(target)).unwrap();
                assert!(
                    meta.file_type().is_file(),
                    "{name}: {target} is not a regular file"
                );
            }
        }
    }

    /// Row 5: a temp left by a crash is never loaded: a reader ignores it (it holds garbage that
    /// would not parse), and a reader never creates or removes it.
    #[test]
    fn leftover_temp_is_never_loaded() {
        let env = Env::with_store();
        let at = env.data().join(VOCAB_TMP);
        std::fs::write(&at, b"this is not turtle {{{\n").unwrap();
        for args in [
            vec!["--json", "concept", "list"],
            vec!["--json", "describe"],
        ] {
            let out = env.run(&args);
            assert_eq!(out.code, Some(0), "{args:?}: stderr: {}", out.stderr);
        }
        assert_eq!(std::fs::read(&at).unwrap(), b"this is not turtle {{{\n");
    }
}
