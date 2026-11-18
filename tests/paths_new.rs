//! Goal tests for `paths::new_files_dir` (#25): the real target of files.dir when parts of it
//! are missing, a file or a dangling symlink. Contract (spec §9.1 **New:**): the deepest
//! existing prefix of the path must be a directory, else `not_a_directory`; `..` after a
//! missing component is resolved against the symlink-resolved prefix, so the recorded path is
//! always the real one.

#[cfg(feature = "goal")]
mod goal {
    use hali::paths::{FilesDirError, new_files_dir};
    use std::path::Path;

    #[test]
    fn dotdot_after_a_missing_component_uses_the_real_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        let target = root.join("target");
        std::fs::create_dir(&target).unwrap();
        std::os::unix::fs::symlink(&target, root.join("link")).unwrap();
        let home = root.to_str().unwrap().to_string();
        let env = |k: &str| (k == "HOME").then(|| home.clone());
        let own: [&Path; 0] = [];

        let dotted = format!("{home}/nope/../link/a");
        assert_eq!(
            new_files_dir(env, Some(&dotted), &own),
            Ok(format!("{home}/target/a"))
        );
        let deeper = format!("{home}/x/y/../../link/a/b");
        assert_eq!(
            new_files_dir(env, Some(&deeper), &own),
            Ok(format!("{home}/target/a/b"))
        );
        assert!(!root.join("nope").exists() && !target.join("a").exists());
    }

    #[test]
    fn the_deepest_existing_prefix_must_be_a_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        std::fs::write(root.join("afile"), b"x").unwrap();
        std::os::unix::fs::symlink(root.join("nowhere"), root.join("dangling")).unwrap();
        let home = root.to_str().unwrap().to_string();
        let env = |k: &str| (k == "HOME").then(|| home.clone());
        let own: [&Path; 0] = [];

        for dir in [
            format!("{home}/afile/sub"),
            format!("{home}/afile/a/b"),
            format!("{home}/dangling"),
            format!("{home}/dangling/sub"),
        ] {
            let got = new_files_dir(env, Some(&dir), &own);
            assert!(
                matches!(got, Err(FilesDirError::NotADirectory { .. })),
                "{dir}: {got:?}"
            );
        }
        // Sanity: a missing tail below a real directory is fine.
        assert_eq!(
            new_files_dir(env, Some(&format!("{home}/new/dir")), &own),
            Ok(format!("{home}/new/dir"))
        );
        assert!(!root.join("new").exists() && !root.join("nowhere").exists());
    }

    #[test]
    fn a_dotdot_after_a_blocking_file_stays_in_the_reported_path() {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        std::fs::write(root.join("afile"), b"x").unwrap();
        std::os::unix::fs::symlink(root.join("nowhere"), root.join("dangling")).unwrap();
        let home = root.to_str().unwrap().to_string();
        let env = |k: &str| (k == "HOME").then(|| home.clone());
        let own: [&Path; 0] = [];

        for (dir, shown) in [
            ("afile/../notes", "afile/../notes"),
            ("afile/a/../../notes", "afile/a/../../notes"),
            ("dangling/../notes", "dangling/../notes"),
        ] {
            assert_eq!(
                new_files_dir(env, Some(&format!("{home}/{dir}")), &own),
                Err(FilesDirError::NotADirectory {
                    path: format!("{home}/{shown}")
                }),
                "{dir}"
            );
        }
        assert!(!root.join("notes").exists());
    }
}
