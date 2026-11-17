//! Goal tests for the `storage` seam (#23, spec §3, §4, §7): the remembered scan roots live in
//! `<state dir>/roots.json`, outside the store.
//!
//! Assumed API, module `hali::storage::state`:
//!   `ROOTS_FILE: &str` = `"roots.json"`.
//!   `read_roots(dir: &Path) -> Result<Option<String>, hali::storage::Error>`: the text of
//!       `<dir>/roots.json`; `Ok(None)` when `dir` or the file does not exist. It reads at most
//!       1 MiB (a file of exactly 1 MiB is read, one byte more is an error, found without
//!       reading the rest), never blocks (a FIFO is refused, not opened for reading), follows no
//!       symlink and refuses anything that is not a regular file or is not UTF-8. Every refusal
//!       is `Error::Io { path: <dir>/roots.json }`.
//!   `write_roots(dir: &Path, text: &str) -> Result<(), hali::storage::Error>`: writes the
//!       file atomically (a temp file in `dir`, then a rename), creating `dir` with mode 0700
//!       and the file with mode 0600, and leaves no other file behind. A failure is
//!       `Error::Io` with a path that is, or is under, the one that failed.

mod goal {
    use hali::storage::Error;
    use hali::storage::state::{ROOTS_FILE, read_roots, write_roots};
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::path::{Path, PathBuf};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    fn io_path(err: Error) -> PathBuf {
        match err {
            Error::Io { path } => path,
            other => panic!("expected Error::Io, got {other:?}"),
        }
    }

    fn mode(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn the_file_is_roots_json() {
        assert_eq!(ROOTS_FILE, "roots.json");
    }

    /// Row 3: A missing dir or file is `None`, and nothing is created.
    #[test]
    fn read_of_nothing_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(read_roots(&tmp.path().join("nope")).unwrap(), None);
        assert_eq!(read_roots(tmp.path()).unwrap(), None);
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 0);
    }

    /// Row 3: What was written is read back; a rewrite replaces it; no temp file is left.
    #[test]
    fn write_then_read() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("state/hali");
        write_roots(&dir, "first\n").unwrap();
        assert_eq!(read_roots(&dir).unwrap().as_deref(), Some("first\n"));
        write_roots(&dir, "second, longer text\n").unwrap();
        assert_eq!(
            read_roots(&dir).unwrap().as_deref(),
            Some("second, longer text\n")
        );
        assert_eq!(names(&dir), ["roots.json"], "no temp file is left behind");
        write_roots(&dir, "").unwrap();
        assert_eq!(read_roots(&dir).unwrap().as_deref(), Some(""));
        assert_eq!(names(&dir), ["roots.json"]);
    }

    /// Row 3: The dir is 0700 and the file 0600, and a rewrite makes a loosened file private
    /// again.
    #[test]
    fn modes_are_private() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("state/hali");
        write_roots(&dir, "x\n").unwrap();
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&dir.join("roots.json")), 0o600);

        std::fs::set_permissions(
            dir.join("roots.json"),
            std::fs::Permissions::from_mode(0o666),
        )
        .unwrap();
        write_roots(&dir, "y\n").unwrap();
        assert_eq!(mode(&dir.join("roots.json")), 0o600);
    }

    /// Row 3: A failed write reports a path and leaves what was there.
    #[test]
    fn write_failures_are_io_errors() {
        let tmp = tempfile::tempdir().unwrap();
        // The state dir path is a regular file.
        let blocked = tmp.path().join("blocked");
        std::fs::write(&blocked, "in the way").unwrap();
        let path = io_path(write_roots(&blocked, "x\n").unwrap_err());
        assert!(path.starts_with(&blocked), "{}", path.display());
        assert_eq!(std::fs::read_to_string(&blocked).unwrap(), "in the way");

        // roots.json is a directory: a file cannot replace it, and it is left alone.
        let dir = tmp.path().join("hali");
        std::fs::create_dir_all(dir.join("roots.json")).unwrap();
        assert!(write_roots(&dir, "x\n").is_err());
        assert!(dir.join("roots.json").is_dir());
        assert_eq!(names(&dir), ["roots.json"], "no temp file is left behind");
    }

    /// Row 3, 15: Anything that is not a regular file, or not UTF-8, is refused with the file's
    /// path.
    #[test]
    fn read_refuses_what_is_not_a_regular_utf8_file() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("roots.json");

        // A directory.
        std::fs::create_dir(&file).unwrap();
        assert_eq!(io_path(read_roots(tmp.path()).unwrap_err()), file);
        std::fs::remove_dir(&file).unwrap();

        // A symlink, to a regular file and to nothing.
        let target = tmp.path().join("real.json");
        std::fs::write(&target, "{}\n").unwrap();
        symlink(&target, &file).unwrap();
        assert_eq!(io_path(read_roots(tmp.path()).unwrap_err()), file);
        std::fs::remove_file(&file).unwrap();
        symlink(tmp.path().join("nowhere"), &file).unwrap();
        assert_eq!(io_path(read_roots(tmp.path()).unwrap_err()), file);
        std::fs::remove_file(&file).unwrap();

        // Not UTF-8.
        std::fs::write(&file, b"{\"a\":\"\xff\xfe\"}").unwrap();
        assert_eq!(io_path(read_roots(tmp.path()).unwrap_err()), file);
    }

    /// Row 3, 15: A FIFO is refused, not opened for reading (which would block for ever).
    #[test]
    fn read_of_a_fifo_does_not_block() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("roots.json");
        let c = CString::new(file.as_os_str().as_bytes()).unwrap();
        // SAFETY: `c` is a valid NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);

        let (tx, rx) = mpsc::channel();
        let dir = tmp.path().to_path_buf();
        std::thread::spawn(move || {
            let _ = tx.send(read_roots(&dir));
        });
        let result = rx
            .recv_timeout(Duration::from_secs(10))
            .expect("read_roots blocked on a FIFO");
        assert_eq!(io_path(result.unwrap_err()), file);
    }

    /// Row 3, 15: At most 1 MiB is read: a file of exactly 1 MiB is read, one byte more is
    /// refused, and a huge sparse file is refused without reading it.
    #[test]
    fn read_is_bounded_to_one_mebibyte() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("roots.json");

        let exact = " ".repeat(1 << 20);
        std::fs::write(&file, &exact).unwrap();
        assert_eq!(
            read_roots(tmp.path()).unwrap().as_deref(),
            Some(exact.as_str())
        );

        std::fs::write(&file, " ".repeat((1 << 20) + 1)).unwrap();
        assert_eq!(io_path(read_roots(tmp.path()).unwrap_err()), file);

        let f = std::fs::File::create(&file).unwrap();
        f.set_len(8 << 30).unwrap();
        drop(f);
        let started = Instant::now();
        assert_eq!(io_path(read_roots(tmp.path()).unwrap_err()), file);
        assert!(
            started.elapsed() < Duration::from_millis(1500),
            "read_roots read the whole file: {:?}",
            started.elapsed()
        );
    }
}
