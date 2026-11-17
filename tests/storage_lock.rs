//! Goal tests for the `storage` seam of the store lock (#5, spec §3.3).
//!
//! Assumed API (the issue's contract):
//!   `hali::storage::Writer::open(dir: &Path) -> Result<Writer, hali::storage::Error>`
//!       takes the exclusive lock on `<dir>/.lock` (creating the file 0600; waiting up to 5 s),
//!       but when `dir` does not exist it locks nothing and creates nothing;
//!   `Writer::load(&self) -> Result<hali::storage::Store, hali::storage::Error>`;
//!   `Writer::save(&self, store: &Store, file: hali::storage::File) -> Result<(), Error>`
//!       publishes one file by temp + rename; with no data dir yet it creates the dir (0700) and
//!       `.lock`, takes the exclusive lock and, when a store file now exists, fails with a
//!       `STORE_LOCKED` error of reason `changed` and writes nothing. The lock is held until the
//!       `Writer` is dropped;
//!   `hali::storage::load(dir: &Path) -> Result<Store, Error>` takes the shared lock itself (a
//!       missing `.lock` is read without one, and is not created);
//!   `hali::storage::dir_problem(uid: u32, mode: u32, euid: u32) -> Option<_>`: `Some` when the
//!       dir is not owned by the effective user, or `mode & 0o022 != 0`.
//! Errors convert to the CLI's error through `hali::core::Error::from(storage_error)`, whose
//! `rule`, `details` and `exit_code()` are what these tests read.

#[cfg(feature = "goal")]
mod goal {
    use hali::core::Error as CoreError;
    use hali::storage::{self, File, Writer};
    use std::fs::OpenOptions;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    /// An existing private data dir holding the `store` fixture.
    fn data_dir() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("data");
        std::fs::create_dir(&data).unwrap();
        std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o700)).unwrap();
        for name in ["vocabulary.ttl", "resources.ttl", "extensions.ttl"] {
            std::fs::copy(fixture("store").join(name), data.join(name)).unwrap();
        }
        (tmp, data)
    }

    fn lock_file(data: &Path) -> std::fs::File {
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(data.join(".lock"))
            .unwrap()
    }

    fn locked(err: storage::Error) -> CoreError {
        CoreError::from(err)
    }

    /// Row 1: with no data dir yet, two writers both open (nothing is locked, nothing created);
    /// the first publishes; the second's save then finds a store file where there was none and
    /// fails with `STORE_LOCKED`, reason `changed`, writing nothing.
    #[test]
    fn first_write_race_is_changed() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("data");
        let first = Writer::open(&data).unwrap();
        let second = Writer::open(&data).unwrap();
        assert!(!data.exists(), "`open` created the data dir");
        let store_one = first.load().unwrap();
        let store_two = second.load().unwrap();

        first.save(&store_one, File::Vocabulary).unwrap();
        let written = std::fs::read(data.join("vocabulary.ttl")).unwrap();
        assert_eq!(
            written,
            storage::to_turtle(&store_one, File::Vocabulary).into_bytes()
        );
        drop(first);

        let err = locked(second.save(&store_two, File::Vocabulary).unwrap_err());
        assert_eq!(err.rule, "STORE_LOCKED");
        assert_eq!(err.exit_code(), 3);
        assert_eq!(err.details["reason"], "changed");
        assert!(
            err.details["path"]
                .as_str()
                .unwrap()
                .ends_with("/data/.lock"),
            "{}",
            err.details
        );
        assert_eq!(std::fs::read(data.join("vocabulary.ttl")).unwrap(), written);
        let names: Vec<String> = std::fs::read_dir(&data)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .filter(|n| n != ".lock" && n != "vocabulary.ttl")
            .collect();
        assert!(names.is_empty(), "the loser left {names:?}");
    }

    /// Row 1: the first save into an absent dir creates it (0700) and `.lock` (0600), which stays.
    #[test]
    fn first_save_creates_dir_and_lock() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("data");
        let writer = Writer::open(&data).unwrap();
        let store = writer.load().unwrap();
        writer.save(&store, File::Resources).unwrap();
        drop(writer);
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&data), 0o700);
        assert_eq!(mode(&data.join(".lock")), 0o600);
        assert_eq!(mode(&data.join("resources.ttl")), 0o600);
        assert_eq!(std::fs::metadata(data.join(".lock")).unwrap().len(), 0);
    }

    /// Row 1: `open` on an existing data dir creates `.lock` (0600) and holds the exclusive lock
    /// until the writer is dropped.
    #[test]
    fn writer_holds_the_exclusive_lock_until_dropped() {
        let (_tmp, data) = data_dir();
        let writer = Writer::open(&data).unwrap();
        let mode = std::fs::metadata(data.join(".lock"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        let probe = lock_file(&data);
        assert!(
            probe.try_lock().is_err(),
            "the writer does not hold the lock"
        );
        assert!(
            probe.try_lock_shared().is_err(),
            "a reader could share a writer's lock"
        );
        drop(writer);
        probe.try_lock().expect("the lock outlived the writer");
    }

    /// Row 1: a reader's `load` waits for an exclusive holder (released after 1 s) and then reads.
    #[test]
    fn load_waits_for_an_exclusive_lock() {
        let (_tmp, data) = data_dir();
        let held = lock_file(&data);
        held.lock().unwrap();
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(1));
            drop(held);
        });
        let started = Instant::now();
        let store = storage::load(&data).unwrap();
        assert!(
            started.elapsed() >= Duration::from_millis(900),
            "load did not wait for the exclusive lock: {:?}",
            started.elapsed()
        );
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(store.len().unwrap() > 0);
        releaser.join().unwrap();
    }

    /// Row 1: a reader shares with other readers, and the shared lock is not kept after `load`
    /// returns (a writer can lock at once).
    #[test]
    fn load_shares_and_does_not_keep_the_lock() {
        let (_tmp, data) = data_dir();
        let held = lock_file(&data);
        held.lock_shared().unwrap();
        let started = Instant::now();
        storage::load(&data).unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );
        drop(held);
        let probe = lock_file(&data);
        probe
            .try_lock()
            .expect("`load` kept its shared lock after it returned");
    }

    /// Row 1: a writer's `open` waits for a shared holder (released after 1 s), then succeeds.
    #[test]
    fn writer_open_waits_for_a_shared_lock() {
        let (_tmp, data) = data_dir();
        let held = lock_file(&data);
        held.lock_shared().unwrap();
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(1));
            drop(held);
        });
        let started = Instant::now();
        let writer = Writer::open(&data).unwrap();
        assert!(
            started.elapsed() >= Duration::from_millis(900),
            "open did not wait: {:?}",
            started.elapsed()
        );
        drop(writer);
        releaser.join().unwrap();
    }

    /// Row 3: `Writer::open` gives up after 5 s with `STORE_LOCKED`, reason `timeout`, naming the
    /// lock file.
    #[test]
    fn writer_open_times_out() {
        let (_tmp, data) = data_dir();
        let held = lock_file(&data);
        held.lock().unwrap();
        let started = Instant::now();
        let err = locked(
            Writer::open(&data)
                .err()
                .expect("open must not get the lock"),
        );
        let waited = started.elapsed();
        assert!(waited >= Duration::from_millis(4900), "{waited:?}");
        assert!(waited < Duration::from_secs(10), "{waited:?}");
        assert_eq!(err.rule, "STORE_LOCKED");
        assert_eq!(err.exit_code(), 3);
        assert_eq!(err.details["reason"], "timeout");
        assert_eq!(
            err.details["path"].as_str().unwrap(),
            data.join(".lock").to_str().unwrap()
        );
        assert_eq!(
            err.message,
            format!(
                "another hali process holds the store lock {}; gave up after 5 s",
                data.join(".lock").display()
            )
        );
    }

    /// Row 1: a reader creates no `.lock`, in an existing dir or an absent one.
    #[test]
    fn load_creates_no_lock_file() {
        let (_tmp, data) = data_dir();
        storage::load(&data).unwrap();
        assert!(!data.join(".lock").exists(), "load created .lock");
        let tmp = tempfile::tempdir().unwrap();
        let absent = tmp.path().join("nope");
        storage::load(&absent).unwrap();
        assert!(!absent.exists());
    }

    /// Row 8: the data dir rule, pure: not owned by the effective user, or group- or
    /// world-writable, is a problem; read and search bits for others are not.
    #[test]
    fn dir_problem_rules() {
        let euid = 1000;
        for mode in [0o700, 0o750, 0o755, 0o500, 0o555, 0o711] {
            assert!(
                storage::dir_problem(euid, mode, euid).is_none(),
                "{mode:o} owned by the user is fine"
            );
        }
        for mode in [
            0o770, 0o775, 0o777, 0o720, 0o702, 0o707, 0o722, 0o1777, 0o2770,
        ] {
            assert!(
                storage::dir_problem(euid, mode, euid).is_some(),
                "{mode:o} is group- or world-writable"
            );
        }
        for mode in [0o700, 0o755, 0o777] {
            assert!(
                storage::dir_problem(0, mode, euid).is_some(),
                "{mode:o} owned by root is not the user's"
            );
            assert!(
                storage::dir_problem(euid + 1, mode, euid).is_some(),
                "{mode:o} owned by another user"
            );
        }
        // Root is not special: a root-owned 0755 dir is fine for root.
        assert!(storage::dir_problem(0, 0o755, 0).is_none());
        assert!(storage::dir_problem(0, 0o775, 0).is_some());
    }
}
