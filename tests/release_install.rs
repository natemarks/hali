//! Goal of #33 (criteria 12 and 13): `scripts/install.sh`, run from a staged directory with a shell
//! stub `hali` and a temporary HOME. Spec §4 gives the install locations.

mod release_support;

mod install {
    use std::collections::BTreeMap;
    use std::fs;
    use std::os::unix::fs::{PermissionsExt, symlink as make_link};
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use crate::release_support::{Run, exec, read_script, tree_listing, write, write_mode};

    const STUB: &str = "#!/bin/sh\nprintf '%s\\n' \"$*\" >>\"${STUB_LOG:-/dev/null}\"\ncase \"$1 $2\" in\n  'skill --help') exit \"${STUB_SKILL_HELP_EXIT:-0}\" ;;\n  'skill install') exit \"${STUB_SKILL_INSTALL_EXIT:-0}\" ;;\nesac\nexit 0\n";

    /// destination (relative to HOME), source (relative to the stage), mode
    const FILES: [(&str, &str, u32); 5] = [
        (".local/bin/hali", "hali", 0o755),
        (".local/share/man/man1/hali.1", "hali.1", 0o644),
        (
            ".local/share/bash-completion/completions/hali",
            "completions/hali.bash",
            0o644,
        ),
        (
            ".local/share/zsh/site-functions/_hali",
            "completions/_hali",
            0o644,
        ),
        (
            ".config/fish/completions/hali.fish",
            "completions/hali.fish",
            0o644,
        ),
    ];

    struct Env {
        tmp: tempfile::TempDir,
        stage: PathBuf,
        home: PathBuf,
        cwd: PathBuf,
        log: PathBuf,
    }

    fn setup() -> Env {
        let tmp = tempfile::tempdir().unwrap();
        let stage = tmp.path().join("stage");
        let home = tmp.path().join("home");
        let cwd = tmp.path().join("cwd");
        for d in [&stage, &home, &cwd] {
            fs::create_dir_all(d).unwrap();
        }
        write_mode(&stage.join("install.sh"), &read_script("install.sh"), 0o755);
        write_mode(&stage.join("hali"), STUB, 0o755);
        // Sources with a restrictive mode: the install must set the modes itself.
        write_mode(&stage.join("hali.1"), "stub man page\n", 0o600);
        write_mode(
            &stage.join("completions/hali.bash"),
            "stub bash completion\n",
            0o600,
        );
        write_mode(
            &stage.join("completions/_hali"),
            "stub zsh completion\n",
            0o600,
        );
        write_mode(
            &stage.join("completions/hali.fish"),
            "stub fish completion\n",
            0o600,
        );
        write(&stage.join("README.md"), "stub readme\n");
        Env {
            log: tmp.path().join("stub.log"),
            tmp,
            stage,
            home,
            cwd,
        }
    }

    impl Env {
        /// Run install.sh as a user would: from the stage, with only HOME and PATH (plus the stub's
        /// own variables, and some XDG noise the script must ignore), under a strict umask.
        fn run(&self, vars: &[(&str, &str)]) -> Run {
            self.run_with_home(Some(self.home.to_str().unwrap()), vars)
        }

        fn run_with_home(&self, home: Option<&str>, vars: &[(&str, &str)]) -> Run {
            let mut c = Command::new("/bin/sh");
            c.args([
                "-c",
                "umask 077; exec \"$0\"",
                self.stage.join("install.sh").to_str().unwrap(),
            ])
            .env_clear()
            .current_dir(&self.cwd)
            .env("PATH", "/usr/bin:/bin")
            .env("STUB_LOG", &self.log)
            .env("XDG_DATA_HOME", self.tmp.path().join("xdg-data"))
            .env("XDG_CONFIG_HOME", self.tmp.path().join("xdg-config"))
            .env("XDG_BIN_HOME", self.tmp.path().join("xdg-bin"));
            if let Some(h) = home {
                c.env("HOME", h);
            }
            for (k, v) in vars {
                c.env(k, v);
            }
            exec(&mut c, "scripts/install.sh")
        }

        fn log(&self) -> String {
            fs::read_to_string(&self.log).unwrap_or_default()
        }

        fn dest(&self, rel: &str) -> PathBuf {
            self.home.join(rel)
        }

        fn tree(&self) -> BTreeMap<String, String> {
            tree_listing(&self.home, &[])
        }
    }

    fn mode_of(p: &Path) -> u32 {
        fs::symlink_metadata(p).unwrap().permissions().mode() & 0o777
    }

    fn mentions_relogin(text: &str) -> bool {
        let t = text.to_lowercase();
        ["log out", "log in", "login", "re-login", "relogin"]
            .iter()
            .any(|k| t.contains(k))
    }

    /// The five installed files exist with the right mode and content.
    fn assert_installed(e: &Env) {
        for (dest, src, mode) in FILES {
            let d = e.dest(dest);
            let m = fs::symlink_metadata(&d).unwrap_or_else(|err| panic!("{dest} missing: {err}"));
            assert!(m.file_type().is_file(), "{dest} is not a regular file");
            assert_eq!(mode_of(&d), mode, "{dest} mode");
            assert_eq!(
                fs::read(&d).unwrap(),
                fs::read(e.stage.join(src)).unwrap(),
                "{dest} content"
            );
        }
    }

    #[test]
    #[ignore = "needs bash, git, tar, shellcheck, jq, cargo-edit (make test-external)"]
    fn xdg_paths() {
        let e = setup();
        let r = e.run(&[]);
        assert!(r.ok(), "{}", r.dump());
        assert_installed(&e);
        // Nothing else under HOME: exactly these files, and no directory that is not their ancestor.
        let mut expected_dirs = std::collections::BTreeSet::new();
        let mut expected_files = std::collections::BTreeSet::new();
        for (dest, _, _) in FILES {
            expected_files.insert(dest.to_string());
            let mut p = Path::new(dest).parent();
            while let Some(d) = p {
                if !d.as_os_str().is_empty() {
                    expected_dirs.insert(d.to_string_lossy().into_owned());
                }
                p = d.parent();
            }
        }
        for (path, what) in e.tree() {
            if what.starts_with("dir") {
                assert!(
                    expected_dirs.contains(&path),
                    "stray directory under HOME: {path}"
                );
            } else {
                assert!(
                    expected_files.contains(&path),
                    "stray entry under HOME: {path} ({what})"
                );
            }
        }
        // XDG variables are not read: spec §4 fixes these locations.
        assert!(!e.tmp.path().join("xdg-data").exists());
        assert!(!e.tmp.path().join("xdg-config").exists());
        assert!(!e.tmp.path().join("xdg-bin").exists());
        // The stage is untouched.
        assert!(e.stage.join("hali").exists());
        // No claude dir, so no skill call.
        assert!(!e.log().contains("skill install"));
    }

    #[test]
    #[ignore = "needs bash, git, tar, shellcheck, jq, cargo-edit (make test-external)"]
    fn symlink() {
        let e = setup();
        for (i, (dest, _, _)) in FILES.iter().enumerate() {
            let victim = e.tmp.path().join(format!("victim-{i}"));
            let d = e.dest(dest);
            fs::create_dir_all(d.parent().unwrap()).unwrap();
            if i % 2 == 0 {
                write(&victim, "VICTIM\n");
            }
            // Odd entries: a dangling link, which must not be created through either.
            make_link(&victim, &d).unwrap();
        }
        let r = e.run(&[]);
        assert!(r.ok(), "{}", r.dump());
        assert_installed(&e);
        for (i, (dest, _, _)) in FILES.iter().enumerate() {
            let victim = e.tmp.path().join(format!("victim-{i}"));
            if i % 2 == 0 {
                assert_eq!(
                    fs::read_to_string(&victim).unwrap(),
                    "VICTIM\n",
                    "wrote through the link at {dest}"
                );
            } else {
                assert!(
                    !victim.exists(),
                    "created the target of the dangling link at {dest}"
                );
            }
        }
    }

    #[test]
    #[ignore = "needs bash, git, tar, shellcheck, jq, cargo-edit (make test-external)"]
    fn directory() {
        for (dest, _, _) in FILES {
            let e = setup();
            let d = e.dest(dest);
            fs::create_dir_all(&d).unwrap();
            let r = e.run(&[]);
            assert!(
                !r.ok(),
                "{dest}: a directory at the destination must be refused\n{}",
                r.dump()
            );
            assert!(
                !r.stderr.trim().is_empty(),
                "{dest}: no message\n{}",
                r.dump()
            );
            assert!(
                fs::symlink_metadata(&d).unwrap().is_dir(),
                "{dest} is no longer a directory"
            );
            assert_eq!(
                fs::read_dir(&d).unwrap().count(),
                0,
                "{dest}: something was put inside the directory"
            );
            // Earlier files may already be installed, but nothing may be left that is not a final
            // name: no temp files anywhere under HOME.
            let allowed: Vec<&str> = FILES.iter().map(|(p, _, _)| *p).collect();
            for (path, what) in e.tree() {
                if what.starts_with("file") {
                    assert!(
                        allowed.contains(&path.as_str()),
                        "temp file left behind after refusing {dest}: {path}"
                    );
                }
            }
        }
    }

    #[test]
    #[ignore = "needs bash, git, tar, shellcheck, jq, cargo-edit (make test-external)"]
    fn relogin() {
        // ~/.local/bin is created by this run: warn.
        let e = setup();
        let r = e.run(&[]);
        assert!(r.ok(), "{}", r.dump());
        assert!(
            mentions_relogin(&r.combined()),
            "no re-login warning when ~/.local/bin was created\n{}",
            r.dump()
        );

        // ~/.local/bin already exists: no warning.
        let e = setup();
        fs::create_dir_all(e.dest(".local/bin")).unwrap();
        let r = e.run(&[]);
        assert!(r.ok(), "{}", r.dump());
        assert!(
            !mentions_relogin(&r.combined()),
            "re-login warning although ~/.local/bin existed\n{}",
            r.dump()
        );
        assert_installed(&e);
    }

    #[test]
    #[ignore = "needs bash, git, tar, shellcheck, jq, cargo-edit (make test-external)"]
    fn idempotent() {
        let e = setup();
        let first = e.run(&[]);
        assert!(first.ok(), "{}", first.dump());
        let after_first = e.tree();
        let second = e.run(&[]);
        assert!(second.ok(), "{}", second.dump());
        assert_eq!(
            after_first,
            e.tree(),
            "a second run changed the installed tree"
        );
        assert!(
            !mentions_relogin(&second.combined()),
            "second run warned about re-login\n{}",
            second.dump()
        );
        assert_installed(&e);
    }

    #[test]
    #[ignore = "needs bash, git, tar, shellcheck, jq, cargo-edit (make test-external)"]
    fn bad_home() {
        let cases: [(&str, Option<&str>); 5] = [
            ("empty", Some("")),
            ("relative", Some("relative/home")),
            ("dot", Some(".")),
            ("tilde", Some("~")),
            ("unset", None),
        ];
        for (name, home) in cases {
            let e = setup();
            let r = e.run_with_home(home, &[]);
            assert!(!r.ok(), "HOME {name}: must be refused\n{}", r.dump());
            assert!(
                !r.stderr.trim().is_empty(),
                "HOME {name}: no message\n{}",
                r.dump()
            );
            assert_eq!(
                fs::read_dir(&e.cwd).unwrap().count(),
                0,
                "HOME {name}: wrote into the working directory"
            );
            assert_eq!(
                fs::read_dir(&e.home).unwrap().count(),
                0,
                "HOME {name}: wrote into the real HOME dir"
            );
            assert!(!e.stage.join("relative").exists());
        }
    }

    #[test]
    #[ignore = "needs bash, git, tar, shellcheck, jq, cargo-edit (make test-external)"]
    fn skill_with_claude() {
        let e = setup();
        fs::create_dir_all(e.dest(".claude")).unwrap();
        let r = e.run(&[]);
        assert!(r.ok(), "{}", r.dump());
        let log = e.log();
        assert!(
            log.lines().any(|l| l.starts_with("skill --help")),
            "skill --help was not probed:\n{log}"
        );
        assert!(
            log.lines().any(|l| l.starts_with("skill install")),
            "skill install was not run:\n{log}"
        );
        assert_installed(&e);
    }

    #[test]
    #[ignore = "needs bash, git, tar, shellcheck, jq, cargo-edit (make test-external)"]
    fn no_claude() {
        // No ~/.claude at all.
        let e = setup();
        let r = e.run(&[]);
        assert!(r.ok(), "{}", r.dump());
        assert!(
            !e.log().contains("skill install"),
            "ran skill install without ~/.claude"
        );
        assert!(
            r.combined().to_lowercase().contains("skill")
                || r.combined().to_lowercase().contains("claude"),
            "no note about the skipped skill step\n{}",
            r.dump()
        );
        assert!(!e.dest(".claude").exists(), "created ~/.claude");

        // ~/.claude is a file, not a directory.
        let e = setup();
        write(&e.dest(".claude"), "not a directory\n");
        let r = e.run(&[]);
        assert!(r.ok(), "{}", r.dump());
        assert!(
            !e.log().contains("skill install"),
            "ran skill install with a ~/.claude that is a file"
        );
    }

    #[test]
    #[ignore = "needs bash, git, tar, shellcheck, jq, cargo-edit (make test-external)"]
    fn no_skill_subcommand() {
        let e = setup();
        fs::create_dir_all(e.dest(".claude")).unwrap();
        let r = e.run(&[("STUB_SKILL_HELP_EXIT", "2")]);
        assert!(
            r.ok(),
            "an old hali without `skill` must not fail the install\n{}",
            r.dump()
        );
        let log = e.log();
        assert!(
            log.lines().any(|l| l.starts_with("skill --help")),
            "skill --help was not probed:\n{log}"
        );
        assert!(
            !log.lines().any(|l| l.starts_with("skill install")),
            "ran skill install although skill --help failed:\n{log}"
        );
        assert!(
            r.combined().to_lowercase().contains("skill"),
            "no note about the skipped skill step\n{}",
            r.dump()
        );
        assert_installed(&e);
    }
}
