//! Goal tests for the `storage` seam of `hali new` (#25, spec §8): creating a new file safely.
//!
//! Assumed API (invented here; the implementer follows it), appended at the end of
//! `src/storage/mod.rs`:
//!
//! ```text
//! pub struct CreatedFile { .. }             // opaque: remembers the descriptor's dev and ino
//! #[derive(Debug)] pub enum CreateError { .. }
//! pub fn create_new(path: &Path, bytes: &[u8], mtime: i64) -> Result<CreatedFile, CreateError>
//! pub fn remove_created(created: &CreatedFile)
//! ```
//!
//! `create_new` makes the regular file at `path`, whose directory exists, with
//! `O_CREAT|O_EXCL|O_NOFOLLOW|O_CLOEXEC|O_NOCTTY` (mode 0666 minus the umask), writes `bytes`,
//! sets its mtime to `mtime` (whole seconds since the Unix epoch) and `fsync`s it. Anything
//! already at `path` (a file, a directory, a symlink, a dangling symlink) is an error and is
//! left exactly as it was; a symlink is never followed. `remove_created` unlinks `path` only
//! while `lstat` there still reports the dev and ino of the descriptor `create_new` opened;
//! otherwise it leaves whatever is there. It never removes the directory.

#[cfg(feature = "goal")]
mod goal {
    use hali::storage::{create_new, remove_created};
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    const MTIME: i64 = 1_792_834_200; // 2026-10-24T09:30:00Z

    #[test]
    fn create_new_writes_bytes_and_mtime() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        create_new(&path, b"# A\n", MTIME).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"# A\n");
        let meta = std::fs::symlink_metadata(&path).unwrap();
        assert!(meta.is_file());
        assert_eq!(meta.mtime(), MTIME);
        let mode = meta.permissions().mode();
        assert_eq!(mode & 0o111, 0, "{mode:o}: no execute bit");
        assert_eq!(mode & 0o600, 0o600, "{mode:o}: the owner reads and writes");
        // Empty content is fine too.
        let empty = dir.path().join("empty.md");
        create_new(&empty, b"", MTIME).unwrap();
        assert_eq!(std::fs::read(&empty).unwrap(), b"");
    }

    #[test]
    fn create_new_is_exclusive_nofollow() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // A file: untouched.
        let file = root.join("file.md");
        std::fs::write(&file, b"mine\n").unwrap();
        assert!(create_new(&file, b"# new\n", MTIME).is_err());
        assert_eq!(std::fs::read(&file).unwrap(), b"mine\n");
        assert_ne!(std::fs::metadata(&file).unwrap().mtime(), MTIME);

        // A directory.
        let sub = root.join("sub.md");
        std::fs::create_dir(&sub).unwrap();
        assert!(create_new(&sub, b"# new\n", MTIME).is_err());
        assert!(sub.is_dir());
        assert_eq!(std::fs::read_dir(&sub).unwrap().count(), 0);

        // A symlink to a file: the file is not written through it.
        let victim = root.join("victim.txt");
        std::fs::write(&victim, b"precious\n").unwrap();
        let link = root.join("link.md");
        std::os::unix::fs::symlink(&victim, &link).unwrap();
        assert!(create_new(&link, b"# new\n", MTIME).is_err());
        assert_eq!(std::fs::read(&victim).unwrap(), b"precious\n");
        assert_ne!(std::fs::metadata(&victim).unwrap().mtime(), MTIME);
        assert!(std::fs::symlink_metadata(&link).unwrap().is_symlink());

        // A dangling symlink: its target is not created.
        let nowhere = root.join("nowhere.txt");
        let dangling = root.join("dangling.md");
        std::os::unix::fs::symlink(&nowhere, &dangling).unwrap();
        assert!(create_new(&dangling, b"# new\n", MTIME).is_err());
        assert!(!nowhere.exists(), "a dangling symlink is never followed");
        assert!(std::fs::symlink_metadata(&dangling).unwrap().is_symlink());

        // A symlink to a directory.
        let target_dir = root.join("target");
        std::fs::create_dir(&target_dir).unwrap();
        let dir_link = root.join("dirlink.md");
        std::os::unix::fs::symlink(&target_dir, &dir_link).unwrap();
        assert!(create_new(&dir_link, b"# new\n", MTIME).is_err());
        assert_eq!(std::fs::read_dir(&target_dir).unwrap().count(), 0);

        // A second create at a path just created is refused, and keeps the first's bytes.
        let once = root.join("once.md");
        create_new(&once, b"first\n", MTIME).unwrap();
        assert!(create_new(&once, b"second\n", MTIME).is_err());
        assert_eq!(std::fs::read(&once).unwrap(), b"first\n");
    }

    #[test]
    fn remove_created_removes_the_file_and_keeps_the_directory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        let created = create_new(&path, b"# A\n", MTIME).unwrap();
        remove_created(&created);
        assert!(!path.exists());
        assert!(
            std::fs::symlink_metadata(&path).is_err(),
            "nothing is left at the path"
        );
        assert!(dir.path().is_dir());
    }

    #[test]
    fn remove_created_leaves_a_file_that_was_swapped_in() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        let created = create_new(&path, b"# A\n", MTIME).unwrap();
        // Another file takes the name: renamed over it, so it has an inode of its own.
        let other = dir.path().join("other.md");
        std::fs::write(&other, b"not yours\n").unwrap();
        std::fs::rename(&other, &path).unwrap();
        remove_created(&created);
        assert_eq!(std::fs::read(&path).unwrap(), b"not yours\n");
    }

    #[test]
    fn remove_created_leaves_a_symlink_that_was_swapped_in() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        let created = create_new(&path, b"# A\n", MTIME).unwrap();
        let victim = dir.path().join("victim.txt");
        std::fs::write(&victim, b"precious\n").unwrap();
        let swap = dir.path().join("swap");
        std::os::unix::fs::symlink(&victim, &swap).unwrap();
        std::fs::rename(&swap, &path).unwrap();
        remove_created(&created);
        assert!(std::fs::symlink_metadata(&path).unwrap().is_symlink());
        assert_eq!(std::fs::read(&victim).unwrap(), b"precious\n");
    }

    #[test]
    fn remove_created_of_a_file_already_gone_is_harmless() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        let created = create_new(&path, b"# A\n", MTIME).unwrap();
        std::fs::remove_file(&path).unwrap();
        remove_created(&created);
        assert!(!path.exists());
    }
}
