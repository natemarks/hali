//! Goal tests for `hali relink` and move detection on `hali add` through the real binary (#21,
//! spec §6, §9.1 **Relink:**, §9.4, §9.5, §9.8). Locked golden snapshots sit in tests/snapshots/
//! (hand-written from the contract; the ids are those `hali add` mints with `HALI_TEST_ID_SEED=1`
//! for the first file, as in tests/cli_scan.rs):
//!   `cli_relink__relink_json`, `cli_relink__relink_explicit_json`,
//!   `cli_dry_run__relink_dry_run_json`, `cli_relink__schema_list_json` (stdout) and the stderr
//!   envelopes `cli_relink__possible_move_json_envelope` and `cli_relink__hash_mismatch_json_envelope`.
//!
//! The contract the tests fix (the issue and the approved plan leave these open; change them,
//! with the human, if the wording should differ):
//! - Output of `relink [--root DIR]…` (a search), keys in this order: `schema_version`, `roots`
//!   (the roots searched, real paths, in argument order without repeats; with no `--root`, the
//!   usable default roots), `counts` (`relinked`, `ambiguous`, `unmatched`, exact), `relinked`
//!   (`{"resource": <Resource record after>, "from": "<old path>"}`), `ambiguous` (`{"resource":
//!   <record>, "candidates": [<path>, at most 10]}`), `unmatched` (the Missing Resources with no
//!   match: records), `truncated` (a list was cut at 100). Records are in id order.
//! - Output of `relink <ref> <path>`: `schema_version`, `resource` (the record after), `from`
//!   (the old path), `refreshed` (true when `--force` also refreshed size, mtime and hash).
//! - Under `--dry-run` both add `dry_run` and `changes` (§9.5).
//! - `relink <ref> <path>` stores the real path of `<path>`. `<path>` that is no regular file is
//!   `NOT_A_REGULAR_FILE` (exit 1), one that cannot be read `FILE_UNREADABLE` (exit 1). A `--root`
//!   that is not a directory is a usage error (exit 2). `relink <ref>` alone, `<ref> <path>` with
//!   `--root`, `--force` in a search, or more than two words are usage errors (exit 2).
//! - `HASH_MISMATCH`: message `<path> does not have the content of Resource <id>; nothing was
//!   written`, details `{"resource","path","expected","actual"}` (the two SHA-256s), a fix that
//!   mentions `--force`, and `hali relink <id> <path> --force` (the path POSIX single-quoted) as
//!   the suggested command.
//! - `relink <ref> <path> --force` on different content asks (§9.8): prompt on stderr `relink
//!   resource <id> "<title>" <path>? [y/N] ` (the new path), and without both terminals fails with
//!   `CONFIRMATION_REQUIRED` (details command `relink`), suggesting `hali relink <id> <path>
//!   --force --yes`. On the same content it never asks. `--dry-run` never asks. A search never
//!   asks, never reads stdin and ignores `--yes`.
//! - `POSSIBLE_MOVE`: message `<path> has the content of missing Resource <ids joined with ", ">;
//!   nothing was written`, details `{"path","resources":[ids sorted]}`, a fix that mentions
//!   `relink` and `--new`, and `hali relink <id> <path>` (path single-quoted) as the suggested
//!   command, or bare `hali relink` when several Resources match. `add` checks it after the paths
//!   resolve and before Type and Concept references; with several paths it names the first
//!   offender and nothing is added.
//! - Default roots are the one directory of the `files.dir` chain: `[files] dir` (with a leading
//!   `~` expanded to $HOME), else `XDG_DOCUMENTS_DIR` of `$XDG_CONFIG_HOME/user-dirs.dirs` plus
//!   `/hali`, else `$HOME/Documents/hali`. The first one defined is the only one; if it is not a
//!   directory it is skipped, silently.
//! - A candidate is never a path some Resource already records, and one path found under several
//!   roots counts once.
//! - Text mode prints, per relinked Resource, a line with `relinked`, the old and the new path;
//!   per ambiguous one a line with `ambiguous` and its id; values are escaped for the terminal.
//!
//! Isolation (the walker honours ignore files in the root's parents and the user's git
//! configuration): the tree is built under the system temp dir, never under `target/`; the data
//! dir is a sibling of the root; `HOME`, `XDG_CONFIG_HOME`, `GIT_CONFIG_GLOBAL` and
//! `GIT_CONFIG_SYSTEM` point at temp paths and the rest of the environment is cleared; every test
//! asserts that no ancestor of the temp dir holds a `.gitignore`, `.ignore`, `.haliignore`, `.git`
//! or `.jj`.
//!
//! The implementer must also update the 25-name schema lists in tests/cli_refresh.rs,
//! tests/cli_relation.rs, tests/cli_type.rs, tests/cli_schema.rs (its snapshot
//! tests/snapshots/cli_schema__schema_list_json.snap too), tests/core_schema.rs and the others
//! that list the schema names, the `[user] name and [vocabulary] language` fix text in
//! tests/cli_scan.rs and tests/cli_error_text.rs, the help-text snapshots that list subcommands,
//! and the `relink` row of tests/dry_run_guard.rs (`.goal/checks/02-dry_run_guard_relink.sh`).

#[cfg(feature = "goal")]
mod goal {
    use assert_cmd::Command;
    use serde_json::{Value, json};
    use std::collections::{BTreeMap, BTreeSet};
    use std::ffi::OsStr;
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime};
    use tempfile::TempDir;

    const NOW: &str = "2026-10-24T09:30:00Z";
    const NOW_RELINK: &str = "2026-10-25T08:00:00Z";
    const PINNED_SECS: u64 = 1_792_504_931; // 2026-10-20T14:02:11Z
    const NEW_SECS: u64 = 1_792_663_200; // 2026-10-22T10:00:00Z
    const R1: &str = "018bcfe5-6801-710a-adec-89025cc1beeb";
    const SHA_HELLO: &str = "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03";
    const SHA_WORLD: &str = "a948904f2f0f479b8f8197694b30184b0d2ed1c1cd2a1ec0fb85d299a192a447";
    const SHA_UPPER: &str = "3b09aeb6f5f5336beb205d7f720371bc927cd46c21922e334d47ba264acb5ba4";
    const FIX: &str =
        "Run it again with --yes to confirm, or with --dry-run to see what it would change.";

    struct Env {
        _tmp: TempDir,
        base: PathBuf,
        data: PathBuf,
    }

    struct Out {
        code: Option<i32>,
        stdout: String,
        stderr: String,
    }

    fn privileged() -> bool {
        // SAFETY: geteuid has no preconditions.
        unsafe { libc::geteuid() == 0 }
    }

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::new(secs, 0)
    }

    fn pin(path: &Path, secs: u64) {
        std::fs::File::open(path)
            .unwrap()
            .set_modified(at(secs))
            .unwrap();
    }

    impl Env {
        /// A fresh temp dir with `root/`, `cfg/` and `home/`; the data dir `data/` is not
        /// created.
        fn new() -> Env {
            let tmp = tempfile::tempdir().unwrap();
            let base = std::fs::canonicalize(tmp.path()).unwrap();
            for dir in base.ancestors().skip(1) {
                for name in [".gitignore", ".ignore", ".haliignore", ".git", ".jj"] {
                    assert!(
                        !dir.join(name).exists(),
                        "{} exists: the host's ignore rules would reach the search root; point TMPDIR elsewhere",
                        dir.join(name).display()
                    );
                }
            }
            for d in ["root", "cfg", "home"] {
                std::fs::create_dir_all(base.join(d)).unwrap();
            }
            let data = base.join("data");
            Env {
                _tmp: tmp,
                base,
                data,
            }
        }

        /// The standard scene: `root/a.txt` ("hello\n", mtime 2026-10-20T14:02:11Z) cataloged as
        /// R1 (seed 1, the clock at 2026-10-24T09:30:00Z), then moved to `root/moved/renamed.txt`
        /// with mtime 2026-10-22T10:00:00Z, next to an unrelated `root/other.bin`.
        fn moved() -> Env {
            let env = Env::new();
            let id = env.add("a.txt", b"hello\n", PINNED_SECS, "1");
            assert_eq!(id, R1);
            env.mv("a.txt", "moved/renamed.txt", NEW_SECS);
            env.put_at("other.bin", b"xyz!", PINNED_SECS);
            env
        }

        fn root(&self) -> PathBuf {
            self.base.join("root")
        }

        fn root_str(&self) -> String {
            self.root().to_str().unwrap().to_string()
        }

        /// `root/<rel>` as a string.
        fn p(&self, rel: &str) -> String {
            self.root().join(rel).to_str().unwrap().to_string()
        }

        fn data(&self) -> PathBuf {
            self.data.clone()
        }

        fn home(&self) -> PathBuf {
            self.base.join("home")
        }

        /// Writes `root/<rel>` (creating parents), mtime pinned.
        fn put_at(&self, rel: &str, bytes: &[u8], secs: u64) -> PathBuf {
            self.put_abs(&self.root().join(rel), bytes, secs)
        }

        fn put_abs(&self, path: &Path, bytes: &[u8], secs: u64) -> PathBuf {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
            pin(path, secs);
            path.to_path_buf()
        }

        /// Moves `root/<from>` to `root/<to>`, mtime pinned.
        fn mv(&self, from: &str, to: &str, secs: u64) {
            let to = self.root().join(to);
            std::fs::create_dir_all(to.parent().unwrap()).unwrap();
            std::fs::rename(self.root().join(from), &to).unwrap();
            pin(&to, secs);
        }

        /// Moves `root/<from>` to the absolute path `to`, mtime pinned.
        fn mv_abs(&self, from: &str, to: &Path, secs: u64) {
            std::fs::create_dir_all(to.parent().unwrap()).unwrap();
            std::fs::rename(self.root().join(from), to).unwrap();
            pin(to, secs);
        }

        /// Writes `root/<rel>` and catalogs it with `hali add` (id seed `seed`); returns its id.
        fn add(&self, rel: &str, bytes: &[u8], secs: u64, seed: &str) -> String {
            let path = self.put_at(rel, bytes, secs);
            let out = self.run_env(
                &[("HALI_TEST_ID_SEED", seed)],
                ["--json", "add", path.to_str().unwrap()],
            );
            assert_eq!(out.code, Some(0), "setup add {rel}: {}", out.stderr);
            let v: Value = serde_json::from_str(&out.stdout).unwrap();
            v["added"][0]["id"].as_str().unwrap().to_string()
        }

        fn config(&self, text: &str) {
            let dir = self.base.join("cfg/hali");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("config.toml"), text).unwrap();
        }

        fn vars(&self, extra: &[(&str, &str)]) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
            let mut vars: Vec<(std::ffi::OsString, std::ffi::OsString)> = vec![
                ("HOME".into(), self.home().into()),
                ("XDG_CONFIG_HOME".into(), self.base.join("cfg").into()),
                ("HALI_DATA_DIR".into(), self.data.clone().into()),
                (
                    "GIT_CONFIG_GLOBAL".into(),
                    self.base.join("gitconfig").into(),
                ),
                (
                    "GIT_CONFIG_SYSTEM".into(),
                    self.base.join("gitconfig-system").into(),
                ),
                ("HALI_TEST_NOW".into(), NOW.into()),
                ("HALI_TEST_ID_SEED".into(), "1".into()),
            ];
            for (k, v) in extra {
                vars.retain(|(name, _)| name != OsStr::new(k));
                vars.push((k.into(), v.into()));
            }
            vars
        }

        fn exec<I, S>(
            &self,
            cwd: Option<&Path>,
            extra: &[(&str, &str)],
            stdin: &[u8],
            args: I,
        ) -> Out
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            let mut cmd = Command::cargo_bin("hali").unwrap();
            cmd.env_clear()
                .envs(self.vars(extra))
                .timeout(Duration::from_secs(60))
                .write_stdin(stdin.to_vec());
            if let Some(cwd) = cwd {
                cmd.current_dir(cwd);
            }
            let out = cmd.args(args).output().unwrap();
            Out {
                code: out.status.code(),
                stdout: String::from_utf8(out.stdout).unwrap(),
                stderr: String::from_utf8(out.stderr).unwrap(),
            }
        }

        fn run<I, S>(&self, args: I) -> Out
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            self.exec(None, &[], b"", args)
        }

        fn run_env<I, S>(&self, extra: &[(&str, &str)], args: I) -> Out
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            self.exec(None, extra, b"", args)
        }

        /// A relink-day run: the clock at 2026-10-25T08:00:00Z.
        fn later<I, S>(&self, args: I) -> Out
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            self.exec(None, &[("HALI_TEST_NOW", NOW_RELINK)], b"", args)
        }

        /// A terminal (`HALI_TEST_TTY=1`) whose user types `answer`.
        fn tty<I, S>(&self, answer: &[u8], args: I) -> Out
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            self.exec(
                None,
                &[("HALI_TEST_TTY", "1"), ("HALI_TEST_NOW", NOW_RELINK)],
                answer,
                args,
            )
        }

        /// The output with the temp dir shown as `/TMP`.
        fn clean(&self, text: &str) -> String {
            text.replace(self.base.to_str().unwrap(), "/TMP")
        }

        /// Names -> bytes of the files in the data dir; `None` when it does not exist.
        fn state(&self) -> Option<BTreeMap<String, Vec<u8>>> {
            if !self.data.exists() {
                return None;
            }
            Some(
                std::fs::read_dir(&self.data)
                    .unwrap()
                    .map(|e| {
                        let e = e.unwrap();
                        (
                            e.file_name().into_string().unwrap(),
                            std::fs::read(e.path()).unwrap(),
                        )
                    })
                    .collect(),
            )
        }

        fn resources_ttl(&self) -> String {
            std::fs::read_to_string(self.data.join("resources.ttl")).unwrap()
        }

        /// Every entry under `dir`: its kind, bytes and mtime.
        fn tree(&self, dir: &Path) -> BTreeMap<String, (String, Vec<u8>, SystemTime)> {
            fn walk(
                dir: &Path,
                base: &Path,
                out: &mut BTreeMap<String, (String, Vec<u8>, SystemTime)>,
            ) {
                for e in std::fs::read_dir(dir).unwrap() {
                    let path = e.unwrap().path();
                    let meta = std::fs::symlink_metadata(&path).unwrap();
                    let name = path.strip_prefix(base).unwrap().display().to_string();
                    if meta.is_dir() {
                        out.insert(name, ("dir".into(), vec![], meta.modified().unwrap()));
                        walk(&path, base, out);
                    } else if meta.is_file() {
                        out.insert(
                            name,
                            (
                                "file".into(),
                                std::fs::read(&path).unwrap(),
                                meta.modified().unwrap(),
                            ),
                        );
                    } else {
                        out.insert(name, ("other".into(), vec![], meta.modified().unwrap()));
                    }
                }
            }
            let mut out = BTreeMap::new();
            walk(dir, dir, &mut out);
            out
        }
    }

    fn json(s: &str) -> Value {
        assert_eq!(
            s.matches('\n').count(),
            1,
            "one compact line expected: {s:?}"
        );
        serde_json::from_str(s).unwrap_or_else(|e| panic!("not JSON ({e}): {s:?}"))
    }

    fn ok(out: &Out) -> Value {
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        json(&out.stdout)
    }

    /// The error envelope of a failed run: exit code `code`, nothing on stdout.
    fn failed(out: &Out, code: i32, rule: &str) -> Value {
        assert_eq!(out.code, Some(code), "stderr: {}", out.stderr);
        assert_eq!(out.stdout, "");
        let v = json(&out.stderr);
        assert_eq!(v["error"]["rule"], rule, "{}", out.stderr);
        v
    }

    fn snapshot_is_unchanged(env: &Env, before: &Option<BTreeMap<String, Vec<u8>>>, what: &str) {
        assert_eq!(&env.state(), before, "{what}: the data dir changed");
    }

    // ---- row 1, 2: a unique match is relinked; what is written -----------------------------

    #[test]
    fn relink_search_json() {
        let env = Env::moved();
        let out = env.later(["--json", "relink", "--root", env.root_str().as_str()]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!("cli_relink__relink_json", env.clean(&out.stdout));
        });
        let ttl = env.resources_ttl();
        assert!(ttl.contains(&format!("file://{}", env.p("moved/renamed.txt"))));
        assert!(!ttl.contains(&format!("file://{}", env.p("a.txt"))));
    }

    #[test]
    fn relink_dry_run_json() {
        let env = Env::moved();
        let before = env.state();
        let out = env.later([
            "--json",
            "--dry-run",
            "relink",
            "--root",
            env.root_str().as_str(),
        ]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        snapshot_is_unchanged(&env, &before, "a dry run");
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!("cli_dry_run__relink_dry_run_json", env.clean(&out.stdout));
        });
    }

    /// The triples of a Turtle file as `<...> <...> ... .` strings with the temp dir as `/TMP`.
    fn triples(env: &Env, bytes: &[u8]) -> BTreeSet<String> {
        oxttl::TurtleParser::new()
            .for_slice(bytes)
            .map(|t| env.clean(&format!("{} .", t.unwrap())))
            .collect()
    }

    /// Runs `args` (after `--json`) as a real run in one scene and as a dry run in an identical
    /// one: the dry JSON is the real JSON plus `dry_run` and `changes`, the dry run wrote
    /// nothing, and `changes` is exactly what the real run changed in resources.ttl.
    fn assert_dry_matches_real(scene: fn() -> Env, args: &dyn Fn(&Env) -> Vec<String>) {
        let (real_env, dry_env) = (scene(), scene());
        let before_bytes = std::fs::read(real_env.data().join("resources.ttl")).unwrap();
        let before = triples(&real_env, &before_bytes);
        let real_args = args(&real_env);
        let real =
            real_env.later(std::iter::once("--json".to_string()).chain(real_args.iter().cloned()));
        let real_json = ok(&real);
        let after = triples(&real_env, real_env.resources_ttl().as_bytes());

        let dry_before = dry_env.state();
        let dry = dry_env.later(
            ["--json".to_string(), "--dry-run".to_string()]
                .into_iter()
                .chain(args(&dry_env)),
        );
        let dry_json = ok(&dry);
        assert_eq!(dry_env.state(), dry_before, "the dry run wrote");

        let clean = |v: &Value, env: &Env| -> Value {
            serde_json::from_str(&env.clean(&v.to_string())).unwrap()
        };
        let real_json = clean(&real_json, &real_env);
        let dry_json = clean(&dry_json, &dry_env);
        let mut dry_obj = dry_json.as_object().unwrap().clone();
        let keys: Vec<String> = dry_obj.keys().cloned().collect();
        let n = keys.len();
        assert_eq!(&keys[n - 2..], ["dry_run", "changes"], "{keys:?}");
        assert_eq!(dry_obj["dry_run"], json!(true));
        let changes = dry_obj.remove("changes").unwrap();
        dry_obj.remove("dry_run");
        assert_eq!(Value::Object(dry_obj), real_json, "same payload");
        assert!(real_json.get("dry_run").is_none() && real_json.get("changes").is_none());

        let mut added = BTreeSet::new();
        let mut removed = BTreeSet::new();
        for c in changes.as_array().unwrap() {
            let t = c["triple"].as_str().unwrap().to_string();
            match c["op"].as_str().unwrap() {
                "add" => added.insert(t),
                "remove" => removed.insert(t),
                other => panic!("op {other}"),
            };
        }
        assert_eq!(added, after.difference(&before).cloned().collect());
        assert_eq!(removed, before.difference(&after).cloned().collect());
        assert!(!added.is_empty() || !removed.is_empty());
    }

    #[test]
    fn search_dry_run_matches_real_run() {
        assert_dry_matches_real(Env::moved, &|env| {
            vec!["relink".into(), "--root".into(), env.root_str()]
        });
    }

    #[test]
    fn explicit_dry_run_matches_real_run() {
        assert_dry_matches_real(Env::moved, &|env| {
            vec!["relink".into(), R1.into(), env.p("moved/renamed.txt")]
        });
    }

    fn edited() -> Env {
        let env = Env::moved();
        env.put_at("moved/renamed.txt", b"hello world\n", NEW_SECS);
        env
    }

    #[test]
    fn forced_explicit_dry_run_matches_real_run() {
        // The real run needs `--yes` (no terminal); the dry run never asks.
        let real_env = edited();
        let dry_env = edited();
        let real = real_env.later([
            "--json",
            "--yes",
            "relink",
            R1,
            real_env.p("moved/renamed.txt").as_str(),
            "--force",
        ]);
        let real_json = ok(&real);
        assert_eq!(real_json["refreshed"], json!(true));
        let dry_before = dry_env.state();
        let dry = dry_env.later([
            "--json",
            "--dry-run",
            "relink",
            R1,
            dry_env.p("moved/renamed.txt").as_str(),
            "--force",
        ]);
        let dry_json = ok(&dry);
        assert_eq!(dry_env.state(), dry_before);
        assert_eq!(dry_json["refreshed"], json!(true));
        let triples: Vec<&str> = dry_json["changes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["triple"].as_str().unwrap())
            .collect();
        assert!(triples.iter().any(|t| t.contains(SHA_WORLD)), "{triples:?}");
        assert!(triples.iter().any(|t| t.contains(SHA_HELLO)), "{triples:?}");
    }

    // ---- row 3, 4: several matches, one-to-one --------------------------------------------------

    #[test]
    fn several_matches_are_reported_and_nothing_changes() {
        let env = Env::moved();
        env.put_at("moved/copy.txt", b"hello\n", NEW_SECS);
        let before = env.state();
        let out = env.later(["--json", "relink", "--root", env.root_str().as_str()]);
        let v = ok(&out);
        assert_eq!(
            v["counts"],
            json!({"relinked": 0, "ambiguous": 1, "unmatched": 0})
        );
        assert_eq!(v["relinked"], json!([]));
        assert_eq!(v["unmatched"], json!([]));
        let a = &v["ambiguous"][0];
        assert_eq!(a["resource"]["id"], R1);
        assert_eq!(a["resource"]["path"], env.p("a.txt"), "the old path stays");
        assert_eq!(a["resource"]["missing"], json!(true));
        assert_eq!(
            a["candidates"],
            json!([env.p("moved/copy.txt"), env.p("moved/renamed.txt")])
        );
        snapshot_is_unchanged(&env, &before, "an ambiguous search");
    }

    #[test]
    fn one_file_for_two_missing_resources_is_ambiguous() {
        let env = Env::new();
        let a = env.add("a.txt", b"same\n", PINNED_SECS, "1");
        let b = env.add("b.txt", b"same\n", PINNED_SECS, "2");
        std::fs::remove_file(env.root().join("a.txt")).unwrap();
        std::fs::remove_file(env.root().join("b.txt")).unwrap();
        env.put_at("moved.txt", b"same\n", NEW_SECS);
        let before = env.state();
        let v = ok(&env.later(["--json", "relink", "--root", env.root_str().as_str()]));
        assert_eq!(
            v["counts"],
            json!({"relinked": 0, "ambiguous": 2, "unmatched": 0})
        );
        let ids: Vec<&str> = v["ambiguous"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["resource"]["id"].as_str().unwrap())
            .collect();
        let mut want = vec![a.as_str(), b.as_str()];
        want.sort();
        assert_eq!(ids, want, "id order");
        for e in v["ambiguous"].as_array().unwrap() {
            assert_eq!(e["candidates"], json!([env.p("moved.txt")]));
        }
        snapshot_is_unchanged(&env, &before, "an ambiguous search");
    }

    #[test]
    fn a_candidate_with_other_content_is_unmatched() {
        // Same size, other content: it is not the file.
        let env = Env::moved();
        env.put_at("moved/renamed.txt", b"HELLO\n", NEW_SECS);
        let before = env.state();
        let v = ok(&env.later(["--json", "relink", "--root", env.root_str().as_str()]));
        assert_eq!(
            v["counts"],
            json!({"relinked": 0, "ambiguous": 0, "unmatched": 1})
        );
        assert_eq!(v["unmatched"][0]["id"], R1);
        assert_eq!(v["unmatched"][0]["missing"], json!(true));
        snapshot_is_unchanged(&env, &before, "an unmatched search");
    }

    #[test]
    fn two_missing_resources_each_with_its_own_file_are_both_relinked() {
        let env = Env::new();
        let a = env.add("a.txt", b"alpha\n", PINNED_SECS, "1");
        let b = env.add("b.txt", b"beta beta\n", PINNED_SECS, "2");
        env.mv("a.txt", "new/x.txt", NEW_SECS);
        env.mv("b.txt", "new/y.txt", NEW_SECS);
        let v = ok(&env.later(["--json", "relink", "--root", env.root_str().as_str()]));
        assert_eq!(
            v["counts"],
            json!({"relinked": 2, "ambiguous": 0, "unmatched": 0})
        );
        let relinked: BTreeMap<&str, &str> = v["relinked"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                (
                    e["resource"]["id"].as_str().unwrap(),
                    e["resource"]["path"].as_str().unwrap(),
                )
            })
            .collect();
        assert_eq!(relinked[a.as_str()], env.p("new/x.txt"));
        assert_eq!(relinked[b.as_str()], env.p("new/y.txt"));
    }

    #[test]
    fn a_file_that_is_already_cataloged_is_not_a_candidate() {
        // The cataloged copy has R1's content: it is no place for R1 to have gone.
        let env = Env::moved();
        let out = env.run_env(
            &[("HALI_TEST_ID_SEED", "2")],
            [
                "--json",
                "add",
                "--new",
                env.p("moved/renamed.txt").as_str(),
            ],
        );
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        let v = ok(&env.later(["--json", "relink", "--root", env.root_str().as_str()]));
        assert_eq!(
            v["counts"],
            json!({"relinked": 0, "ambiguous": 0, "unmatched": 1})
        );
    }

    // ---- row 7: one mutation, one SHACL check, only resources.ttl ----------------------------------

    #[test]
    fn only_resources_ttl_is_written() {
        let env = Env::moved();
        // A vocabulary and hand-written extensions: they must stay byte for byte.
        let out = env.run(["--json", "concept", "add", "machine learning"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        std::fs::write(
            env.data().join("extensions.ttl"),
            "@prefix ex: <http://example.org/> .\nex:a ex:b ex:c .\n",
        )
        .unwrap();
        let before = env.state().unwrap();
        let root_before = env.tree(&env.root());
        let out = env.later(["--json", "relink", "--root", env.root_str().as_str()]);
        ok(&out);
        let after = env.state().unwrap();
        assert_eq!(
            before.keys().collect::<Vec<_>>(),
            after.keys().collect::<Vec<_>>(),
            "no file appears or goes in the data dir"
        );
        for (name, bytes) in &before {
            if name == "resources.ttl" {
                assert_ne!(bytes, &after[name], "resources.ttl is rewritten");
            } else {
                assert_eq!(bytes, &after[name], "{name} is untouched");
            }
        }
        assert_eq!(
            env.tree(&env.root()),
            root_before,
            "nothing under the root changes"
        );
    }

    fn extension_forbidding_a_move(env: &Env) {
        let url = format!("file://{}", env.p("a.txt"));
        std::fs::write(
            env.data().join("extensions.ttl"),
            format!(
                "@prefix hali: <https://w3id.org/hali#> .\n\
                 @prefix dcat: <http://www.w3.org/ns/dcat#> .\n\
                 @prefix sh: <http://www.w3.org/ns/shacl#> .\n\
                 @prefix ex: <http://example.org/shapes#> .\n\n\
                 ex:UrlsStay a sh:NodeShape ;\n\
                 \tsh:targetClass hali:Resource ;\n\
                 \tsh:property ex:UrlRule .\n\
                 ex:UrlRule sh:path dcat:downloadURL ;\n\
                 \tsh:in ( <{url}> ) ;\n\
                 \tsh:message \"The file stays\"@en .\n"
            ),
        )
        .unwrap();
    }

    #[test]
    fn a_relink_that_breaks_a_shape_is_a_shacl_violation_and_writes_nothing() {
        for explicit in [false, true] {
            let env = Env::moved();
            extension_forbidding_a_move(&env);
            let before = env.state();
            let out = if explicit {
                env.later(["--json", "relink", R1, env.p("moved/renamed.txt").as_str()])
            } else {
                env.later(["--json", "relink", "--root", env.root_str().as_str()])
            };
            let v = failed(&out, 1, "SHACL_VIOLATION");
            assert_eq!(v["error"]["details"]["total"], 1);
            snapshot_is_unchanged(&env, &before, "a refused relink");
        }
    }

    // ---- row 9: roots, and nothing to do -------------------------------------------------------------

    #[test]
    fn roots_are_real_paths_without_repeats() {
        let env = Env::moved();
        symlink(env.root(), env.base.join("link-to-root")).unwrap();
        let sub = env.p("moved");
        let v = ok(&env.later([
            "--json",
            "relink",
            "--root",
            env.base.join("link-to-root").to_str().unwrap(),
            "--root",
            env.root_str().as_str(),
            "--root",
            sub.as_str(),
        ]));
        assert_eq!(v["roots"], json!([env.root_str(), sub]));
        // The file lies under both roots, and is still one candidate.
        assert_eq!(v["counts"]["relinked"], 1, "{v}");
    }

    #[test]
    fn a_relative_root_is_resolved_against_the_current_directory() {
        let env = Env::moved();
        let out = env.exec(
            Some(&env.root()),
            &[("HALI_TEST_NOW", NOW_RELINK)],
            b"",
            ["--json", "relink", "--root", "moved/../moved/."],
        );
        let v = ok(&out);
        assert_eq!(v["roots"], json!([env.p("moved")]));
        assert_eq!(v["counts"]["relinked"], 1);
    }

    #[test]
    fn a_root_that_is_not_a_directory_is_a_usage_error() {
        let env = Env::moved();
        let before = env.state();
        let gone = env.p("no-such-dir");
        let file = env.p("other.bin");
        for root in [gone.as_str(), file.as_str()] {
            let out = env.later(["--json", "relink", "--root", root]);
            assert_eq!(out.code, Some(2), "{root}: {}", out.stderr);
            assert_eq!(out.stdout, "");
            assert!(out.stderr.contains(root), "{root}: {}", out.stderr);
            snapshot_is_unchanged(&env, &before, root);
        }
    }

    #[test]
    fn nothing_missing_writes_nothing_and_creates_no_data_dir() {
        let env = Env::new();
        env.put_at("a.txt", b"hello\n", PINNED_SECS);
        let v = ok(&env.run(["--json", "relink", "--root", env.root_str().as_str()]));
        assert_eq!(
            v,
            json!({"schema_version": 1, "roots": [env.root_str()],
                "counts": {"relinked": 0, "ambiguous": 0, "unmatched": 0},
                "relinked": [], "ambiguous": [], "unmatched": [], "truncated": false})
        );
        assert!(!env.data().exists(), "no data dir is created");

        // And with a store whose Resources are all there.
        let env = Env::new();
        env.add("a.txt", b"hello\n", PINNED_SECS, "1");
        let before = env.state();
        let v = ok(&env.later(["--json", "relink", "--root", env.root_str().as_str()]));
        assert_eq!(
            v["counts"],
            json!({"relinked": 0, "ambiguous": 0, "unmatched": 0})
        );
        snapshot_is_unchanged(&env, &before, "a search with nothing missing");
    }

    #[test]
    fn walk_skips_are_not_reported() {
        if privileged() {
            return;
        }
        let env = Env::moved();
        let locked = env.root().join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::write(locked.join("x.txt"), b"x").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let out = env.later(["--json", "relink", "--root", env.root_str().as_str()]);
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        let v = ok(&out);
        assert_eq!(v["counts"]["relinked"], 1);
        let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
        assert_eq!(
            keys,
            [
                "schema_version",
                "roots",
                "counts",
                "relinked",
                "ambiguous",
                "unmatched",
                "truncated"
            ]
        );
    }

    #[test]
    fn lists_are_capped_at_100_and_counts_stay_exact() {
        // 101 Resources, each moved to its own file: counts say 101, the list holds 100.
        let env = Env::new();
        let paths: Vec<(String, Vec<u8>)> = (0..101)
            .map(|i| {
                (
                    format!("f{i:03}.txt"),
                    format!("content {i}\n").into_bytes(),
                )
            })
            .collect();
        let mut args = vec!["--json".to_string(), "add".to_string()];
        for (name, bytes) in &paths {
            args.push(
                env.put_at(name, bytes, PINNED_SECS)
                    .to_str()
                    .unwrap()
                    .to_string(),
            );
        }
        let out = env.run(args);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        for (name, _) in &paths {
            env.mv(name, &format!("moved/{name}"), NEW_SECS);
        }
        let v = ok(&env.later(["--json", "relink", "--root", env.root_str().as_str()]));
        assert_eq!(v["counts"]["relinked"], 101);
        assert_eq!(v["relinked"].as_array().unwrap().len(), 100);
        assert_eq!(v["truncated"], json!(true));
        let ids: Vec<&str> = v["relinked"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["resource"]["id"].as_str().unwrap())
            .collect();
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted, "id order");
    }

    #[test]
    fn candidates_are_capped_at_ten() {
        let env = Env::moved();
        for i in 0..12 {
            env.put_at(&format!("moved/c{i:02}.txt"), b"hello\n", NEW_SECS);
        }
        let v = ok(&env.later(["--json", "relink", "--root", env.root_str().as_str()]));
        let c = v["ambiguous"][0]["candidates"].as_array().unwrap();
        assert_eq!(c.len(), 10);
        let strings: Vec<&str> = c.iter().map(|p| p.as_str().unwrap()).collect();
        let mut sorted = strings.clone();
        sorted.sort();
        assert_eq!(strings, sorted, "byte order");
        assert_eq!(strings[0], env.p("moved/c00.txt"));
    }

    // ---- row 10: explicit relink ---------------------------------------------------------------------

    #[test]
    fn relink_explicit_json() {
        let env = Env::moved();
        let out = env.later(["--json", "relink", R1, env.p("moved/renamed.txt").as_str()]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!("cli_relink__relink_explicit_json", env.clean(&out.stdout));
        });
        // The store now agrees with the file: nothing is Stale or Missing.
        let v = ok(&env.run(["--json", "find", "--stale"]));
        assert_eq!(v["total"], 0, "{v}");
        let v = ok(&env.run(["--json", "find", "--missing"]));
        assert_eq!(v["total"], 0, "{v}");
    }

    #[test]
    fn explicit_relink_by_the_old_path_and_the_real_path() {
        let env = Env::moved();
        symlink(
            env.root().join("moved/renamed.txt"),
            env.root().join("link.txt"),
        )
        .unwrap();
        // The reference is the old path (nothing is there now); the target is a symlink, and
        // the Resource records the real path.
        let v = ok(&env.later([
            "--json",
            "relink",
            env.p("a.txt").as_str(),
            env.p("link.txt").as_str(),
        ]));
        assert_eq!(v["resource"]["id"], R1);
        assert_eq!(v["resource"]["path"], env.p("moved/renamed.txt"));
        assert_eq!(v["from"], env.p("a.txt"));
    }

    #[test]
    fn a_resource_that_is_not_missing_can_be_relinked() {
        let env = Env::new();
        let id = env.add("a.txt", b"hello\n", PINNED_SECS, "1");
        env.put_at("copy.txt", b"hello\n", NEW_SECS);
        let v = ok(&env.later(["--json", "relink", id.as_str(), env.p("copy.txt").as_str()]));
        assert_eq!(v["from"], env.p("a.txt"));
        assert_eq!(v["resource"]["path"], env.p("copy.txt"));
        assert_eq!(v["resource"]["missing"], json!(false));
        assert_eq!(v["refreshed"], json!(false));
        assert_eq!(
            std::fs::read(env.root().join("a.txt")).unwrap(),
            b"hello\n",
            "the old file is never touched"
        );
    }

    #[test]
    fn explicit_relink_errors() {
        let env = Env::moved();
        let before = env.state();
        let v = failed(
            &env.later([
                "--json",
                "relink",
                "0192f0c4-0000-7000-8000-000000000999",
                env.p("moved/renamed.txt").as_str(),
            ]),
            1,
            "UNKNOWN_RESOURCE",
        );
        assert_eq!(
            v["error"]["details"]["ref"],
            "0192f0c4-0000-7000-8000-000000000999"
        );
        let v = failed(
            &env.later(["--json", "relink", R1, env.p("moved/none.txt").as_str()]),
            1,
            "NOT_A_REGULAR_FILE",
        );
        assert_eq!(v["error"]["details"]["kind"], "missing");
        let v = failed(
            &env.later(["--json", "relink", R1, env.p("moved").as_str()]),
            1,
            "NOT_A_REGULAR_FILE",
        );
        assert_eq!(v["error"]["details"]["kind"], "directory");
        if !privileged() {
            let locked = env.p("moved/renamed.txt");
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
            let out = env.later(["--json", "relink", R1, locked.as_str()]);
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();
            failed(&out, 1, "FILE_UNREADABLE");
        }
        snapshot_is_unchanged(&env, &before, "refused relinks");
    }

    #[test]
    fn relink_usage_errors() {
        let env = Env::moved();
        let before = env.state();
        // The command exists: a well-formed search is not a usage error.
        let control = env.later([
            "--json",
            "--dry-run",
            "relink",
            "--root",
            env.root_str().as_str(),
        ]);
        ok(&control);
        let moved = env.p("moved/renamed.txt");
        let root = env.root_str();
        let cases: Vec<Vec<&str>> = vec![
            vec!["relink", R1],
            vec!["relink", R1, moved.as_str(), "extra"],
            vec!["relink", R1, moved.as_str(), "--root", root.as_str()],
            vec!["relink", "--force"],
            vec!["relink", "--root", root.as_str(), "--force"],
        ];
        for argv in cases {
            let out = env.later(argv.iter().copied().chain(["--json"]));
            assert_eq!(out.code, Some(2), "{argv:?}: {}", out.stderr);
            assert_eq!(out.stdout, "", "{argv:?}");
            assert!(
                !out.stderr.contains("unrecognized subcommand"),
                "{argv:?}: {}",
                out.stderr
            );
            snapshot_is_unchanged(&env, &before, &format!("{argv:?}"));
        }
    }

    // ---- row 11: HASH_MISMATCH ------------------------------------------------------------------------

    #[test]
    fn hash_mismatch_json_envelope() {
        let env = edited();
        let before = env.state();
        let out = env.later(["--json", "relink", R1, env.p("moved/renamed.txt").as_str()]);
        assert_eq!(out.code, Some(1), "{}", out.stderr);
        assert_eq!(out.stdout, "");
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!("cli_relink__hash_mismatch_json_envelope", env.clean(&out.stderr));
        });
        snapshot_is_unchanged(&env, &before, "HASH_MISMATCH");
    }

    #[test]
    fn same_size_other_content_is_a_hash_mismatch_too() {
        let env = Env::moved();
        env.put_at("moved/renamed.txt", b"HELLO\n", NEW_SECS);
        let before = env.state();
        let v = failed(
            &env.later(["--json", "relink", R1, env.p("moved/renamed.txt").as_str()]),
            1,
            "HASH_MISMATCH",
        );
        assert_eq!(v["error"]["details"]["expected"], SHA_HELLO);
        assert_eq!(v["error"]["details"]["actual"], SHA_UPPER);
        // `--yes` confirms a `--force`, it does not force.
        failed(
            &env.later([
                "--json",
                "--yes",
                "relink",
                R1,
                env.p("moved/renamed.txt").as_str(),
            ]),
            1,
            "HASH_MISMATCH",
        );
        snapshot_is_unchanged(&env, &before, "HASH_MISMATCH");
    }

    // ---- row 12: --force and its confirmation -----------------------------------------------------------

    #[test]
    fn force_needs_a_terminal_or_yes_when_the_content_differs() {
        let env = edited();
        let moved = env.p("moved/renamed.txt");
        let before = env.state();
        let out = env.later(["--json", "relink", R1, moved.as_str(), "--force"]);
        let v = failed(&out, 1, "CONFIRMATION_REQUIRED");
        let e = &v["error"];
        assert_eq!(
            e["details"],
            json!({"command": "relink", "declined": false})
        );
        assert_eq!(
            e["message"],
            "relink needs confirmation, but stdin or stderr is not a terminal; nothing was written"
        );
        assert_eq!(e["suggested_fix"], FIX);
        assert_eq!(
            e["suggested_command"],
            format!("hali relink {R1} '{moved}' --force --yes")
        );
        snapshot_is_unchanged(&env, &before, "CONFIRMATION_REQUIRED");

        // The suggested command works, and records what the file is now.
        let out = env.later(["--json", "relink", R1, moved.as_str(), "--force", "--yes"]);
        let v = ok(&out);
        assert_eq!(v["refreshed"], json!(true));
        assert_eq!(v["resource"]["byte_size"], 12);
        assert_eq!(v["resource"]["modified"], "2026-10-22T10:00:00Z");
        assert_eq!(v["resource"]["path"], moved);
        assert!(env.resources_ttl().contains(SHA_WORLD));
        assert!(!env.resources_ttl().contains(SHA_HELLO));
        let found = ok(&env.run(["--json", "find", "--stale"]));
        assert_eq!(found["total"], 0, "the record agrees with the file");
    }

    /// A run that asked `prompt` on stderr and was declined: the error envelope follows the
    /// prompt (after a line break when no answer was typed).
    fn declined(out: &Out, prompt: &str) -> Value {
        let rest = out
            .stderr
            .strip_prefix(prompt)
            .unwrap_or_else(|| panic!("the prompt {prompt:?} first: {:?}", out.stderr));
        let rest = Out {
            code: out.code,
            stdout: out.stdout.clone(),
            stderr: rest.trim_start_matches('\n').to_string(),
        };
        failed(&rest, 1, "CONFIRMATION_REQUIRED")
    }

    #[test]
    fn force_asks_at_a_terminal() {
        let env = edited();
        let moved = env.p("moved/renamed.txt");
        let before = env.state();
        let prompt = format!("relink resource {R1} \"a\" {moved}? [y/N] ");

        let out = env.tty(b"n\n", ["--json", "relink", R1, moved.as_str(), "--force"]);
        let v = declined(&out, &prompt);
        assert_eq!(v["error"]["details"]["declined"], json!(true));
        assert_eq!(
            v["error"]["message"],
            "relink was not confirmed; nothing was written"
        );
        snapshot_is_unchanged(&env, &before, "a declined relink");

        for answer in [&b""[..], &b"\n"[..], &b"maybe\n"[..]] {
            let out = env.tty(answer, ["--json", "relink", R1, moved.as_str(), "--force"]);
            declined(&out, &prompt);
            snapshot_is_unchanged(&env, &before, "a declined relink");
        }

        let out = env.tty(
            b"YES\n",
            ["--json", "relink", R1, moved.as_str(), "--force"],
        );
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert!(out.stderr.contains(&prompt), "{:?}", out.stderr);
        assert!(out.stderr.ends_with("[y/N] "), "{:?}", out.stderr);
        assert_eq!(json(&out.stdout)["refreshed"], json!(true));
    }

    #[test]
    fn force_on_the_same_content_never_asks() {
        let env = Env::moved();
        let moved = env.p("moved/renamed.txt");
        let out = env.later(["--json", "relink", R1, moved.as_str(), "--force"]);
        let v = ok(&out);
        assert_eq!(v["refreshed"], json!(false));
        assert_eq!(v["resource"]["path"], moved);
        // Not even at a terminal.
        let env = Env::moved();
        let moved = env.p("moved/renamed.txt");
        let out = env.tty(b"", ["--json", "relink", R1, moved.as_str(), "--force"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert!(!out.stderr.contains("[y/N]"), "{:?}", out.stderr);
    }

    #[test]
    fn force_dry_run_never_asks() {
        let env = edited();
        let moved = env.p("moved/renamed.txt");
        let before = env.state();
        let out = env.tty(
            b"",
            [
                "--json",
                "--dry-run",
                "relink",
                R1,
                moved.as_str(),
                "--force",
            ],
        );
        let v = ok(&out);
        assert_eq!(v["dry_run"], json!(true));
        assert_eq!(v["refreshed"], json!(true));
        snapshot_is_unchanged(&env, &before, "a dry run");
        let out = env.later([
            "--json",
            "--dry-run",
            "relink",
            R1,
            moved.as_str(),
            "--force",
        ]);
        assert_eq!(
            out.code,
            Some(0),
            "never CONFIRMATION_REQUIRED: {}",
            out.stderr
        );
    }

    #[test]
    fn domain_errors_come_before_the_confirmation() {
        let env = edited();
        let before = env.state();
        let v = failed(
            &env.later([
                "--json",
                "relink",
                "0192f0c4-0000-7000-8000-000000000999",
                env.p("moved/renamed.txt").as_str(),
                "--force",
            ]),
            1,
            "UNKNOWN_RESOURCE",
        );
        assert_eq!(
            v["error"]["details"]["ref"],
            "0192f0c4-0000-7000-8000-000000000999"
        );
        let v = failed(
            &env.later(["--json", "relink", R1, env.p("moved").as_str(), "--force"]),
            1,
            "NOT_A_REGULAR_FILE",
        );
        assert_eq!(v["error"]["details"]["kind"], "directory");
        snapshot_is_unchanged(&env, &before, "refused relinks");
    }

    #[test]
    fn a_search_never_asks_and_ignores_yes() {
        let env = Env::moved();
        let before = env.state();
        // At a terminal that would answer nothing: a prompt would show on stderr.
        let out = env.tty(
            b"",
            [
                "--json",
                "--dry-run",
                "relink",
                "--root",
                env.root_str().as_str(),
            ],
        );
        assert_eq!(out.stderr, "");
        snapshot_is_unchanged(&env, &before, "a dry run");
        let out = env.tty(
            b"",
            [
                "--json",
                "--yes",
                "relink",
                "--root",
                env.root_str().as_str(),
            ],
        );
        let v = ok(&out);
        assert_eq!(v["counts"]["relinked"], 1);
        let env = Env::moved();
        let out = env.tty(b"", ["--json", "relink", "--root", env.root_str().as_str()]);
        let v = ok(&out);
        assert_eq!(v["counts"]["relinked"], 1);
    }

    // ---- row 14-17: POSSIBLE_MOVE on add -------------------------------------------------------------------

    #[test]
    fn possible_move_json_envelope() {
        let env = Env::moved();
        let before = env.state();
        let out = env.run(["--json", "add", env.p("moved/renamed.txt").as_str()]);
        assert_eq!(out.code, Some(1), "{}", out.stderr);
        assert_eq!(out.stdout, "");
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!("cli_relink__possible_move_json_envelope", env.clean(&out.stderr));
        });
        snapshot_is_unchanged(&env, &before, "POSSIBLE_MOVE");
    }

    #[test]
    fn possible_move_text_suggests_relink() {
        let env = Env::moved();
        let before = env.state();
        let moved = env.p("moved/renamed.txt");
        let out = env.run(["add", moved.as_str()]);
        assert_eq!(out.code, Some(1), "{}", out.stderr);
        assert_eq!(out.stdout, "");
        assert!(out.stderr.starts_with("error: "), "{:?}", out.stderr);
        assert!(out.stderr.contains(R1), "{:?}", out.stderr);
        assert!(
            out.stderr
                .contains(&format!("Try: hali relink {R1} '{moved}'")),
            "{:?}",
            out.stderr
        );
        snapshot_is_unchanged(&env, &before, "POSSIBLE_MOVE");
    }

    #[test]
    fn a_dry_run_add_reports_the_same_error() {
        let env = Env::moved();
        let moved = env.p("moved/renamed.txt");
        let real = env.run(["--json", "add", moved.as_str()]);
        let dry = env.run(["--json", "--dry-run", "add", moved.as_str()]);
        assert_eq!(dry.code, real.code);
        assert_eq!(dry.stderr, real.stderr);
        assert_eq!(dry.stdout, "");
        failed(&dry, 1, "POSSIBLE_MOVE");
    }

    #[test]
    fn add_new_overrides_the_refusal() {
        let env = Env::moved();
        let moved = env.p("moved/renamed.txt");
        let out = env.run_env(
            &[("HALI_TEST_ID_SEED", "2")],
            ["--json", "add", "--new", moved.as_str()],
        );
        let v = ok(&out);
        assert_eq!(v["added"].as_array().unwrap().len(), 1);
        assert_ne!(v["added"][0]["id"], R1);
        assert_eq!(v["added"][0]["path"], moved);
        // R1 is still there, still Missing.
        let found = ok(&env.run(["--json", "find", "--missing"]));
        assert_eq!(found["total"], 1);
        assert_eq!(found["results"][0]["id"], R1);
        // And now that path is cataloged, adding it again is a re-add.
        let again = ok(&env.run(["--json", "add", moved.as_str()]));
        assert_eq!(again["added"], json!([]));
        assert_eq!(again["existing"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn a_multi_path_add_is_all_or_nothing() {
        let env = Env::moved();
        let fresh = env.put_at("fresh.txt", b"something new\n", PINNED_SECS);
        let moved = env.p("moved/renamed.txt");
        let before = env.state();
        for paths in [
            [fresh.to_str().unwrap(), moved.as_str()],
            [moved.as_str(), fresh.to_str().unwrap()],
        ] {
            let mut argv = vec!["--json", "add"];
            argv.extend(paths);
            let v = failed(&env.run(argv), 1, "POSSIBLE_MOVE");
            assert_eq!(v["error"]["details"]["path"], moved);
            snapshot_is_unchanged(&env, &before, "an add that was refused");
        }
        // With --new both are added.
        let v = ok(&env.run_env(
            &[("HALI_TEST_ID_SEED", "3")],
            [
                "--json",
                "add",
                "--new",
                fresh.to_str().unwrap(),
                moved.as_str(),
            ],
        ));
        assert_eq!(v["added"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn the_first_offender_in_argument_order_is_named() {
        let env = Env::moved();
        env.put_at("moved/second.txt", b"hello\n", NEW_SECS);
        let first = env.p("moved/renamed.txt");
        let second = env.p("moved/second.txt");
        let v = failed(
            &env.run(["--json", "add", second.as_str(), first.as_str()]),
            1,
            "POSSIBLE_MOVE",
        );
        assert_eq!(v["error"]["details"]["path"], second);
    }

    #[test]
    fn possible_move_comes_before_unknown_type_and_concept() {
        let env = Env::moved();
        let moved = env.p("moved/renamed.txt");
        let before = env.state();
        for flag in [["--type", "Nope"], ["--tag", "nope"]] {
            let v = failed(
                &env.run(["--json", "add", moved.as_str(), flag[0], flag[1]]),
                1,
                "POSSIBLE_MOVE",
            );
            assert_eq!(v["error"]["details"]["path"], moved);
        }
        snapshot_is_unchanged(&env, &before, "refused adds");
        // With --new the same references are the error.
        failed(
            &env.run(["--json", "add", "--new", moved.as_str(), "--type", "Nope"]),
            1,
            "UNKNOWN_TYPE",
        );
        failed(
            &env.run(["--json", "add", "--new", moved.as_str(), "--tag", "nope"]),
            1,
            "UNKNOWN_CONCEPT",
        );
    }

    #[test]
    fn what_is_not_a_move_is_added() {
        let env = Env::moved();
        // Contrast: the moved file itself is refused.
        failed(
            &env.run(["--json", "add", env.p("moved/renamed.txt").as_str()]),
            1,
            "POSSIBLE_MOVE",
        );
        // Same size, other content.
        let other = env.put_at("same-size.txt", b"HELLO\n", PINNED_SECS);
        ok(&env.run_env(
            &[("HALI_TEST_ID_SEED", "2")],
            ["--json", "add", other.to_str().unwrap()],
        ));
        // Same content as a Resource whose file is there: a duplicate, allowed.
        let a = env.add("b.txt", b"dup\n", PINNED_SECS, "3");
        assert!(!a.is_empty());
        let copy = env.put_at("b-copy.txt", b"dup\n", PINNED_SECS);
        let v = ok(&env.run_env(
            &[("HALI_TEST_ID_SEED", "4")],
            ["--json", "add", copy.to_str().unwrap()],
        ));
        assert_eq!(v["added"].as_array().unwrap().len(), 1);
        // A re-add of a cataloged path is idempotent.
        let v = ok(&env.run(["--json", "add", copy.to_str().unwrap()]));
        assert_eq!(v["existing"].as_array().unwrap().len(), 1);
    }

    // ---- row 24: empty files --------------------------------------------------------------------------------

    #[test]
    fn empty_files_are_never_a_move() {
        let env = Env::moved();
        let empty = env.add("e.txt", b"", PINNED_SECS, "2");
        std::fs::remove_file(env.root().join("e.txt")).unwrap();
        let other = env.put_at("e2.txt", b"", NEW_SECS);
        // Contrast: the moved file with content is refused.
        failed(
            &env.run(["--json", "add", env.p("moved/renamed.txt").as_str()]),
            1,
            "POSSIBLE_MOVE",
        );
        // An empty file has the hash of every empty file: it is no sign of a move.
        let v = ok(&env.run_env(
            &[("HALI_TEST_ID_SEED", "3")],
            ["--json", "add", other.to_str().unwrap()],
        ));
        assert_eq!(v["added"].as_array().unwrap().len(), 1);
        // And a search leaves the empty Resource alone, though empty files are there.
        let v = ok(&env.later(["--json", "relink", "--root", env.root_str().as_str()]));
        let listed: Vec<&str> = v["relinked"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["resource"]["id"].as_str().unwrap())
            .chain(
                v["ambiguous"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|e| e["resource"]["id"].as_str().unwrap()),
            )
            .collect();
        assert_eq!(listed, [R1], "only R1 is repaired, not {empty}");
    }

    // ---- row 18: default roots ----------------------------------------------------------------------------

    fn search_defaults(env: &Env) -> Value {
        let out = env.later(["--json", "relink"]);
        ok(&out)
    }

    #[test]
    fn files_dir_from_the_config_is_searched() {
        let env = Env::new();
        let docs = env.base.join("docs");
        env.add("a.txt", b"hello\n", PINNED_SECS, "1");
        env.mv_abs("a.txt", &docs.join("x.txt"), NEW_SECS);
        env.config(&format!("[files]\ndir = \"{}\"\n", docs.display()));
        let v = search_defaults(&env);
        assert_eq!(v["roots"], json!([docs.to_str().unwrap()]));
        assert_eq!(v["counts"]["relinked"], 1);
        assert_eq!(
            v["relinked"][0]["resource"]["path"],
            docs.join("x.txt").to_str().unwrap()
        );
    }

    #[test]
    fn a_tilde_in_files_dir_is_the_home_dir() {
        let env = Env::new();
        env.add("a.txt", b"hello\n", PINNED_SECS, "1");
        let target = env.home().join("Docs2/x.txt");
        env.mv_abs("a.txt", &target, NEW_SECS);
        env.config("[files]\ndir = \"~/Docs2\"\n");
        let v = search_defaults(&env);
        assert_eq!(
            v["roots"],
            json!([env.home().join("Docs2").to_str().unwrap()])
        );
        assert_eq!(v["counts"]["relinked"], 1);
    }

    #[test]
    fn without_a_config_files_dir_is_documents_hali() {
        let env = Env::new();
        env.add("a.txt", b"hello\n", PINNED_SECS, "1");
        let dir = env.home().join("Documents/hali");
        env.mv_abs("a.txt", &dir.join("x.txt"), NEW_SECS);
        let v = search_defaults(&env);
        assert_eq!(v["roots"], json!([dir.to_str().unwrap()]));
        assert_eq!(v["counts"]["relinked"], 1);
    }

    #[test]
    fn user_dirs_names_the_documents_dir() {
        // `XDG_DOCUMENTS_DIR` of user-dirs.dirs, with $HOME or an absolute path.
        for (line, dir) in [
            ("XDG_DOCUMENTS_DIR=\"$HOME/Docs\"", "home/Docs/hali"),
            ("XDG_DOCUMENTS_DIR=\"@BASE@/elsewhere\"", "elsewhere/hali"),
        ] {
            let env = Env::new();
            env.add("a.txt", b"hello\n", PINNED_SECS, "1");
            let dir = env.base.join(dir);
            env.mv_abs("a.txt", &dir.join("x.txt"), NEW_SECS);
            // A decoy where the default would be.
            env.put_abs(
                &env.home().join("Documents/hali/decoy.txt"),
                b"hello\n",
                NEW_SECS,
            );
            std::fs::write(
                env.base.join("cfg/user-dirs.dirs"),
                format!(
                    "# user dirs\nXDG_DESKTOP_DIR=\"$HOME/Desktop\"\n{}\n",
                    line.replace("@BASE@", env.base.to_str().unwrap())
                ),
            )
            .unwrap();
            let v = search_defaults(&env);
            assert_eq!(v["roots"], json!([dir.to_str().unwrap()]), "{line}");
            assert_eq!(
                v["relinked"][0]["resource"]["path"],
                dir.join("x.txt").to_str().unwrap()
            );
        }
    }

    #[test]
    fn the_config_beats_user_dirs_and_the_default() {
        let env = Env::new();
        env.add("a.txt", b"hello\n", PINNED_SECS, "1");
        let configured = env.base.join("configured");
        std::fs::create_dir_all(&configured).unwrap();
        env.mv_abs("a.txt", &env.home().join("Documents/hali/x.txt"), NEW_SECS);
        env.config(&format!("[files]\ndir = \"{}\"\n", configured.display()));
        std::fs::write(
            env.base.join("cfg/user-dirs.dirs"),
            "XDG_DOCUMENTS_DIR=\"$HOME/Docs\"\n",
        )
        .unwrap();
        let v = search_defaults(&env);
        assert_eq!(v["roots"], json!([configured.to_str().unwrap()]));
        assert_eq!(
            v["counts"],
            json!({"relinked": 0, "ambiguous": 0, "unmatched": 1})
        );
    }

    #[test]
    fn explicit_roots_replace_the_defaults() {
        let env = Env::new();
        env.add("a.txt", b"hello\n", PINNED_SECS, "1");
        let docs = env.home().join("Documents/hali");
        env.mv_abs("a.txt", &docs.join("x.txt"), NEW_SECS);
        let elsewhere = env.base.join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        let v = ok(&env.later(["--json", "relink", "--root", elsewhere.to_str().unwrap()]));
        assert_eq!(v["roots"], json!([elsewhere.to_str().unwrap()]));
        assert_eq!(v["counts"]["unmatched"], 1);
        assert_eq!(v["counts"]["relinked"], 0);
    }

    #[test]
    fn an_unusable_default_root_is_skipped_silently() {
        // Nothing at ~/Documents/hali.
        let env = Env::moved();
        let before = env.state();
        let out = env.later(["--json", "relink"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        let v = json(&out.stdout);
        assert_eq!(v["roots"], json!([]));
        assert_eq!(
            v["counts"],
            json!({"relinked": 0, "ambiguous": 0, "unmatched": 1})
        );
        snapshot_is_unchanged(&env, &before, "a search with no roots");

        // A configured dir that is not there, or is a file: skipped, and the chain stops at it.
        for setup in ["missing", "file"] {
            let env = Env::new();
            env.add("a.txt", b"hello\n", PINNED_SECS, "1");
            env.mv_abs("a.txt", &env.home().join("Documents/hali/x.txt"), NEW_SECS);
            let dir = env.base.join("configured");
            if setup == "file" {
                std::fs::write(&dir, b"not a directory").unwrap();
            }
            env.config(&format!("[files]\ndir = \"{}\"\n", dir.display()));
            let out = env.later(["--json", "relink"]);
            assert_eq!(out.code, Some(0), "{setup}: {}", out.stderr);
            assert_eq!(out.stderr, "", "{setup}");
            let v = json(&out.stdout);
            assert_eq!(v["roots"], json!([]), "{setup}");
            assert_eq!(v["counts"]["relinked"], 0, "{setup}");
        }
    }

    #[test]
    fn a_files_dir_of_the_wrong_type_is_config_invalid() {
        let env = Env::moved();
        env.config("[files]\ndir = 5\n");
        let out = env.later(["--json", "relink"]);
        failed(&out, 3, "CONFIG_INVALID");
    }

    // ---- row 23: text mode -----------------------------------------------------------------------------------

    #[test]
    fn relink_text_names_old_and_new_path() {
        let env = Env::moved();
        let out = env.later(["relink", "--root", env.root_str().as_str()]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        let line = out
            .stdout
            .lines()
            .find(|l| l.contains("relinked"))
            .unwrap_or_else(|| panic!("a relinked line: {:?}", out.stdout));
        assert!(line.contains(R1), "{line}");
        assert!(line.contains(&env.p("a.txt")), "{line}");
        assert!(line.contains(&env.p("moved/renamed.txt")), "{line}");

        let env = Env::moved();
        env.put_at("moved/copy.txt", b"hello\n", NEW_SECS);
        let out = env.later(["relink", "--root", env.root_str().as_str()]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        let line = out
            .stdout
            .lines()
            .find(|l| l.contains("ambiguous"))
            .unwrap_or_else(|| panic!("an ambiguous line: {:?}", out.stdout));
        assert!(line.contains(R1), "{line}");

        let env = Env::moved();
        let out = env.later(["--dry-run", "relink", "--root", env.root_str().as_str()]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert!(
            out.stderr.contains("dry run; nothing was written"),
            "{}",
            out.stderr
        );
    }

    #[test]
    fn text_output_is_escaped_for_the_terminal() {
        let env = Env::new();
        env.add("a.txt", b"hello\n", PINNED_SECS, "1");
        env.mv("a.txt", "moved/\u{1b}[31mred.txt", NEW_SECS);
        let out = env.later(["relink", "--root", env.root_str().as_str()]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert!(!out.stdout.contains('\u{1b}'), "{:?}", out.stdout);
        assert!(
            out.stdout.contains("\\u{1b}[31mred.txt"),
            "{:?}",
            out.stdout
        );
        // JSON is never escaped this way.
        let env = Env::new();
        env.add("a.txt", b"hello\n", PINNED_SECS, "1");
        env.mv("a.txt", "moved/\u{1b}[31mred.txt", NEW_SECS);
        let v = ok(&env.later(["--json", "relink", "--root", env.root_str().as_str()]));
        assert_eq!(
            v["relinked"][0]["resource"]["path"],
            env.p("moved/\u{1b}[31mred.txt")
        );
    }

    // ---- row 19: the schema list ----------------------------------------------------------------------------------

    #[test]
    fn schema_list_json() {
        let env = Env::new();
        let out = env.run(["schema", "--json"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!("cli_relink__schema_list_json", out.stdout);
        });
    }

    #[test]
    fn relink_has_a_schema() {
        let env = Env::new();
        let out = env.run(["schema", "relink"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        let schema: Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(schema["title"], "hali relink --json");
    }
}
