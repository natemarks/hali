//! Goal of #33, update round (criteria 17-23): hardening found by review of the release scripts.
//! install.sh refuses what it cannot install and writes nothing; the scripts ignore CDPATH; dist is
//! independent of a setgid TMPDIR; release-publish orders versions numerically, counts only vX.Y.Z
//! tags and validates the merge commit id. Everything runs against throwaway fixtures with a fake
//! `gh`; nothing tags, pushes or contacts GitHub.

mod release_support;

mod hardening {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::process::Command;

    use crate::release_support::{
        Fixture, Run, assert_refused, exec, pr_list_json, read_script, tar_listing, tree_listing,
        write, write_mode,
    };

    const STUB: &str = "#!/bin/sh\nexit 0\n";

    // ---- install.sh (criteria 17 and 18) -------------------------------------------------------

    struct Env {
        tmp: tempfile::TempDir,
        stage: PathBuf,
        home: PathBuf,
    }

    fn setup() -> Env {
        let tmp = tempfile::tempdir().unwrap();
        let stage = tmp.path().join("stage");
        let home = tmp.path().join("home");
        fs::create_dir_all(&home).unwrap();
        write_mode(&stage.join("install.sh"), &read_script("install.sh"), 0o755);
        write_mode(&stage.join("hali"), STUB, 0o755);
        for f in [
            "hali.1",
            "completions/hali.bash",
            "completions/_hali",
            "completions/hali.fish",
        ] {
            write_mode(&stage.join(f), "stub\n", 0o600);
        }
        Env { tmp, stage, home }
    }

    fn run_install(e: &Env, home: &str) -> Run {
        let mut c = Command::new("/bin/sh");
        c.args([
            "-c",
            "umask 077; exec \"$0\"",
            e.stage.join("install.sh").to_str().unwrap(),
        ])
        .env_clear()
        .current_dir(e.tmp.path())
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home);
        exec(&mut c, "scripts/install.sh")
    }

    fn assert_clean_refusal(r: &Run) {
        assert!(!r.ok(), "expected a refusal\n{}", r.dump());
        assert!(
            r.code != Some(127),
            "exit 127 is a missing command\n{}",
            r.dump()
        );
        assert!(
            r.stderr.lines().any(|l| l.starts_with("install.sh:")),
            "no message prefixed `install.sh:` on stderr\n{}",
            r.dump()
        );
    }

    #[test]
    #[ignore = "needs bash, git, tar, shellcheck, jq, cargo-edit (make test-external)"]
    fn install_refuses_a_file_where_a_directory_is_needed() {
        let e = setup();
        write(&e.home.join(".local/share/man/man1"), "not a directory\n");
        let before = tree_listing(&e.home, &[]);
        let r = run_install(&e, e.home.to_str().unwrap());
        assert_clean_refusal(&r);
        assert_eq!(
            before,
            tree_listing(&e.home, &[]),
            "a refused install must write nothing (no hali, no temp files)\n{}",
            r.dump()
        );
        assert!(!e.home.join(".local/bin/hali").exists());
    }

    #[test]
    #[ignore = "needs bash, git, tar, shellcheck, jq, cargo-edit (make test-external)"]
    fn install_refuses_a_home_with_a_newline() {
        let e = setup();
        let t = e.tmp.path().display();
        let evil = format!("{t}/victim\n0644 hali.1 {t}/x");
        let before = tree_listing(e.tmp.path(), &[]);
        let r = run_install(&e, &evil);
        assert_clean_refusal(&r);
        assert_eq!(
            before,
            tree_listing(e.tmp.path(), &[]),
            "a refused install must write nothing anywhere\n{}",
            r.dump()
        );
        assert!(!e.tmp.path().join("x").exists());
        assert!(!e.tmp.path().join("victim").exists());
    }

    // ---- CDPATH (criterion 20) -----------------------------------------------------------------

    /// Copies the real scripts into the fixture's `scripts/` (ignored by git through
    /// `.git/info/exclude`, so the tree stays clean), as the Makefile sees them, and makes a decoy
    /// CDPATH directory whose `scripts/release-lib.sh` prints DECOY and exits 77.
    fn with_scripts(fx: &Fixture) -> PathBuf {
        let mut exclude = String::new();
        for name in [
            "dist.sh",
            "release-prepare.sh",
            "release-publish.sh",
            "release-lib.sh",
        ] {
            write_mode(
                &fx.work.join("scripts").join(name),
                &read_script(name),
                0o755,
            );
            exclude.push_str(&format!("/scripts/{name}\n"));
        }
        let info = fx.work.join(".git/info/exclude");
        let old = fs::read_to_string(&info).unwrap_or_default();
        fs::write(&info, format!("{old}{exclude}")).unwrap();
        let decoy = fx.tmp.path().join("decoy");
        write_mode(
            &decoy.join("scripts/release-lib.sh"),
            "echo DECOY\nexit 77\n",
            0o755,
        );
        decoy
    }

    /// `scripts/<name>` by a relative path with the fixture as cwd, as `make` runs it. A relative
    /// path is the form that CDPATH affects (`cd scripts`); the fixture seam cannot run the real
    /// Makefile against the fixture, so the fixture holds copies of the scripts.
    fn run_relative(fx: &Fixture, name: &str, vars: &[(&str, &str)]) -> Run {
        let mut c = fx.command("sh");
        c.args(["-c", &format!("exec scripts/{name}")]);
        for (k, v) in vars {
            c.env(k, v);
        }
        exec(&mut c, &format!("scripts/{name}"))
    }

    fn assert_decoy_unused(r: &Run, what: &str) {
        assert!(r.ok(), "{what} fails under CDPATH\n{}", r.dump());
        assert!(
            r.code != Some(77) && !r.combined().contains("DECOY"),
            "{what} sourced the decoy through CDPATH\n{}",
            r.dump()
        );
        let out = r.combined();
        assert!(
            !out.contains("command not found") && !out.contains("No such file"),
            "{what} could not find its shared library under CDPATH\n{}",
            r.dump()
        );
        assert!(
            out.lines().any(|l| l.starts_with("dry-run:")),
            "{what} did not reach its dry-run output under CDPATH\n{}",
            r.dump()
        );
    }

    #[test]
    #[ignore = "needs bash, git, tar, shellcheck, jq, cargo-edit (make test-external)"]
    fn scripts_ignore_cdpath() {
        let fx = Fixture::new("0.2.0");
        let decoy = with_scripts(&fx);
        let cdpath = decoy.to_str().unwrap();
        let r = run_relative(&fx, "dist.sh", &[("CDPATH", cdpath), ("DRY_RUN", "1")]);
        assert_decoy_unused(&r, "dist.sh");

        let fx = Fixture::new("0.1.0");
        let decoy = with_scripts(&fx);
        let cdpath = decoy.to_str().unwrap();
        let r = run_relative(
            &fx,
            "release-prepare.sh",
            &[("CDPATH", cdpath), ("BUMP", "minor"), ("DRY_RUN", "1")],
        );
        assert_decoy_unused(&r, "release-prepare.sh");

        let fx = Fixture::publish_ready("0.2.0", Some("0.1.0"), true);
        let decoy = with_scripts(&fx);
        let cdpath = decoy.to_str().unwrap();
        let r = run_relative(
            &fx,
            "release-publish.sh",
            &[("CDPATH", cdpath), ("DRY_RUN", "1")],
        );
        assert_decoy_unused(&r, "release-publish.sh");
    }

    // ---- setgid TMPDIR (criterion 21) ----------------------------------------------------------

    #[test]
    #[ignore = "needs bash, git, tar, shellcheck, jq, cargo-edit (make test-external)"]
    fn dist_ignores_a_setgid_tmpdir() {
        let fx = Fixture::new("0.2.0");
        let sg = fx.tmp.path().join("sgid-tmp");
        fs::create_dir_all(&sg).unwrap();
        fs::set_permissions(&sg, fs::Permissions::from_mode(0o2775)).unwrap();
        let probe = sg.join("probe");
        fs::create_dir(&probe).unwrap();
        let inherits = fs::metadata(&sg).unwrap().permissions().mode() & 0o2000 != 0
            && fs::metadata(&probe).unwrap().permissions().mode() & 0o2000 != 0;
        if !inherits {
            eprintln!("SKIPPED: this filesystem does not keep or inherit the setgid bit");
            return;
        }
        let tarball = fx.work.join("dist/hali-v0.2.0-x86_64-linux.tar.gz");

        let r = fx.run("dist.sh", &[("TMPDIR", sg.to_str().unwrap())]);
        assert!(r.ok(), "{}", r.dump());
        let with_setgid = fs::read(&tarball).unwrap();
        for e in tar_listing(&tarball).iter().filter(|e| e.kind == 'd') {
            assert_eq!(e.mode, "rwxr-xr-x", "directory {} mode", e.name);
        }

        fs::remove_dir_all(fx.work.join("dist")).unwrap();
        let r = fx.run("dist.sh", &[]);
        assert!(r.ok(), "{}", r.dump());
        assert_eq!(
            with_setgid,
            fs::read(&tarball).unwrap(),
            "the tarball depends on the TMPDIR's setgid bit"
        );
    }

    // ---- publish version ordering and tags (criterion 22) --------------------------------------

    #[test]
    #[ignore = "needs bash, git, tar, shellcheck, jq, cargo-edit (make test-external)"]
    fn publish_orders_versions_numerically() {
        // 0.9.1 is below v0.10.0 although "0.9.1" sorts after "0.10.0" as text.
        let fx = Fixture::publish_ready("0.9.1", Some("0.10.0"), true);
        let before = fx.snapshot(true);
        let r = fx.run("release-publish.sh", &[("DRY_RUN", "1")]);
        assert_refused(
            &fx,
            &before,
            &r,
            "0.9.1 over v0.10.0",
            &["not above", "latest tag"],
        );

        // 0.10.1 is above v0.9.0 although it sorts before it as text.
        let fx = Fixture::publish_ready("0.10.1", Some("0.9.0"), true);
        let r = fx.run("release-publish.sh", &[("DRY_RUN", "1")]);
        assert!(r.ok(), "0.10.1 over v0.9.0 was refused\n{}", r.dump());
    }

    /// Only `vX.Y.Z` tags count towards "above every other tag": a pre-release tag or a word is
    /// ignored (spec §13.3 is aligned to say so).
    #[test]
    #[ignore = "needs bash, git, tar, shellcheck, jq, cargo-edit (make test-external)"]
    fn publish_counts_only_vx_y_z_tags() {
        let fx = Fixture::publish_ready("0.3.0", Some("0.2.0"), true);
        for t in ["v0.3.0-rc1", "vfoo", "v0.4.0-beta"] {
            fx.git(&["tag", t]);
            fx.git(&["push", "-q", "origin", &format!("refs/tags/{t}")]);
        }
        let r = fx.run("release-publish.sh", &[("DRY_RUN", "1")]);
        assert!(
            r.ok(),
            "non-vX.Y.Z tags must not block a release\n{}",
            r.dump()
        );
        assert!(!r.stderr.contains("not above"), "{}", r.dump());
    }

    // ---- merge commit id (criterion 23) --------------------------------------------------------

    #[test]
    #[ignore = "needs bash, git, tar, shellcheck, jq, cargo-edit (make test-external)"]
    fn publish_refuses_a_malformed_merge_commit_id() {
        let upper = "A".repeat(40);
        for bad in ["--help", "abc123", upper.as_str(), "zzzzzzzz"] {
            let mut fx = Fixture::publish_ready("0.2.0", Some("0.1.0"), true);
            fx.set_var("FAKE_GH_PR_LIST", &pr_list_json("0.2.0", bad));
            let before = fx.snapshot(true);
            let r = fx.run("release-publish.sh", &[]);
            assert_refused(
                &fx,
                &before,
                &r,
                &format!("merge commit id {bad:?}"),
                &["hex", "40", "malformed", "not a commit id", "invalid"],
            );
            assert!(
                !r.stderr.to_lowercase().contains("ancestor"),
                "{bad:?}: the id must be rejected as malformed before any git call, with a \
                 message distinct from the ancestor refusal\n{}",
                r.dump()
            );
        }
    }
}
