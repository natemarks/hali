//! Goal of #33 (criteria 3, 9, 10, 11, 15): `scripts/dist.sh` against a throwaway fixture crate
//! named `hali`, then the spec §13.5 fresh-machine sequence with a local copy instead of
//! `gh release download`.

#[cfg(feature = "goal")]
mod release_support;

#[cfg(feature = "goal")]
mod dist {
    use std::collections::BTreeSet;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::process::Command;

    use crate::release_support::{
        ADR_PATH, FIXTURE_MAN, Fixture, GIT_DATE_TAR, Opts, assert_distinct_messages,
        assert_refused, exec, extract, sha256sum_check, tar_listing,
    };

    const VERSION: &str = "0.2.0";

    fn tarball(fx: &Fixture) -> PathBuf {
        fx.work
            .join(format!("dist/hali-v{VERSION}-x86_64-linux.tar.gz"))
    }

    fn built() -> Fixture {
        let fx = Fixture::new(VERSION);
        let r = fx.run("dist.sh", &[]);
        assert!(r.ok(), "dist.sh failed\n{}", r.dump());
        fx
    }

    #[test]
    #[ignore = "needs bash, git, tar, shellcheck, jq, cargo-edit (make test-external)"]
    fn layout_and_modes() {
        let fx = built();
        let tb = tarball(&fx);
        assert!(tb.is_file(), "missing {}", tb.display());
        let dist_files: BTreeSet<String> = fs::read_dir(fx.work.join("dist"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            dist_files,
            BTreeSet::from([
                format!("hali-v{VERSION}-x86_64-linux.tar.gz"),
                "SHA256SUMS".to_string()
            ]),
            "dist/ holds exactly the tarball and SHA256SUMS"
        );

        let top = format!("hali-v{VERSION}");
        let entries = tar_listing(&tb);
        let mut files = BTreeSet::new();
        let mut dirs = BTreeSet::new();
        for e in &entries {
            assert!(
                matches!(e.kind, '-' | 'd'),
                "only regular files and directories: {e:?}"
            );
            assert_eq!(e.owner, "0/0", "owner 0 for {e:?}");
            assert_eq!(e.time, GIT_DATE_TAR, "mtime is the commit time for {e:?}");
            let name = e.name.trim_end_matches('/');
            assert!(
                name == top || name.starts_with(&format!("{top}/")),
                "outside the one top directory: {e:?}"
            );
            if e.kind == 'd' {
                assert_eq!(e.mode, "rwxr-xr-x", "directory mode: {e:?}");
                dirs.insert(name.to_string());
            } else {
                let want = if name.ends_with("/hali") || name.ends_with("/install.sh") {
                    "rwxr-xr-x"
                } else {
                    "rw-r--r--"
                };
                assert_eq!(e.mode, want, "file mode: {e:?}");
                files.insert(name.to_string());
            }
        }
        let want_files: BTreeSet<String> = [
            "hali",
            "install.sh",
            "hali.1",
            "completions/hali.bash",
            "completions/_hali",
            "completions/hali.fish",
            "README.md",
            "LICENSE-MIT",
            "LICENSE-APACHE",
        ]
        .iter()
        .map(|f| format!("{top}/{f}"))
        .collect();
        assert_eq!(files, want_files, "files in the tarball");
        assert!(dirs.contains(&top), "the top directory has its own entry");
        assert!(dirs.contains(&format!("{top}/completions")));
        assert_eq!(dirs.len(), 2, "no other directories: {dirs:?}");
        assert_eq!(
            entries
                .iter()
                .filter(|e| e.name.trim_end_matches('/') == top)
                .count(),
            1
        );
        // `gzip -n`: no stored name, zero mtime.
        let head = fs::read(&tb).unwrap();
        assert_eq!(&head[..2], &[0x1f, 0x8b]);
        assert_eq!(head[3], 0, "gzip flags: no stored file name");
        assert_eq!(&head[4..8], &[0, 0, 0, 0], "gzip mtime is zero");

        // Contents.
        let out = fx.tmp.path().join("extracted");
        extract(&tb, &out);
        let root = out.join(&top);
        assert_eq!(
            fs::read_to_string(root.join("README.md")).unwrap(),
            "fixture readme\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("LICENSE-MIT")).unwrap(),
            "fixture MIT license\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("LICENSE-APACHE")).unwrap(),
            "fixture Apache license\n"
        );
        assert_eq!(
            fs::read(root.join("install.sh")).unwrap(),
            fs::read(fx.work.join("scripts/install.sh")).unwrap(),
            "install.sh is the repository's"
        );
        let hali = root.join("hali");
        let version = exec(Command::new(&hali).env_clear().arg("--version"), "hali");
        assert_eq!(version.stdout, format!("hali {VERSION}\n"));
        assert_eq!(
            fs::read_to_string(root.join("hali.1")).unwrap(),
            FIXTURE_MAN
        );
        for (file, shell) in [
            ("hali.bash", "bash"),
            ("_hali", "zsh"),
            ("hali.fish", "fish"),
        ] {
            let gen_out = exec(
                Command::new(&hali)
                    .env_clear()
                    .args(["generate", "completions", shell]),
                "hali generate",
            );
            assert_eq!(
                fs::read_to_string(root.join("completions").join(file)).unwrap(),
                gen_out.stdout,
                "completions/{file} matches `env -i hali generate completions {shell}`"
            );
        }
        // The binary has no test hooks.
        let bytes = fs::read(&hali).unwrap();
        assert!(
            !bytes.windows(10).any(|w| w == b"HALI_TEST_"),
            "binary contains HALI_TEST_"
        );
        assert_eq!(
            fs::metadata(&hali).unwrap().permissions().mode() & 0o111,
            0o111
        );
    }

    #[test]
    #[ignore = "needs bash, git, tar, shellcheck, jq, cargo-edit (make test-external)"]
    fn sha256sums_verify() {
        let fx = built();
        let sums = fs::read_to_string(fx.work.join("dist/SHA256SUMS")).unwrap();
        let lines: Vec<&str> = sums.lines().collect();
        assert_eq!(lines.len(), 1, "SHA256SUMS lists the tarball only:\n{sums}");
        let (hash, name) = lines[0]
            .split_once("  ")
            .expect("sha256sum format: hash, two spaces, name");
        assert_eq!(hash.len(), 64);
        assert_eq!(
            name,
            format!("hali-v{VERSION}-x86_64-linux.tar.gz"),
            "bare file name, no path"
        );
        let r = sha256sum_check(&fx.work.join("dist"));
        assert!(r.ok(), "{}", r.dump());
    }

    #[test]
    #[ignore = "needs bash, git, tar, shellcheck, jq, cargo-edit (make test-external)"]
    fn reproducible() {
        let fx = built();
        let first = fs::read(tarball(&fx)).unwrap();
        let first_sums = fs::read(fx.work.join("dist/SHA256SUMS")).unwrap();
        fs::remove_dir_all(fx.work.join("dist")).unwrap();
        let r = fx.run("dist.sh", &[]);
        assert!(r.ok(), "{}", r.dump());
        assert_eq!(
            first,
            fs::read(tarball(&fx)).unwrap(),
            "two builds are byte-identical"
        );
        assert_eq!(
            first_sums,
            fs::read(fx.work.join("dist/SHA256SUMS")).unwrap()
        );
        // A rerun over an existing dist/ works too and gives the same bytes.
        let r = fx.run("dist.sh", &[]);
        assert!(r.ok(), "{}", r.dump());
        assert_eq!(first, fs::read(tarball(&fx)).unwrap());
    }

    #[test]
    #[ignore = "needs bash, git, tar, shellcheck, jq, cargo-edit (make test-external)"]
    fn refuses_missing_license() {
        let mut messages = Vec::new();
        for file in ["LICENSE-MIT", "LICENSE-APACHE"] {
            let fx = Fixture::new(VERSION);
            fx.git(&["rm", "-q", file]);
            fx.commit_all(&format!("remove {file}"));
            let before = fx.snapshot(true);
            let r = fx.run("dist.sh", &[]);
            let msg = assert_refused(&fx, &before, &r, file, &[file]);
            assert!(!fx.work.join("dist").exists(), "{file}: dist/ was created");
            messages.push((file, msg));
        }
        assert_distinct_messages(&messages);
    }

    #[test]
    #[ignore = "needs bash, git, tar, shellcheck, jq, cargo-edit (make test-external)"]
    fn refuses_dirty_tree() {
        let fx = Fixture::new(VERSION);
        fx.edit("README.md", "edited, not committed\n");
        let before = fx.snapshot(true);
        let r = fx.run("dist.sh", &[]);
        assert_refused(
            &fx,
            &before,
            &r,
            "dirty",
            &["clean", "dirty", "uncommitted", "changes", "modified"],
        );
        assert!(!fx.work.join("dist").exists());

        // Untracked files are not tracked changes.
        let fx = Fixture::new(VERSION);
        fx.edit("scratch.txt", "untracked\n");
        let r = fx.run("dist.sh", &[]);
        assert!(
            r.ok(),
            "an untracked file must not block dist\n{}",
            r.dump()
        );
        assert!(tarball(&fx).is_file());
    }

    #[test]
    #[ignore = "needs bash, git, tar, shellcheck, jq, cargo-edit (make test-external)"]
    fn refuses_test_hooks_binary() {
        let fx = Fixture::with(Opts {
            version: VERSION,
            bench_heading: true,
            default_hooks: true,
        });
        let before = fx.snapshot(true);
        let r = fx.run("dist.sh", &[]);
        assert_refused(&fx, &before, &r, "test hooks", &["HALI_TEST_"]);
        assert!(
            !fx.work.join("dist").exists(),
            "a refused build leaves no dist/"
        );
    }

    #[test]
    #[ignore = "needs bash, git, tar, shellcheck, jq, cargo-edit (make test-external)"]
    fn dist_ignores_rustflags_and_wrapper() {
        // `--cfg hali_hook` would put HALI_TEST_ into the fixture binary, and the wrapper does not
        // exist: dist must unset all three before it builds.
        let fx = Fixture::new(VERSION);
        let r = fx.run(
            "dist.sh",
            &[
                ("RUSTFLAGS", "--cfg hali_hook"),
                ("CARGO_ENCODED_RUSTFLAGS", "--cfg\u{1f}hali_hook"),
                ("RUSTC_WRAPPER", "/nonexistent/wrapper"),
            ],
        );
        assert!(
            r.ok(),
            "dist must ignore RUSTFLAGS, CARGO_ENCODED_RUSTFLAGS and RUSTC_WRAPPER\n{}",
            r.dump()
        );
        assert!(tarball(&fx).is_file());
    }

    #[test]
    #[ignore = "needs bash, git, tar, shellcheck, jq, cargo-edit (make test-external)"]
    fn refuses_v0_1_0_without_bench_results() {
        let fx = Fixture::with(Opts {
            version: "0.1.0",
            bench_heading: false,
            default_hooks: false,
        });
        let before = fx.snapshot(true);
        let r = fx.run("dist.sh", &[]);
        assert_refused(
            &fx,
            &before,
            &r,
            "no bench heading",
            &["Benchmark results", "benchmark"],
        );
        assert!(!fx.work.join("dist").exists());
        fx.edit(ADR_PATH, "# ADR 1\n\n## Benchmark results\n\nnumbers\n");
        fx.commit_all("adr: record benchmark results");
        let r = fx.run("dist.sh", &[]);
        assert!(r.ok(), "{}", r.dump());
        assert!(
            fx.work
                .join("dist/hali-v0.1.0-x86_64-linux.tar.gz")
                .is_file()
        );
    }

    #[test]
    #[ignore = "needs bash, git, tar, shellcheck, jq, cargo-edit (make test-external)"]
    fn dist_dry_run_writes_nothing() {
        let fx = Fixture::new(VERSION);
        let before = fx.snapshot(true);
        let r = fx.run("dist.sh", &[("DRY_RUN", "1")]);
        assert!(r.ok(), "{}", r.dump());
        assert!(
            r.combined().contains("dry-run:"),
            "no `dry-run:` line\n{}",
            r.dump()
        );
        assert!(!fx.work.join("dist").exists(), "dry run created dist/");
        assert_eq!(before, fx.snapshot(true), "dry run changed the fixture");
        assert!(fx.gh_mutations().is_empty());

        // A dry run exits 0 only when a real run would pass its preflights.
        let fx = Fixture::new(VERSION);
        fx.git(&["rm", "-q", "LICENSE-MIT"]);
        fx.commit_all("remove LICENSE-MIT");
        let before = fx.snapshot(true);
        let r = fx.run("dist.sh", &[("DRY_RUN", "1")]);
        assert_refused(
            &fx,
            &before,
            &r,
            "dry run, missing license",
            &["LICENSE-MIT"],
        );
    }

    #[test]
    #[ignore = "needs bash, git, tar, shellcheck, jq, cargo-edit (make test-external)"]
    fn dist_rejects_bad_dry_run() {
        let fx = Fixture::new(VERSION);
        let before = fx.snapshot(true);
        let mut messages = Vec::new();
        for bad in ["true", "yes", "2", "01", " 1", "1 ", "on", "-1"] {
            let r = fx.run("dist.sh", &[("DRY_RUN", bad)]);
            let msg = assert_refused(&fx, &before, &r, &format!("DRY_RUN={bad:?}"), &["DRY_RUN"]);
            messages.push(("DRY_RUN", msg));
        }
        assert!(!fx.work.join("dist").exists());
        // The valid spellings are accepted (0 and empty build for real).
        for ok in ["", "0"] {
            let r = fx.run("dist.sh", &[("DRY_RUN", ok)]);
            assert!(r.ok(), "DRY_RUN={ok:?} must be accepted\n{}", r.dump());
        }
    }

    #[test]
    #[ignore = "needs bash, git, tar, shellcheck, jq, cargo-edit (make test-external)"]
    fn fresh_machine_install() {
        // Spec §13.5 with a local copy in place of `gh release download`:
        //   sha256sum -c --ignore-missing SHA256SUMS && tar xzf hali-*.tar.gz && ./hali-*/install.sh
        let fx = built();
        let dl = fx.tmp.path().join("download");
        let home = fx.tmp.path().join("fresh-home");
        fs::create_dir_all(&dl).unwrap();
        fs::create_dir_all(&home).unwrap();
        fs::copy(
            tarball(&fx),
            dl.join(format!("hali-v{VERSION}-x86_64-linux.tar.gz")),
        )
        .unwrap();
        fs::copy(fx.work.join("dist/SHA256SUMS"), dl.join("SHA256SUMS")).unwrap();
        let r = exec(
            Command::new("sh")
                .args([
                    "-c",
                    "sha256sum -c --ignore-missing SHA256SUMS && tar xzf hali-*.tar.gz && ./hali-*/install.sh",
                ])
                .current_dir(&dl)
                .env_clear()
                .env("HOME", &home)
                .env("PATH", "/usr/bin:/bin"),
            "the §13.5 command",
        );
        assert!(r.ok(), "{}", r.dump());
        let bin = home.join(".local/bin/hali");
        let v = exec(
            Command::new(&bin).env_clear().arg("--version"),
            "installed hali",
        );
        assert_eq!(v.stdout, format!("hali {VERSION}\n"));
        assert_eq!(
            fs::read_to_string(home.join(".local/share/man/man1/hali.1")).unwrap(),
            FIXTURE_MAN
        );
        for p in [
            ".local/share/bash-completion/completions/hali",
            ".local/share/zsh/site-functions/_hali",
            ".config/fish/completions/hali.fish",
        ] {
            assert!(home.join(p).is_file(), "missing {p}");
        }
        let t = r.combined().to_lowercase();
        assert!(
            t.contains("log out") || t.contains("log in") || t.contains("login"),
            "no re-login warning\n{}",
            r.dump()
        );
    }
}
