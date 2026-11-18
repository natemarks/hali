//! Goal of #33 (criteria 2-9): `scripts/release-prepare.sh` and `scripts/release-publish.sh` in a
//! throwaway fixture repo with a local bare `origin` and a fake `gh`. Nothing here contacts GitHub,
//! tags or pushes in this repository, or runs the real release targets.

#[cfg(feature = "goal")]
mod release_support;

#[cfg(feature = "goal")]
mod flow {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use crate::release_support::{
        ADR_PATH, Fixture, Opts, assert_distinct_messages, assert_refused, exec, pr_list_json,
        write_mode,
    };

    const BENCH_FOR_ADR: &str = "# ADR 1\n\n## Benchmark results\n\nnumbers\n";
    const VERSION_KEYWORDS: &[&str] = &["range", "1.0.0", "0.1.0"];
    const DIRTY_KEYWORDS: &[&str] = &["clean", "dirty", "uncommitted", "changes", "modified"];
    const ORIGIN_KEYWORDS: &[&str] = &[
        "origin",
        "behind",
        "up to date",
        "up-to-date",
        "ahead",
        "fetch",
    ];
    const AUTH_KEYWORDS: &[&str] = &["auth", "logged", "gh auth"];
    const BENCH_KEYWORDS: &[&str] = &["Benchmark results", "benchmark"];

    // ---- criterion 2: bad BUMP or DRY_RUN ------------------------------------------------------

    #[test]
    #[ignore = "needs bash, git, tar, shellcheck, jq, cargo-edit (make test-external)"]
    fn rejects_bad_bump_and_dry_run() {
        let fx = Fixture::new("0.1.0");
        let before = fx.snapshot(true);
        let mut messages = Vec::new();
        for bump in [
            "",
            "huge",
            "Patch",
            "patch minor",
            "--bump",
            "pre",
            "prepatch",
            "1.2.3",
        ] {
            let r = fx.run("release-prepare.sh", &[("BUMP", bump)]);
            let msg = assert_refused(&fx, &before, &r, &format!("BUMP={bump:?}"), &["BUMP"]);
            messages.push(("BUMP", msg));
        }
        let bad_dry_runs = ["true", "yes", "2", "01", " 1", "1 ", "on", "-1"];
        for bad in bad_dry_runs {
            let r = fx.run("release-prepare.sh", &[("BUMP", "patch"), ("DRY_RUN", bad)]);
            let msg = assert_refused(
                &fx,
                &before,
                &r,
                &format!("prepare DRY_RUN={bad:?}"),
                &["DRY_RUN"],
            );
            messages.push(("DRY_RUN", msg));
        }
        assert_distinct_messages(&messages);

        let fx = Fixture::publish_ready("0.2.0", Some("0.1.0"), true);
        let before = fx.snapshot(true);
        for bad in bad_dry_runs {
            let r = fx.run("release-publish.sh", &[("DRY_RUN", bad)]);
            assert_refused(
                &fx,
                &before,
                &r,
                &format!("publish DRY_RUN={bad:?}"),
                &["DRY_RUN"],
            );
        }
        assert!(!fx.work.join("dist").exists());
    }

    // ---- criterion 3: dry runs -----------------------------------------------------------------

    #[test]
    #[ignore = "needs bash, git, tar, shellcheck, jq, cargo-edit (make test-external)"]
    fn prepare_dry_run_changes_nothing() {
        let fx = Fixture::new("0.1.0");
        let before = fx.snapshot(true);
        let r = fx.run("release-prepare.sh", &[("BUMP", "minor"), ("DRY_RUN", "1")]);
        assert!(r.ok(), "{}", r.dump());
        let out = r.combined();
        assert!(
            out.lines().any(|l| l.trim_start().starts_with("dry-run:")),
            "no `dry-run:` line\n{}",
            r.dump()
        );
        assert!(
            out.contains("pr create"),
            "the PR creation is not shown as a dry-run command\n{}",
            r.dump()
        );
        assert!(
            out.contains("git push"),
            "the push is not shown as a dry-run command\n{}",
            r.dump()
        );
        assert_eq!(
            before,
            fx.snapshot(true),
            "a dry run changed the fixture\n{}",
            r.dump()
        );
        assert!(
            fx.gh_called("auth status"),
            "preflights did not run: {:?}",
            fx.gh_calls()
        );
        assert!(fx.gh_mutations().is_empty(), "{:?}", fx.gh_mutations());

        // Exit 0 only when a real run would pass its preflights.
        let fx = Fixture::new("0.1.0");
        fx.git(&["checkout", "-q", "-b", "feature"]);
        let before = fx.snapshot(true);
        let r = fx.run("release-prepare.sh", &[("BUMP", "minor"), ("DRY_RUN", "1")]);
        assert_refused(&fx, &before, &r, "dry run off main", &["main"]);
    }

    #[test]
    #[ignore = "needs bash, git, tar, shellcheck, jq, cargo-edit (make test-external)"]
    fn publish_dry_run_changes_nothing() {
        let fx = Fixture::publish_ready("0.2.0", Some("0.1.0"), true);
        let before = fx.snapshot(true);
        let r = fx.run("release-publish.sh", &[("DRY_RUN", "1")]);
        assert!(r.ok(), "{}", r.dump());
        let out = r.combined();
        assert!(
            out.lines().any(|l| l.trim_start().starts_with("dry-run:")),
            "no `dry-run:` line\n{}",
            r.dump()
        );
        for shown in ["git tag", "git push", "release create"] {
            assert!(
                out.contains(shown),
                "`{shown}` is not shown as a dry-run command\n{}",
                r.dump()
            );
        }
        assert_eq!(
            before,
            fx.snapshot(true),
            "a dry run changed the fixture\n{}",
            r.dump()
        );
        assert!(!fx.work.join("dist").exists(), "a dry run built dist/");
        assert!(fx.local_tag("v0.2.0").is_none() && fx.origin_tag("v0.2.0").is_none());
        for verb in ["auth status", "pr list", "release view"] {
            assert!(
                fx.gh_called(verb),
                "preflight `gh {verb}` did not run: {:?}",
                fx.gh_calls()
            );
        }
        assert!(fx.gh_mutations().is_empty(), "{:?}", fx.gh_mutations());

        let mut fx = Fixture::publish_ready("0.2.0", Some("0.1.0"), true);
        fx.set_var("FAKE_GH_RELEASE_VIEW_EXIT", "0");
        let before = fx.snapshot(true);
        let r = fx.run("release-publish.sh", &[("DRY_RUN", "1")]);
        assert_refused(
            &fx,
            &before,
            &r,
            "dry run with a release",
            &["release", "exists", "already"],
        );
    }

    // ---- criteria 4 and 9 (prepare): preflights ------------------------------------------------

    struct PrepCase {
        name: &'static str,
        group: &'static str,
        version: &'static str,
        bump: &'static str,
        bench: bool,
        keywords: &'static [&'static str],
        setup: fn(&mut Fixture),
        vars: &'static [(&'static str, &'static str)],
    }

    fn tag_v0_1_1(fx: &Fixture) {
        fx.git(&["tag", "-a", "v0.1.1", "-m", "hali v0.1.1"]);
    }

    #[test]
    #[ignore = "needs bash, git, tar, shellcheck, jq, cargo-edit (make test-external)"]
    fn prepare_preflights_refuse() {
        let cases = [
            PrepCase {
                name: "not on main",
                group: "main",
                version: "0.1.0",
                bump: "patch",
                bench: true,
                keywords: &["main"],
                vars: &[],
                setup: |fx| {
                    fx.git(&["checkout", "-q", "-b", "feature"]);
                },
            },
            PrepCase {
                name: "dirty tree",
                group: "clean",
                version: "0.1.0",
                bump: "patch",
                bench: true,
                keywords: DIRTY_KEYWORDS,
                vars: &[],
                setup: |fx| fx.edit("README.md", "edited, not committed\n"),
            },
            PrepCase {
                name: "behind origin",
                group: "origin",
                version: "0.1.0",
                bump: "patch",
                bench: true,
                keywords: ORIGIN_KEYWORDS,
                vars: &[],
                setup: |fx| fx.push_from_other_clone(),
            },
            PrepCase {
                name: "ahead of origin",
                group: "origin",
                version: "0.1.0",
                bump: "patch",
                bench: true,
                keywords: ORIGIN_KEYWORDS,
                vars: &[],
                setup: |fx| {
                    fx.edit("README.md", "unpushed\n");
                    fx.commit_all("unpushed");
                },
            },
            PrepCase {
                name: "gh not authenticated",
                group: "auth",
                version: "0.1.0",
                bump: "patch",
                bench: true,
                keywords: AUTH_KEYWORDS,
                vars: &[("FAKE_GH_AUTH_EXIT", "1")],
                setup: |_| {},
            },
            PrepCase {
                name: "cargo-edit is another version",
                group: "cargo-edit",
                version: "0.1.0",
                bump: "patch",
                bench: true,
                keywords: &["cargo-edit", "set-version"],
                vars: &[],
                setup: |fx| fx.shim_cargo_edit("cargo-edit-set-version 0.13.13", 0),
            },
            PrepCase {
                name: "cargo-edit is missing",
                group: "cargo-edit",
                version: "0.1.0",
                bump: "patch",
                bench: true,
                keywords: &["cargo-edit", "set-version"],
                vars: &[],
                setup: |fx| fx.shim_cargo_edit("", 101),
            },
            PrepCase {
                name: "new version is 1.0.0",
                group: "range",
                version: "0.9.9",
                bump: "major",
                bench: true,
                keywords: VERSION_KEYWORDS,
                vars: &[],
                setup: |_| {},
            },
            PrepCase {
                name: "new version is below 0.1.0",
                group: "range",
                version: "0.0.5",
                bump: "patch",
                bench: true,
                keywords: VERSION_KEYWORDS,
                vars: &[],
                setup: |_| {},
            },
            PrepCase {
                name: "tag exists locally",
                group: "tag",
                version: "0.1.0",
                bump: "patch",
                bench: true,
                keywords: &["tag", "v0.1.1"],
                vars: &[],
                setup: |fx| tag_v0_1_1(fx),
            },
            PrepCase {
                name: "tag exists on origin only",
                group: "tag",
                version: "0.1.0",
                bump: "patch",
                bench: true,
                keywords: &["tag", "v0.1.1"],
                vars: &[],
                setup: |fx| {
                    tag_v0_1_1(fx);
                    fx.git(&["push", "-q", "origin", "refs/tags/v0.1.1"]);
                    fx.git(&["tag", "-d", "v0.1.1"]);
                },
            },
            PrepCase {
                name: "branch exists locally",
                group: "branch",
                version: "0.1.0",
                bump: "patch",
                bench: true,
                keywords: &["branch", "release/v0.1.1"],
                vars: &[],
                setup: |fx| {
                    fx.git(&["branch", "release/v0.1.1"]);
                },
            },
            PrepCase {
                name: "branch exists on origin only",
                group: "branch",
                version: "0.1.0",
                bump: "patch",
                bench: true,
                keywords: &["branch", "release/v0.1.1"],
                vars: &[],
                setup: |fx| {
                    fx.git(&["push", "-q", "origin", "HEAD:refs/heads/release/v0.1.1"]);
                },
            },
            PrepCase {
                name: "LICENSE-MIT missing",
                group: "license-mit",
                version: "0.1.0",
                bump: "patch",
                bench: true,
                keywords: &["LICENSE-MIT"],
                vars: &[],
                setup: |fx| {
                    fx.git(&["rm", "-q", "LICENSE-MIT"]);
                    fx.commit_push("remove LICENSE-MIT");
                },
            },
            PrepCase {
                name: "LICENSE-APACHE missing",
                group: "license-apache",
                version: "0.1.0",
                bump: "patch",
                bench: true,
                keywords: &["LICENSE-APACHE"],
                vars: &[],
                setup: |fx| {
                    fx.git(&["rm", "-q", "LICENSE-APACHE"]);
                    fx.commit_push("remove LICENSE-APACHE");
                },
            },
            PrepCase {
                name: "LICENSE-APACHE empty",
                group: "license-apache",
                version: "0.1.0",
                bump: "patch",
                bench: true,
                keywords: &["LICENSE-APACHE"],
                vars: &[],
                setup: |fx| {
                    fx.edit("LICENSE-APACHE", "");
                    fx.commit_push("empty LICENSE-APACHE");
                },
            },
            PrepCase {
                name: "0.1.0 without benchmark results",
                group: "adr",
                version: "0.0.9",
                bump: "minor",
                bench: false,
                keywords: BENCH_KEYWORDS,
                vars: &[],
                setup: |_| {},
            },
            PrepCase {
                name: "make static-check fails",
                group: "static",
                version: "0.1.0",
                bump: "patch",
                bench: true,
                keywords: &["static"],
                vars: &[("RELEASE_STATIC", "static-fail")],
                setup: |_| {},
            },
        ];
        let mut messages = Vec::new();
        for c in cases {
            let mut fx = Fixture::with(Opts {
                version: c.version,
                bench_heading: c.bench,
                default_hooks: false,
            });
            (c.setup)(&mut fx);
            let before = fx.snapshot(false);
            let mut vars = vec![("BUMP", c.bump)];
            vars.extend_from_slice(c.vars);
            let r = fx.run("release-prepare.sh", &vars);
            let msg = assert_refused(&fx, &before, &r, c.name, c.keywords);
            messages.push((c.group, msg));
        }
        assert_distinct_messages(&messages);
    }

    // ---- criterion 5: the bump PR --------------------------------------------------------------

    #[test]
    #[ignore = "needs bash, git, tar, shellcheck, jq, cargo-edit (make test-external)"]
    fn prepare_opens_bump_pr() {
        for (bump, new) in [("patch", "0.1.1"), ("minor", "0.2.0")] {
            let fx = Fixture::new("0.1.0");
            let head_before = fx.head();
            let branch = format!("release/v{new}");
            let r = fx.run("release-prepare.sh", &[("BUMP", bump), ("DRY_RUN", "0")]);
            assert!(r.ok(), "BUMP={bump}\n{}", r.dump());

            let refname = format!("refs/heads/{branch}");
            assert!(
                fx.origin_git(&["rev-parse", "-q", "--verify", &refname])
                    .len()
                    >= 40,
                "{branch} is not on origin"
            );
            let range = format!("refs/heads/main..{refname}");
            assert_eq!(
                fx.origin_git(&["rev-list", "--count", &range]),
                "1",
                "one bump commit"
            );
            assert_eq!(
                fx.origin_git(&["log", "-1", "--format=%s", &refname]),
                format!("release: v{new}")
            );
            let mut files: Vec<String> = fx
                .origin_git(&["diff", "--name-only", "refs/heads/main", &refname])
                .lines()
                .map(str::to_string)
                .collect();
            files.sort();
            assert_eq!(
                files,
                ["Cargo.lock", "Cargo.toml"],
                "the commit touches only the manifest and the lock"
            );
            assert!(
                fx.origin_git(&["show", &format!("{refname}:Cargo.toml")])
                    .contains(&format!("version = \"{new}\""))
            );
            assert!(
                fx.origin_git(&["show", &format!("{refname}:Cargo.lock")])
                    .contains(&format!("name = \"hali\"\nversion = \"{new}\""))
            );

            // The pushed branch builds with --locked --offline.
            let check = fx.tmp.path().join("check");
            let origin = fx.origin.to_str().unwrap().to_string();
            let clone = exec(
                fx.command("git").current_dir(fx.tmp.path()).args([
                    "clone",
                    "-q",
                    "--branch",
                    &branch,
                    &origin,
                    check.to_str().unwrap(),
                ]),
                "git clone",
            );
            assert!(clone.ok(), "{}", clone.dump());
            let meta = exec(
                fx.command("cargo").current_dir(&check).args([
                    "metadata",
                    "--locked",
                    "--offline",
                    "--format-version",
                    "1",
                ]),
                "cargo metadata",
            );
            assert!(
                meta.ok(),
                "cargo metadata --locked --offline on the bump branch\n{}",
                meta.dump()
            );

            // The PR.
            let calls = fx.gh_calls();
            let create: Vec<&String> = calls
                .iter()
                .filter(|l| l.starts_with("pr create"))
                .collect();
            assert_eq!(create.len(), 1, "one `gh pr create`: {calls:?}");
            let line = create[0];
            assert!(line.contains("--base main"), "{line}");
            assert!(line.contains(&format!("--head {branch}")), "{line}");
            assert!(
                line.contains(&format!("v{new}")),
                "the PR names the version: {line}"
            );

            // Back on main, nothing else changed, no tags.
            assert_eq!(fx.git(&["rev-parse", "--abbrev-ref", "HEAD"]), "main");
            assert_eq!(fx.head(), head_before);
            assert_eq!(
                fx.git(&["status", "--porcelain=v1", "--untracked-files=all"]),
                ""
            );
            assert_eq!(fx.git(&["tag", "--list"]), "");
            assert_eq!(fx.origin_git(&["tag", "--list"]), "");
            assert_eq!(
                fx.origin_git(&["rev-parse", "refs/heads/main"]),
                head_before
            );
        }
    }

    // ---- criteria 6 and 9 (publish): preflights ------------------------------------------------

    struct PubCase {
        name: &'static str,
        group: &'static str,
        version: &'static str,
        prev: Option<&'static str>,
        bench: bool,
        keywords: &'static [&'static str],
        setup: fn(&mut Fixture),
    }

    fn root_commit(fx: &Fixture) -> String {
        fx.git(&["rev-list", "--max-parents=0", "HEAD"])
    }

    #[test]
    #[ignore = "needs bash, git, tar, shellcheck, jq, cargo-edit (make test-external)"]
    fn publish_preflights_refuse() {
        let v = "0.2.0";
        let p = Some("0.1.0");
        let cases = [
            PubCase {
                name: "not on main",
                group: "main",
                version: v,
                prev: p,
                bench: true,
                keywords: &["main"],
                setup: |fx| {
                    fx.git(&["checkout", "-q", "-b", "feature"]);
                },
            },
            PubCase {
                name: "dirty tree",
                group: "clean",
                version: v,
                prev: p,
                bench: true,
                keywords: DIRTY_KEYWORDS,
                setup: |fx| fx.edit("README.md", "edited, not committed\n"),
            },
            PubCase {
                name: "behind origin",
                group: "origin",
                version: v,
                prev: p,
                bench: true,
                keywords: ORIGIN_KEYWORDS,
                setup: |fx| fx.push_from_other_clone(),
            },
            PubCase {
                name: "ahead of origin",
                group: "origin",
                version: v,
                prev: p,
                bench: true,
                keywords: ORIGIN_KEYWORDS,
                setup: |fx| {
                    fx.edit("README.md", "unpushed\n");
                    fx.commit_all("unpushed");
                },
            },
            PubCase {
                name: "gh not authenticated",
                group: "auth",
                version: v,
                prev: p,
                bench: true,
                keywords: AUTH_KEYWORDS,
                setup: |fx| fx.set_var("FAKE_GH_AUTH_EXIT", "1"),
            },
            PubCase {
                name: "no merged PR",
                group: "no-pr",
                version: v,
                prev: p,
                bench: true,
                keywords: &["merged", "pull request"],
                setup: |fx| fx.set_var("FAKE_GH_PR_LIST", "[]"),
            },
            PubCase {
                name: "only another version's PR is merged",
                group: "no-pr",
                version: v,
                prev: p,
                bench: true,
                keywords: &["merged", "pull request"],
                setup: |fx| {
                    let list = pr_list_json("0.1.0", &fx.merge_sha);
                    fx.set_var("FAKE_GH_PR_LIST", &list);
                },
            },
            PubCase {
                name: "merge commit is not in history",
                group: "ancestor",
                version: v,
                prev: p,
                bench: true,
                keywords: &["ancestor", "merge", "history", "contain"],
                setup: |fx| {
                    let root = root_commit(fx);
                    fx.git(&["checkout", "-q", "-b", "side", &root]);
                    fx.edit("side.txt", "a commit that main does not contain\n");
                    fx.commit_all("side");
                    let side = fx.head();
                    fx.git(&["checkout", "-q", "main"]);
                    let list = pr_list_json("0.2.0", &side);
                    fx.set_var("FAKE_GH_PR_LIST", &list);
                },
            },
            PubCase {
                name: "version not above the latest tag",
                group: "latest",
                version: v,
                prev: p,
                bench: true,
                keywords: &["latest", "above", "greater", "newer", "higher", "tag"],
                setup: |fx| {
                    let root = root_commit(fx);
                    fx.git(&["tag", "-a", "v0.9.0", "-m", "hali v0.9.0", &root]);
                    fx.git(&["push", "-q", "origin", "refs/tags/v0.9.0"]);
                },
            },
            PubCase {
                name: "version is 1.0.0",
                group: "range",
                version: "1.0.0",
                prev: p,
                bench: true,
                keywords: VERSION_KEYWORDS,
                setup: |_| {},
            },
            PubCase {
                name: "version is below 0.1.0",
                group: "range",
                version: "0.0.9",
                prev: None,
                bench: true,
                keywords: VERSION_KEYWORDS,
                setup: |_| {},
            },
            PubCase {
                name: "release exists",
                group: "release",
                version: v,
                prev: p,
                bench: true,
                keywords: &["release", "exists", "already"],
                setup: |fx| fx.set_var("FAKE_GH_RELEASE_VIEW_EXIT", "0"),
            },
            PubCase {
                name: "tag at another commit, local",
                group: "tag-position",
                version: v,
                prev: p,
                bench: true,
                keywords: &["tag", "HEAD"],
                setup: |fx| {
                    let root = root_commit(fx);
                    fx.git(&["tag", "-a", "v0.2.0", "-m", "elsewhere", &root]);
                },
            },
            PubCase {
                name: "tag at another commit, origin only",
                group: "tag-position",
                version: v,
                prev: p,
                bench: true,
                keywords: &["tag", "HEAD"],
                setup: |fx| {
                    let root = root_commit(fx);
                    fx.git(&["tag", "-a", "v0.2.0", "-m", "elsewhere", &root]);
                    fx.git(&["push", "-q", "origin", "refs/tags/v0.2.0"]);
                    fx.git(&["tag", "-d", "v0.2.0"]);
                },
            },
            PubCase {
                name: "tag at HEAD is lightweight",
                group: "annotated",
                version: v,
                prev: p,
                bench: true,
                keywords: &["annotated", "lightweight"],
                setup: |fx| {
                    fx.git(&["tag", "v0.2.0"]);
                },
            },
            PubCase {
                name: "LICENSE-MIT missing",
                group: "license-mit",
                version: v,
                prev: p,
                bench: true,
                keywords: &["LICENSE-MIT"],
                setup: |fx| {
                    fx.git(&["rm", "-q", "LICENSE-MIT"]);
                    fx.commit_push("remove LICENSE-MIT");
                },
            },
            PubCase {
                name: "LICENSE-APACHE missing",
                group: "license-apache",
                version: v,
                prev: p,
                bench: true,
                keywords: &["LICENSE-APACHE"],
                setup: |fx| {
                    fx.git(&["rm", "-q", "LICENSE-APACHE"]);
                    fx.commit_push("remove LICENSE-APACHE");
                },
            },
            PubCase {
                name: "0.1.0 without benchmark results",
                group: "adr",
                version: "0.1.0",
                prev: None,
                bench: false,
                keywords: BENCH_KEYWORDS,
                setup: |_| {},
            },
        ];
        let mut messages = Vec::new();
        for c in cases {
            let mut fx = Fixture::publish_ready(c.version, c.prev, c.bench);
            (c.setup)(&mut fx);
            let before = fx.snapshot(true);
            let r = fx.run("release-publish.sh", &[]);
            let msg = assert_refused(&fx, &before, &r, c.name, c.keywords);
            assert!(
                !fx.work.join("dist").exists(),
                "{}: a refused publish built dist/",
                c.name
            );
            messages.push((c.group, msg));
        }
        assert_distinct_messages(&messages);
    }

    // ---- criterion 7: the happy path -----------------------------------------------------------

    /// A local hook that records, whenever a tag ref is written, whether dist/ already held the
    /// tarball: dist must be built before the tag is created.
    fn watch_tag_creation(fx: &Fixture) -> std::path::PathBuf {
        let marker = fx.tmp.path().join("tag-marker");
        let dist = fx.work.join("dist");
        let hook = format!(
            "#!/bin/sh\n[ \"$1\" = committed ] || exit 0\nwhile read -r old new ref; do\n  case \"$ref\" in\n    refs/tags/*)\n      if [ -f '{d}/SHA256SUMS' ] && ls '{d}'/*.tar.gz >/dev/null 2>&1; then\n        echo \"dist-first $ref\" >>'{m}'\n      else\n        echo \"NO-DIST $ref\" >>'{m}'\n      fi ;;\n  esac\ndone\n",
            d = dist.display(),
            m = marker.display()
        );
        write_mode(
            &fx.work.join(".git/hooks/reference-transaction"),
            &hook,
            0o755,
        );
        marker
    }

    #[test]
    #[ignore = "needs bash, git, tar, shellcheck, jq, cargo-edit (make test-external)"]
    fn publish_tags_and_releases() {
        let fx = Fixture::publish_ready("0.2.0", Some("0.1.0"), true);
        let marker = watch_tag_creation(&fx);
        let head = fx.head();
        let r = fx.run("release-publish.sh", &[("DRY_RUN", "")]);
        assert!(r.ok(), "{}", r.dump());

        assert_eq!(
            fx.git(&["cat-file", "-t", "refs/tags/v0.2.0"]),
            "tag",
            "the tag is annotated"
        );
        assert_eq!(
            fx.git(&["rev-parse", "refs/tags/v0.2.0^{commit}"]),
            head,
            "the tag is at HEAD"
        );
        let local = fx.local_tag("v0.2.0").unwrap();
        assert_eq!(
            fx.origin_tag("v0.2.0"),
            Some(local),
            "the same tag object is on origin"
        );
        let marks = fs::read_to_string(&marker).unwrap_or_default();
        assert!(
            marks.lines().count() >= 1 && marks.lines().all(|l| l.starts_with("dist-first ")),
            "dist must be built before the tag is created: {marks:?}"
        );

        let calls = fx.gh_calls();
        let create: Vec<&String> = calls
            .iter()
            .filter(|l| l.starts_with("release create"))
            .collect();
        assert_eq!(create.len(), 1, "one `gh release create`: {calls:?}");
        let words: Vec<&str> = create[0].split_whitespace().collect();
        assert_eq!(words[2], "v0.2.0", "{}", create[0]);
        assert!(
            words.contains(&"--verify-tag") && words.contains(&"--generate-notes"),
            "{}",
            create[0]
        );
        let mut assets: Vec<&str> = words
            .iter()
            .copied()
            .skip(3)
            .filter(|w| w.contains('/') || w.ends_with(".tar.gz") || w.ends_with("SHA256SUMS"))
            .map(|w| w.rsplit('/').next().unwrap())
            .collect();
        assets.sort_unstable();
        assert_eq!(
            assets,
            ["SHA256SUMS", "hali-v0.2.0-x86_64-linux.tar.gz"],
            "exactly two files: {}",
            create[0]
        );
        assert!(
            fx.work
                .join("dist/hali-v0.2.0-x86_64-linux.tar.gz")
                .is_file()
        );
        // Publishing leaves the tree clean and on main at the same commit.
        assert_eq!(fx.head(), head);
        assert_eq!(fx.git(&["status", "--porcelain=v1"]), "");
        assert_eq!(fx.git(&["rev-parse", "--abbrev-ref", "HEAD"]), "main");
    }

    // ---- criterion 8: resume -------------------------------------------------------------------

    #[test]
    #[ignore = "needs bash, git, tar, shellcheck, jq, cargo-edit (make test-external)"]
    fn publish_resumes_after_failed_push() {
        let fx = Fixture::publish_ready("0.2.0", Some("0.1.0"), true);
        let head = fx.head();
        // Origin refuses tag pushes, as a dropped connection would.
        let hook = fx.origin.join("hooks/pre-receive");
        write_mode(
            &hook,
            "#!/bin/sh\nwhile read -r old new ref; do\n  case \"$ref\" in refs/tags/*) echo 'rejected by the test hook' >&2; exit 1 ;; esac\ndone\n",
            0o755,
        );
        assert_eq!(
            fs::metadata(&hook).unwrap().permissions().mode() & 0o111,
            0o111
        );
        let r = fx.run("release-publish.sh", &[]);
        assert!(!r.ok(), "the push must fail\n{}", r.dump());
        let tag = fx
            .local_tag("v0.2.0")
            .expect("the local tag stays after a failed push");
        assert_eq!(fx.git(&["rev-parse", "refs/tags/v0.2.0^{commit}"]), head);
        assert!(fx.origin_tag("v0.2.0").is_none());
        assert!(
            fx.gh_mutations().is_empty(),
            "released although the push failed: {:?}",
            fx.gh_mutations()
        );

        fs::remove_file(&hook).unwrap();
        let r = fx.run("release-publish.sh", &[]);
        assert!(r.ok(), "the rerun must resume\n{}", r.dump());
        assert_eq!(
            fx.local_tag("v0.2.0"),
            Some(tag.clone()),
            "the tag was moved or recreated"
        );
        assert_eq!(fx.origin_tag("v0.2.0"), Some(tag));
        assert_eq!(
            fx.gh_mutations()
                .iter()
                .filter(|l| l.starts_with("release create"))
                .count(),
            1
        );
    }

    #[test]
    #[ignore = "needs bash, git, tar, shellcheck, jq, cargo-edit (make test-external)"]
    fn publish_resumes_after_failed_release() {
        let mut fx = Fixture::publish_ready("0.2.0", Some("0.1.0"), true);
        fx.set_var("FAKE_GH_RELEASE_CREATE_EXIT", "1");
        let r = fx.run("release-publish.sh", &[]);
        assert!(!r.ok(), "release create fails\n{}", r.dump());
        let tag = fx
            .local_tag("v0.2.0")
            .expect("the tag stays after a failed release create");
        assert_eq!(
            fx.origin_tag("v0.2.0"),
            Some(tag.clone()),
            "the tag was pushed before the release"
        );

        fx.set_var("FAKE_GH_RELEASE_CREATE_EXIT", "0");
        let r = fx.run("release-publish.sh", &[]);
        assert!(r.ok(), "the rerun must resume\n{}", r.dump());
        assert_eq!(
            fx.local_tag("v0.2.0"),
            Some(tag.clone()),
            "the tag was moved or recreated"
        );
        assert_eq!(fx.origin_tag("v0.2.0"), Some(tag));
        assert_eq!(
            fx.gh_mutations()
                .iter()
                .filter(|l| l.starts_with("release create"))
                .count(),
            2,
            "one failed and one successful release create"
        );
    }

    #[test]
    #[ignore = "needs bash, git, tar, shellcheck, jq, cargo-edit (make test-external)"]
    fn publish_resumes_with_tag_only_on_origin() {
        let mut fx = Fixture::publish_ready("0.2.0", Some("0.1.0"), true);
        fx.set_var("FAKE_GH_RELEASE_CREATE_EXIT", "1");
        assert!(!fx.run("release-publish.sh", &[]).ok());
        let tag = fx.origin_tag("v0.2.0").expect("the tag is on origin");
        // The local tag is lost (a fresh clone, say); the tag on origin at HEAD must be kept.
        fx.git(&["tag", "-d", "v0.2.0"]);
        fx.set_var("FAKE_GH_RELEASE_CREATE_EXIT", "0");
        let r = fx.run("release-publish.sh", &[]);
        assert!(
            r.ok(),
            "the rerun must resume from the tag on origin\n{}",
            r.dump()
        );
        assert_eq!(
            fx.origin_tag("v0.2.0"),
            Some(tag),
            "the tag on origin was moved"
        );
        assert!(
            fx.gh_mutations()
                .last()
                .unwrap()
                .starts_with("release create")
        );
    }

    // ---- criterion 9: v0.1.0 needs benchmark results -------------------------------------------

    #[test]
    #[ignore = "needs bash, git, tar, shellcheck, jq, cargo-edit (make test-external)"]
    fn v0_1_0_needs_bench_results() {
        // prepare: 0.0.9 + minor = 0.1.0
        let fx = Fixture::with(Opts {
            version: "0.0.9",
            bench_heading: false,
            default_hooks: false,
        });
        let before = fx.snapshot(true);
        let r = fx.run("release-prepare.sh", &[("BUMP", "minor")]);
        assert_refused(&fx, &before, &r, "prepare v0.1.0", BENCH_KEYWORDS);
        fx.edit(ADR_PATH, BENCH_FOR_ADR);
        fx.commit_push("adr: record benchmark results");
        let r = fx.run("release-prepare.sh", &[("BUMP", "minor")]);
        assert!(r.ok(), "with the heading, prepare passes\n{}", r.dump());
        assert!(
            fx.origin_git(&["rev-parse", "-q", "--verify", "refs/heads/release/v0.1.0"])
                .len()
                >= 40
        );

        // publish
        let fx = Fixture::publish_ready("0.1.0", None, false);
        let before = fx.snapshot(true);
        let r = fx.run("release-publish.sh", &[]);
        assert_refused(&fx, &before, &r, "publish v0.1.0", BENCH_KEYWORDS);
        assert!(!fx.work.join("dist").exists());
        fx.edit(ADR_PATH, BENCH_FOR_ADR);
        fx.commit_push("adr: record benchmark results");
        let r = fx.run("release-publish.sh", &[]);
        assert!(r.ok(), "with the heading, publish passes\n{}", r.dump());
        assert!(fx.origin_tag("v0.1.0").is_some());
    }
}
