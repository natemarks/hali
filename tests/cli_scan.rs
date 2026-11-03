//! Goal tests for `hali scan` through the real binary (#22, spec §7, §9.1 **Scan:**, §9.4, §9.5).
//! Locked golden snapshots sit in tests/snapshots/ (hand-written from the issue's contract; the
//! ids and triples are those `hali add` writes for the same files, which scan must match):
//! `cli_scan__scan_json`, `cli_dry_run__scan_dry_run_json` and `cli_scan__schema_list_json`.
//!
//! Isolation (the walker honours ignore files in the root's parents and the user's git
//! configuration, so a test must not see the host's):
//! - the tree is built under the system temp dir, never under `target/`, whose parent is the
//!   checkout with its own `.gitignore` and `.git`; the data dir is a sibling of the root;
//! - `HOME`, `XDG_CONFIG_HOME`, `GIT_CONFIG_GLOBAL` and `GIT_CONFIG_SYSTEM` point at temp paths,
//!   the rest of the environment is cleared;
//! - every test asserts that no ancestor of the temp dir holds a `.gitignore`, `.ignore`,
//!   `.haliignore`, `.git` or `.jj`;
//! - a socket fixture uses a short path (Unix socket paths are limited to 108 bytes).
//!
//! Every run pins `HALI_TEST_NOW` and `HALI_TEST_ID_SEED=1`; the ids that seed gives are R1/C1/P
//! for the first new file (Resource, catalog record, user Person) and R2/C2 for the second, as in
//! tests/cli_resource.rs.
//!
//! Wording the contract leaves open and these tests fix (change them, with the human, if the
//! wording should differ):
//! - usage errors name what is wrong: `--max-size`, `--exclude` and `invalid media type pattern`
//!   (find's text) appear in stderr; clap's own text covers a missing or extra root;
//! - the last progress update always reports the totals: `scanning: <seen> files, <hashed>
//!   hashed`; progress shows only when `HALI_TEST_TTY=1` (stderr is a terminal in a release
//!   build) and `--json` is off;
//! - in text mode, `truncated<TAB>true` is the last line of a real run's output;
//! - `--exclude` / `--max-size` / `--media-type` are given as `--flag=value` where the value may
//!   start with `-` or be empty.
//!
//! The `hali schema` list is covered here too (older tests assert the old list and are not
//! goal-owned): the implementer must also update `NAMES` and the text list in
//! tests/cli_schema.rs, the name list of tests/core_schema.rs, the hardcoded 24 in
//! tests/cli_relation.rs and tests/cli_type.rs, the exact lists in tests/cli_refresh.rs,
//! tests/cli_describe.rs, tests/cli_find.rs, tests/cli_forget.rs and
//! tests/cli_concept_manage.rs, the snapshot tests/snapshots/cli_schema__schema_list_json.snap,
//! the `[user] name and [vocabulary] language` fix text in tests/cli_error_text.rs, the help
//! text snapshots that list subcommands, and the `scan` row of tests/dry_run_guard.rs.

#[cfg(feature = "goal")]
mod support;

#[cfg(feature = "goal")]
mod goal {
    use assert_cmd::Command;
    use serde_json::Value;
    use std::collections::BTreeMap;
    use std::ffi::{CString, OsStr, OsString};
    use std::io::Read;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::path::{Path, PathBuf};
    use std::process::Stdio;
    use std::time::{Duration, Instant, SystemTime};
    use tempfile::TempDir;

    use super::support;

    const NOW: &str = "2026-10-24T09:30:00Z";
    const PINNED_SECS: u64 = 1_792_504_931; // 2026-10-20T14:02:11Z
    const R1: &str = "018bcfe5-6801-710a-adec-89025cc1beeb";
    const R2: &str = "018bcfe5-6804-7099-ac6c-d7363ca585e7";

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

    /// A directory, restored to mode 0755 / a file to 0644 on drop so the temp dir can go.
    struct Restore(Vec<PathBuf>);

    impl Drop for Restore {
        fn drop(&mut self) {
            for p in &self.0 {
                let mode = if p.is_dir() { 0o755 } else { 0o644 };
                let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode));
            }
        }
    }

    fn privileged() -> bool {
        // SAFETY: geteuid has no preconditions.
        unsafe { libc::geteuid() == 0 }
    }

    fn mkfifo(path: &Path) {
        let c = CString::new(path.as_os_str().as_bytes()).unwrap();
        // SAFETY: `c` is a valid NUL-terminated path.
        let rc = unsafe { libc::mkfifo(c.as_ptr(), 0o644) };
        assert_eq!(rc, 0, "mkfifo {}", path.display());
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
                        "{} exists: the host's ignore rules would reach the scan root; point TMPDIR elsewhere",
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

        fn root(&self) -> PathBuf {
            self.base.join("root")
        }

        fn root_str(&self) -> String {
            self.root().to_str().unwrap().to_string()
        }

        fn data(&self) -> PathBuf {
            self.data.clone()
        }

        /// Writes `root/<rel>` (creating parents), mtime pinned.
        fn put(&self, rel: &str, bytes: &[u8]) -> PathBuf {
            self.put_at(rel, bytes, PINNED_SECS)
        }

        fn put_at(&self, rel: &str, bytes: &[u8], secs: u64) -> PathBuf {
            let path = self.root().join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, bytes).unwrap();
            pin(&path, secs);
            path
        }

        /// `<base>/cfg/hali/config.toml`.
        fn config(&self, text: &str) {
            let dir = self.base.join("cfg/hali");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("config.toml"), text).unwrap();
        }

        fn vars(&self, extra: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
            let mut vars: Vec<(OsString, OsString)> = vec![
                ("HOME".into(), self.base.join("home").into()),
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
            timeout: u64,
            args: I,
        ) -> Out
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            let mut cmd = Command::cargo_bin("hali").unwrap();
            cmd.env_clear()
                .envs(self.vars(extra))
                .timeout(Duration::from_secs(timeout))
                .write_stdin(Vec::new());
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
            self.exec(None, &[], 60, args)
        }

        fn run_env<I, S>(&self, extra: &[(&str, &str)], args: I) -> Out
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            self.exec(None, extra, 60, args)
        }

        /// The output with the temp dir shown as `/TMP`.
        fn clean(&self, text: &str) -> String {
            text.replace(self.base.to_str().unwrap(), "/TMP")
        }

        /// Spawns the binary with stdin a pipe held open by the caller.
        fn spawn(&self, extra: &[(&str, &str)], args: &[&str]) -> std::process::Child {
            std::process::Command::new(env!("CARGO_BIN_EXE_hali"))
                .env_clear()
                .envs(self.vars(extra))
                .args(args)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        }
    }

    /// Names -> bytes of the files in a data dir; `None` when it does not exist.
    fn listing(dir: &Path) -> Option<BTreeMap<String, Vec<u8>>> {
        if !dir.exists() {
            return None;
        }
        Some(
            std::fs::read_dir(dir)
                .unwrap()
                .map(|e| {
                    let e = e.unwrap();
                    let name = e.file_name().into_string().unwrap();
                    let bytes = if e.file_type().unwrap().is_dir() {
                        Vec::new()
                    } else {
                        std::fs::read(e.path()).unwrap()
                    };
                    (name, bytes)
                })
                .collect(),
        )
    }

    /// Kind, size, mtime and content (or link target) of every entry under `root`.
    type Print = BTreeMap<String, (String, u64, SystemTime, Vec<u8>)>;

    fn tree_print(root: &Path) -> Print {
        fn walk(dir: &Path, root: &Path, out: &mut Print) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                let meta = std::fs::symlink_metadata(&path).unwrap();
                let name = path.strip_prefix(root).unwrap().display().to_string();
                let (kind, bytes) = if meta.is_dir() {
                    ("dir", Vec::new())
                } else if meta.file_type().is_symlink() {
                    (
                        "symlink",
                        std::fs::read_link(&path)
                            .unwrap()
                            .as_os_str()
                            .as_bytes()
                            .to_vec(),
                    )
                } else if meta.is_file() {
                    ("file", std::fs::read(&path).unwrap())
                } else {
                    ("other", Vec::new())
                };
                out.insert(
                    name,
                    (
                        kind.to_string(),
                        meta.len(),
                        meta.modified().unwrap(),
                        bytes,
                    ),
                );
                if meta.is_dir() {
                    walk(&path, root, out);
                }
            }
        }
        let mut out = Print::new();
        walk(root, root, &mut out);
        out
    }

    /// Asserts exit 0, empty stderr and one compact JSON line; returns it parsed.
    fn ok_json(out: &Out) -> Value {
        assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
        assert_eq!(out.stderr, "");
        assert_eq!(out.stdout.matches('\n').count(), 1, "{:?}", out.stdout);
        serde_json::from_str(&out.stdout)
            .unwrap_or_else(|e| panic!("not JSON ({e}): {:?}", out.stdout))
    }

    fn record_paths(list: &Value) -> Vec<String> {
        list.as_array()
            .unwrap()
            .iter()
            .map(|r| r["path"].as_str().unwrap().to_string())
            .collect()
    }

    fn skips(v: &Value) -> Vec<(String, String)> {
        v["skipped"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| {
                (
                    s["path"].as_str().unwrap().to_string(),
                    s["reason"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    }

    fn counts(v: &Value) -> [u64; 5] {
        let c = &v["counts"];
        ["added", "refreshed", "unchanged", "missing", "skipped"].map(|k| c[k].as_u64().unwrap())
    }

    fn copy_fixture(name: &str, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        let from = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
        }
    }

    fn garbage_store(dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("resources.ttl"), "this is { not turtle\n").unwrap();
    }

    // ---- criterion 1: usage errors, before the store is read ----------------------------------

    #[test]
    fn usage_errors_before_store() {
        let env = Env::new();
        env.put("a.txt", b"x\n");
        garbage_store(&env.data());
        let before = listing(&env.data());
        let root = env.root_str();
        let other = env.base.join("home").to_str().unwrap().to_string();

        let mut cases: Vec<(Vec<String>, String)> = vec![
            (
                vec!["scan".into()],
                "the following required arguments were not provided:\n  <ROOT>".into(),
            ),
            (
                vec!["scan".into(), root.clone(), other.clone()],
                "unexpected argument".into(),
            ),
            (
                vec!["scan".into(), root.clone(), "--prune".into()],
                "unexpected argument '--prune'".into(),
            ),
            (
                vec!["scan".into(), root.clone(), "--list".into()],
                "unexpected argument '--list'".into(),
            ),
            (
                vec!["scan".into(), "--forget-root".into(), root.clone()],
                "unexpected argument '--forget-root'".into(),
            ),
            (
                vec!["scan".into(), root.clone(), "--max-size".into()],
                "--max-size".into(),
            ),
        ];
        for bad in [
            "",
            "1.5GiB",
            "1GB",
            "1K",
            "-1",
            "+1",
            "GiB",
            "1 GiB",
            "abc",
            "99999999999999999999",
            "17179869184GiB",
        ] {
            cases.push((
                vec!["scan".into(), root.clone(), format!("--max-size={bad}")],
                "--max-size".into(),
            ));
        }
        for bad in ["text", "text/", "/plain", "te xt/plain", "text/*x y", ""] {
            cases.push((
                vec!["scan".into(), root.clone(), format!("--media-type={bad}")],
                "invalid media type pattern".into(),
            ));
        }
        for bad in ["!x", "", "   ", "[", "!"] {
            cases.push((
                vec!["scan".into(), root.clone(), format!("--exclude={bad}")],
                "--exclude".into(),
            ));
        }
        for (args, want) in &cases {
            let out = env.run(args);
            assert_eq!(out.code, Some(2), "{args:?}: {}", out.stderr);
            assert!(
                out.stderr.contains(want.as_str()),
                "{args:?}: {:?}",
                out.stderr
            );
            assert_eq!(out.stdout, "", "{args:?}");
            assert_eq!(
                listing(&env.data()),
                before,
                "{args:?}: the store was touched"
            );
        }
        // The store really is unreadable: with valid arguments scan reads it and fails.
        let out = env.run(["scan", root.as_str()]);
        assert_eq!(out.code, Some(3), "{}", out.stderr);
    }

    #[test]
    fn errors_come_in_the_documented_order() {
        // clap, config (exit 3), exclude globs (usage), root (usage), store (exit 3).
        let env = Env::new();
        let missing = env.base.join("missing");
        let missing = missing.to_str().unwrap();
        garbage_store(&env.data());

        env.config("[scan]\nmax_size = 5\n");
        let out = env.run(["scan", missing, "--exclude=!x"]);
        assert_eq!(out.code, Some(3), "{}", out.stderr);
        assert!(out.stderr.contains("invalid config file"), "{}", out.stderr);

        env.config("");
        let out = env.run(["scan", missing, "--exclude=!x"]);
        assert_eq!(out.code, Some(2), "{}", out.stderr);
        assert!(out.stderr.contains("--exclude"), "{}", out.stderr);
        assert!(!out.stderr.contains("is not a directory"), "{}", out.stderr);

        let out = env.run(["scan", missing]);
        assert_eq!(out.code, Some(2), "{}", out.stderr);
        assert!(out.stderr.contains("is not a directory"), "{}", out.stderr);

        let out = env.run(["scan", env.root_str().as_str()]);
        assert_eq!(out.code, Some(3), "{}", out.stderr);
    }

    #[test]
    fn config_invalid_has_the_new_fix_text() {
        for text in [
            "[scan]\nmax_size = 5\n",
            "[scan]\nexclude = \"*.tmp\"\n",
            "[scan]\ninclude_hidden = \"yes\"\n",
            "[scan]\nmax_size = \"1GB\"\n",
        ] {
            let env = Env::new();
            env.config(text);
            let out = env.run(["scan", env.root_str().as_str()]);
            assert_eq!(out.code, Some(3), "{text:?}: {}", out.stderr);
            assert!(
                out.stderr.contains(
                    "Fix the config file; only [user] name, [vocabulary] language and [scan] exclude, include_hidden and max_size are read."
                ),
                "{text:?}: {}",
                out.stderr
            );
            assert!(!env.data().exists());
        }
    }

    // ---- criterion 2: the root -----------------------------------------------------------------

    #[test]
    fn root_resolution() {
        let real = |env: &Env| env.root_str();
        for what in [
            "absolute",
            "relative",
            "dots and a slash",
            "absolute symlink",
            "relative symlink",
        ] {
            let env = Env::new();
            env.put("a.txt", b"hello\n");
            let (arg, cwd): (String, Option<PathBuf>) = match what {
                "absolute" => (env.root_str(), None),
                "relative" => ("root".to_string(), Some(env.base.clone())),
                "dots and a slash" => ("./root/../root/".to_string(), Some(env.base.clone())),
                "absolute symlink" => {
                    symlink(env.root(), env.base.join("link")).unwrap();
                    (env.base.join("link").to_str().unwrap().to_string(), None)
                }
                _ => {
                    symlink("root", env.base.join("link")).unwrap();
                    ("link".to_string(), Some(env.base.clone()))
                }
            };
            let out = env.exec(cwd.as_deref(), &[], 60, ["--json", "scan", arg.as_str()]);
            let v = ok_json(&out);
            assert_eq!(v["root"], real(&env), "{what}");
            assert_eq!(
                record_paths(&v["added"]),
                vec![format!("{}/a.txt", real(&env))],
                "{what}"
            );
        }
    }

    #[test]
    fn root_errors() {
        let env = Env::new();
        let base = env.base.to_str().unwrap().to_string();
        std::fs::write(env.base.join("file.txt"), "x").unwrap();
        symlink(env.base.join("file.txt"), env.base.join("filelink")).unwrap();

        let not_dir =
            |path: &str| format!("scan root {path} is not a directory; use hali add for a file");
        let cases: Vec<(&str, Option<&Path>, String)> = vec![
            ("missing", None, not_dir(&format!("{base}/missing"))),
            (
                "relative-missing",
                Some(env.base.as_path()),
                not_dir(&format!("{base}/relative-missing")),
            ),
            (
                "file.txt",
                Some(env.base.as_path()),
                not_dir(&format!("{base}/file.txt")),
            ),
            (
                "filelink",
                Some(env.base.as_path()),
                not_dir(&format!("{base}/file.txt")),
            ),
        ];
        for (arg, cwd, want) in cases {
            let arg = if cwd.is_none() {
                format!("{base}/{arg}")
            } else {
                arg.to_string()
            };
            let out = env.exec(cwd, &[], 60, ["scan", arg.as_str()]);
            assert_eq!(out.code, Some(2), "{arg}: {}", out.stderr);
            assert!(
                out.stderr.contains(&want),
                "{arg}: {:?} lacks {want:?}",
                out.stderr
            );
            assert_eq!(out.stdout, "", "{arg}");
            assert!(!env.data().exists(), "{arg}: nothing is written");
        }

        // A root that is not UTF-8.
        let bad = env.base.join(OsStr::from_bytes(b"bad\xffdir"));
        std::fs::create_dir(&bad).unwrap();
        let out = env.run([OsStr::new("--json"), OsStr::new("scan"), bad.as_os_str()]);
        assert_eq!(out.code, Some(1), "{}", out.stderr);
        let err: Value = serde_json::from_str(&out.stderr).unwrap();
        assert_eq!(err["error"]["rule"], "NON_UTF8_PATH");
        assert_eq!(out.stdout, "");
        assert!(!env.data().exists());

        // A root that cannot be read.
        let locked = env.base.join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0)).unwrap();
        let _restore = Restore(vec![locked.clone()]);
        if std::fs::read_dir(&locked).is_err() {
            let out = env.run(["--json", "scan", locked.to_str().unwrap()]);
            assert_eq!(out.code, Some(1), "{}", out.stderr);
            let err: Value = serde_json::from_str(&out.stderr).unwrap();
            assert_eq!(err["error"]["rule"], "FILE_UNREADABLE");
            assert_eq!(out.stdout, "");
            assert!(!env.data().exists());
        }
    }

    #[test]
    fn root_in_data_dir_refused() {
        let env = Env::new();
        let data = env.data();
        copy_fixture("store", &data);
        std::fs::create_dir_all(data.join("sub")).unwrap();
        symlink(data.join("sub"), env.base.join("sublink")).unwrap();
        let before = listing(&data);
        let d = data.to_str().unwrap().to_string();
        let given = [
            (d.clone(), d.clone()),
            (format!("{d}/"), d.clone()),
            (format!("{d}/sub"), format!("{d}/sub")),
            (format!("{d}/sub/.."), d.clone()),
            (
                env.base.join("sublink").to_str().unwrap().to_string(),
                format!("{d}/sub"),
            ),
        ];
        for dry in [false, true] {
            for (arg, canonical) in &given {
                let mut args = vec!["scan", arg.as_str()];
                if dry {
                    args.push("--dry-run");
                }
                let out = env.run(&args);
                assert_eq!(out.code, Some(2), "{args:?}: {}", out.stderr);
                assert!(
                    out.stderr.contains(&format!(
                        "scan root {canonical} is hali's data dir or inside it"
                    )),
                    "{args:?}: {}",
                    out.stderr
                );
                assert_eq!(out.stdout, "", "{args:?}");
                assert_eq!(listing(&data), before, "{args:?}");
            }
        }
        // The check comes before the store is read.
        std::fs::write(data.join("resources.ttl"), "not { turtle\n").unwrap();
        let out = env.run(["scan", data.join("sub").to_str().unwrap()]);
        assert_eq!(out.code, Some(2), "{}", out.stderr);
        assert!(
            out.stderr.contains("is hali's data dir or inside it"),
            "{}",
            out.stderr
        );
    }

    // ---- criterion 7: files the scan cannot or must not open ------------------------------------

    #[test]
    fn special_files_skipped() {
        let env = Env::new();
        let ok = env.put("ok.txt", b"ok\n");
        let fifo = env.root().join("fifo");
        mkfifo(&fifo);
        let sock = env.root().join("s.sock");
        let _listener = std::os::unix::net::UnixListener::bind(&sock).unwrap();
        // A FIFO opened for reading would block forever: the 30 s deadline is the detector.
        let out = env.exec(None, &[], 30, ["--json", "scan", env.root_str().as_str()]);
        let v = ok_json(&out);
        assert_eq!(
            skips(&v),
            vec![
                (
                    fifo.to_str().unwrap().to_string(),
                    "not_regular".to_string()
                ),
                (
                    sock.to_str().unwrap().to_string(),
                    "not_regular".to_string()
                ),
            ]
        );
        assert_eq!(record_paths(&v["added"]), vec![ok.to_str().unwrap()]);
        assert_eq!(counts(&v), [1, 0, 0, 0, 2]);
    }

    #[test]
    fn unreadable_skipped_scan_continues() {
        let env = Env::new();
        let ok = env.put("ok.txt", b"ok\n");
        let secret = env.put("secret.txt", b"secret\n");
        env.put("locked/inner.txt", b"inner\n");
        let locked = env.root().join("locked");
        std::fs::set_permissions(&secret, std::fs::Permissions::from_mode(0)).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0)).unwrap();
        let _restore = Restore(vec![secret.clone(), locked.clone()]);
        if std::fs::File::open(&secret).is_ok() {
            eprintln!("skipped: permission bits do not apply to this user");
            return;
        }
        let out = env.run(["--json", "scan", env.root_str().as_str()]);
        let v = ok_json(&out);
        assert_eq!(
            skips(&v),
            vec![
                (
                    locked.to_str().unwrap().to_string(),
                    "unreadable".to_string()
                ),
                (
                    secret.to_str().unwrap().to_string(),
                    "unreadable".to_string()
                ),
            ]
        );
        assert_eq!(record_paths(&v["added"]), vec![ok.to_str().unwrap()]);
        assert_eq!(counts(&v), [1, 0, 0, 0, 2]);
    }

    #[test]
    fn non_utf8_skipped() {
        let env = Env::new();
        let ok = env.put("ok.txt", b"ok\n");
        std::fs::write(env.root().join(OsStr::from_bytes(b"bad\xff.txt")), "x").unwrap();
        let dir = env.root().join(OsStr::from_bytes(b"d\xfe"));
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("inner.txt"), "x").unwrap();
        let out = env.run(["--json", "scan", env.root_str().as_str()]);
        let v = ok_json(&out);
        let root = env.root_str();
        // Lossy paths; the directory is reported once and not entered.
        assert_eq!(
            skips(&v),
            vec![
                (format!("{root}/bad\u{fffd}.txt"), "non_utf8".to_string()),
                (format!("{root}/d\u{fffd}"), "non_utf8".to_string()),
            ]
        );
        assert_eq!(record_paths(&v["added"]), vec![ok.to_str().unwrap()]);
    }

    // ---- criterion 8: the size limit -----------------------------------------------------------

    #[test]
    fn sparse_file_over_default_skipped() {
        let env = Env::new();
        env.put("ok.txt", b"ok\n");
        let sparse = env.root().join("sparse.bin");
        std::fs::File::create(&sparse)
            .unwrap()
            .set_len((1 << 30) + 1)
            .unwrap();
        // Skipped for its size, so never opened: mode 0 would make it `unreadable` otherwise.
        std::fs::set_permissions(&sparse, std::fs::Permissions::from_mode(0)).unwrap();
        let _restore = Restore(vec![sparse.clone()]);
        let started = Instant::now();
        let out = env.exec(None, &[], 60, ["--json", "scan", env.root_str().as_str()]);
        let v = ok_json(&out);
        assert_eq!(
            skips(&v),
            vec![(
                sparse.to_str().unwrap().to_string(),
                "too_large".to_string()
            )]
        );
        assert_eq!(counts(&v), [1, 0, 0, 0, 1]);
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "hashing 1 GiB would take longer: {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn max_size_precedence() {
        // small 10 bytes, exact 1024 bytes, big 2048 bytes.
        let case = |config: Option<&str>, flag: Option<&str>| -> Vec<String> {
            let env = Env::new();
            env.put("small.txt", &[b's'; 10]);
            env.put("exact.txt", &[b'e'; 1024]);
            env.put("big.txt", &[b'b'; 2048]);
            if let Some(text) = config {
                env.config(text);
            }
            let root = env.root_str();
            let flag = flag.map(|f| format!("--max-size={f}"));
            let mut args = vec!["--json", "scan", root.as_str()];
            args.extend(flag.as_deref());
            let v = ok_json(&env.run(&args));
            record_paths(&v["added"])
                .iter()
                .map(|p| p.rsplit('/').next().unwrap().to_string())
                .collect()
        };
        let cfg = |size: &str| format!("[scan]\nmax_size = \"{size}\"\n");
        // `byte_size > max_size` is skipped, so a file of exactly max_size is registered.
        assert_eq!(case(Some(&cfg("1KiB")), None), ["exact.txt", "small.txt"]);
        assert_eq!(case(None, Some("1kib")), ["exact.txt", "small.txt"]);
        assert_eq!(case(None, Some("1024")), ["exact.txt", "small.txt"]);
        // The flag beats the config, in either direction.
        assert_eq!(
            case(Some(&cfg("1KiB")), Some("4KiB")),
            ["big.txt", "exact.txt", "small.txt"]
        );
        assert_eq!(case(Some(&cfg("4KiB")), Some("100B")), ["small.txt"]);
        // The default is 1 GiB.
        assert_eq!(case(None, None), ["big.txt", "exact.txt", "small.txt"]);
    }

    // ---- criterion 10: what is registered ---------------------------------------------------------

    #[test]
    fn same_store_as_add_in_path_order() {
        let env = Env::new();
        // Walk order (by file name per directory) would be a/x.txt, a.txt, b.txt; the byte
        // order of the paths is a.txt, a/x.txt, b.txt ('.' < '/').
        env.put("a/x.txt", b"x\n");
        env.put("a.txt", b"a\n");
        env.put("b.txt", b"b\n");
        let out = env.run(["scan", env.root_str().as_str()]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);

        let root = env.root_str();
        let add_data = env.base.join("data-add");
        let files = [
            format!("{root}/a.txt"),
            format!("{root}/a/x.txt"),
            format!("{root}/b.txt"),
        ];
        let mut args = vec!["add"];
        args.extend(files.iter().map(String::as_str));
        let out = env.run_env(&[("HALI_DATA_DIR", add_data.to_str().unwrap())], &args);
        assert_eq!(out.code, Some(0), "{}", out.stderr);

        let scanned = listing(&env.data());
        let added = listing(&add_data);
        assert!(scanned.is_some());
        assert_eq!(
            scanned.as_ref().map(|m| m.keys().collect::<Vec<_>>()),
            added.as_ref().map(|m| m.keys().collect::<Vec<_>>())
        );
        for (name, bytes) in scanned.unwrap() {
            assert_eq!(
                String::from_utf8_lossy(&bytes),
                String::from_utf8_lossy(&added.as_ref().unwrap()[&name]),
                "{name} differs between `scan` and `add` of the same files"
            );
        }
    }

    // ---- criterion 11: cataloged Resources ------------------------------------------------------

    #[test]
    fn cataloged_ignored_file_refreshed() {
        let env = Env::new();
        env.put("a.txt", b"one\n");
        env.put("b.txt", b"bee\n");
        env.put("c.txt", b"see\n");
        let root = env.root_str();
        let v = ok_json(&env.run(["--json", "scan", root.as_str()]));
        assert_eq!(counts(&v), [3, 0, 0, 0, 0]);

        // a.txt changes, c.txt goes, and now an ignore file names a.txt and b.txt.
        env.put_at("a.txt", b"one two\n", PINNED_SECS + 100);
        std::fs::remove_file(env.root().join("c.txt")).unwrap();
        env.put(".ignore", b"a.txt\nb.txt\n");
        let v = ok_json(&env.run(["--json", "scan", root.as_str()]));
        assert_eq!(counts(&v), [0, 1, 1, 1, 0], "{v}");
        let a = &v["refreshed"][0];
        assert_eq!(a["path"], format!("{root}/a.txt"));
        assert_eq!(a["byte_size"], 8);
        assert_eq!(a["stale"], false);
        assert_eq!(a["missing"], false);
        let c = &v["missing"][0];
        assert_eq!(c["path"], format!("{root}/c.txt"));
        assert_eq!(c["missing"], true);
        assert_eq!(v["added"], serde_json::json!([]));
        // The vanished file's Resource is still cataloged.
        let ttl = std::fs::read_to_string(env.data().join("resources.ttl")).unwrap();
        assert!(ttl.contains("c.txt"), "{ttl}");

        // Filters that rule out every new file change nothing for cataloged ones; nothing left
        // to refresh, so the store is not rewritten.
        let before = listing(&env.data());
        let v = ok_json(&env.run([
            "--json",
            "scan",
            root.as_str(),
            "--exclude=a.txt",
            "--max-size=1",
            "--media-type=image/png",
        ]));
        assert_eq!(counts(&v), [0, 0, 2, 1, 0], "{v}");
        assert_eq!(listing(&env.data()), before);
    }

    // ---- criterion 13: atomicity ----------------------------------------------------------------

    #[test]
    fn shacl_violation_nothing_written() {
        let env = Env::new();
        copy_fixture("scan/needs-description", &env.data());
        env.put("a.txt", b"a\n");
        env.put("b.txt", b"b\n");
        let before = listing(&env.data());
        let root = env.root_str();

        let real = env.run(["--json", "scan", root.as_str()]);
        assert_eq!(real.code, Some(1), "{}", real.stderr);
        assert_eq!(real.stdout, "");
        let err: Value = serde_json::from_str(&real.stderr).unwrap();
        assert_eq!(err["error"]["rule"], "SHACL_VIOLATION");
        assert_eq!(
            err["error"]["details"]["total"], 2,
            "one check lists both files"
        );
        assert_eq!(listing(&env.data()), before, "nothing is written");

        let dry = env.run(["--json", "--dry-run", "scan", root.as_str()]);
        assert_eq!(dry.code, Some(1), "{}", dry.stderr);
        assert_eq!(dry.stdout, "");
        assert_eq!(
            dry.stderr, real.stderr,
            "a dry run fails as the real run does"
        );
        assert_eq!(listing(&env.data()), before);
    }

    #[test]
    fn nothing_to_do_writes_nothing() {
        // Only a hidden file, so nothing is registered: no data dir appears.
        let env = Env::new();
        env.put(".hidden", b"x\n");
        let root = env.root_str();
        let v = ok_json(&env.run(["--json", "scan", root.as_str()]));
        assert_eq!(counts(&v), [0, 0, 0, 0, 0]);
        assert_eq!(v["truncated"], false);
        assert!(!env.data().exists(), "the data dir was created");
        let out = env.run(["scan", root.as_str()]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert!(!env.data().exists());

        // A second scan that finds nothing new leaves the files alone, mtime included.
        env.put("a.txt", b"a\n");
        env.put("b.txt", b"b\n");
        let v = ok_json(&env.run(["--json", "scan", root.as_str()]));
        assert_eq!(counts(&v), [2, 0, 0, 0, 0]);
        let files: Vec<PathBuf> = std::fs::read_dir(env.data())
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert!(!files.is_empty());
        for f in &files {
            pin(f, 1_000_000_000);
        }
        let before = listing(&env.data());
        let v = ok_json(&env.run(["--json", "scan", root.as_str()]));
        assert_eq!(counts(&v), [0, 0, 2, 0, 0]);
        assert_eq!(listing(&env.data()), before);
        for f in &files {
            assert_eq!(
                std::fs::metadata(f).unwrap().modified().unwrap(),
                at(1_000_000_000),
                "{} was rewritten",
                f.display()
            );
        }
    }

    // ---- criteria 14, 15: output ------------------------------------------------------------------

    /// The golden tree: a.txt, b.txt, n (NUL bytes, so application/octet-stream with or without
    /// the MIME database) and big.txt (over `--max-size 10B`), all with mtime 2026-10-20T14:02:11Z.
    fn golden_tree(env: &Env) {
        env.put("a.txt", b"alpha\n");
        env.put("b.txt", b"beta\n");
        env.put("n", b"\0\0\0\0");
        env.put("big.txt", b"this is far too big\n");
    }

    #[test]
    fn scan_json() {
        let env = Env::new();
        golden_tree(&env);
        let out = env.run([
            "--json",
            "scan",
            env.root_str().as_str(),
            "--max-size",
            "10B",
        ]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!("cli_scan__scan_json", env.clean(&out.stdout));
        });
    }

    #[test]
    fn scan_dry_run_json() {
        let env = Env::new();
        golden_tree(&env);
        let out = env.run([
            "--json",
            "--dry-run",
            "scan",
            env.root_str().as_str(),
            "--max-size",
            "10B",
        ]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        assert!(!env.data().exists(), "a dry run writes nothing");
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!("cli_dry_run__scan_dry_run_json", env.clean(&out.stdout));
        });
    }

    #[test]
    fn scan_text() {
        let env = Env::new();
        env.put("a.txt", b"alpha\n");
        env.put("b.txt", b"beta\n");
        env.put("big.txt", b"this is far too big\n");
        let out = env.run(["scan", env.root_str().as_str(), "--max-size=10"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        assert_eq!(
            env.clean(&out.stdout),
            format!(
                "root\t/TMP/root\nadded\t2\nrefreshed\t0\nunchanged\t0\nmissing\t0\nskipped\t1\n\
                 media_type\ttext/plain\t2\n\
                 added resource {R1} \"a\" /TMP/root/a.txt\n\
                 added resource {R2} \"b\" /TMP/root/b.txt\n\
                 skipped too_large /TMP/root/big.txt\n"
            )
        );

        // Dry run: the same lines in the future tense, then the changes, then the note.
        let env = Env::new();
        env.put("a.txt", b"alpha\n");
        env.put("b.txt", b"beta\n");
        env.put("big.txt", b"this is far too big\n");
        let out = env.run([
            "--dry-run",
            "scan",
            env.root_str().as_str(),
            "--max-size=10",
        ]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "note: dry run; nothing was written\n");
        let text = env.clean(&out.stdout);
        let (head, changes) = text
            .split_once("skipped too_large /TMP/root/big.txt\n")
            .unwrap();
        assert_eq!(
            head,
            format!(
                "root\t/TMP/root\nadded\t2\nrefreshed\t0\nunchanged\t0\nmissing\t0\nskipped\t1\n\
                 media_type\ttext/plain\t2\n\
                 would add resource {R1} \"a\" /TMP/root/a.txt\n\
                 would add resource {R2} \"b\" /TMP/root/b.txt\n"
            )
        );
        assert!(!changes.is_empty());
        assert!(changes.lines().all(|l| l.starts_with("+ <")), "{changes}");
        assert!(!env.data().exists());
    }

    #[test]
    fn scan_text_escaped() {
        let env = Env::new();
        env.put("x\u{1b}[31my.txt", b"x\n");
        env.put("r\u{202e}l.txt", b"r\n");
        env.put("tab\there.txt", b"t\n");
        std::fs::write(env.root().join("big\u{1b}.txt"), vec![b'b'; 2000]).unwrap();
        let out = env.run(["scan", env.root_str().as_str(), "--max-size=100"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        for bad in ['\u{1b}', '\u{202e}', '\t'] {
            // Tabs separate fields only in the summary lines.
            if bad == '\t' {
                for line in out.stdout.lines() {
                    if line.starts_with("added resource")
                        || line.starts_with("skipped ")
                        || line.starts_with("refreshed ")
                    {
                        assert!(!line.contains('\t'), "{line:?}");
                    }
                }
            } else {
                assert!(!out.stdout.contains(bad), "{:?}", out.stdout);
            }
        }
        assert!(out.stdout.contains("x\\u{1b}[31my.txt"), "{}", out.stdout);
        assert!(out.stdout.contains("r\\u{202e}l.txt"), "{}", out.stdout);
        assert!(out.stdout.contains("tab\\u{9}here.txt"), "{}", out.stdout);
        assert!(out.stdout.contains("skipped too_large "), "{}", out.stdout);
        assert!(out.stdout.contains("big\\u{1b}.txt"), "{}", out.stdout);

        // JSON keeps the characters (as JSON escapes).
        let env2 = Env::new();
        env2.put("x\u{1b}y.txt", b"x\n");
        let v = ok_json(&env2.run(["--json", "scan", env2.root_str().as_str()]));
        assert_eq!(
            v["added"][0]["path"],
            format!("{}/x\u{1b}y.txt", env2.root_str())
        );
    }

    #[test]
    fn scan_caps_lists_but_counts_exactly() {
        let env = Env::new();
        for i in 0..101 {
            env.put(&format!("f{i:03}.txt"), format!("file {i}\n").as_bytes());
        }
        let root = env.root_str();
        let v = ok_json(&env.run(["--json", "scan", root.as_str()]));
        assert_eq!(counts(&v), [101, 0, 0, 0, 0]);
        assert_eq!(v["added"].as_array().unwrap().len(), 100);
        assert_eq!(v["added"][0]["path"], format!("{root}/f000.txt"));
        assert_eq!(v["added"][99]["path"], format!("{root}/f099.txt"));
        assert_eq!(v["truncated"], true);
        assert_eq!(v["media_types"]["text/plain"], 101);

        // Text mode: the same cap, and `truncated` last.
        let env = Env::new();
        for i in 0..101 {
            env.put(&format!("f{i:03}.txt"), format!("file {i}\n").as_bytes());
        }
        let out = env.run(["scan", env.root_str().as_str()]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        let lines: Vec<&str> = out.stdout.lines().collect();
        assert_eq!(lines[1], "added\t101");
        assert_eq!(
            lines
                .iter()
                .filter(|l| l.starts_with("added resource "))
                .count(),
            100
        );
        assert_eq!(lines.last(), Some(&"truncated\ttrue"));
    }

    // ---- criterion 15: the dry run ------------------------------------------------------------

    #[test]
    fn scan_dry_run_matches_real_run() {
        // New files only, on an absent store.
        let env = Env::new();
        golden_tree(&env);
        let root = env.root_str();
        let dry = support::assert_dry_run_matches_real_run(None, &["scan", root.as_str()]);
        assert_eq!(counts(&dry), [4, 0, 0, 0, 0]);

        // New, refreshed and missing against a store a scan wrote.
        let env = Env::new();
        env.put("a.txt", b"one\n");
        env.put("b.txt", b"bee\n");
        env.put("c.txt", b"see\n");
        let root = env.root_str();
        let out = env.run(["scan", root.as_str()]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        env.put_at("a.txt", b"one two\n", PINNED_SECS + 100);
        std::fs::remove_file(env.root().join("c.txt")).unwrap();
        env.put("d.txt", b"dee\n");
        let dry =
            support::assert_dry_run_matches_real_run(Some(&env.data()), &["scan", root.as_str()]);
        assert_eq!(counts(&dry), [1, 1, 1, 1, 0], "{dry}");
    }

    // ---- criterion 16: progress ---------------------------------------------------------------------

    #[test]
    fn progress_only_on_tty() {
        let env = Env::new();
        env.put("a.txt", b"a\n");
        env.put("b.txt", b"b\n");
        env.put("c.txt", b"c\n");
        let root = env.root_str();
        let fresh = |env: &Env| {
            let _ = std::fs::remove_dir_all(env.data());
        };

        let tty = env.run_env(&[("HALI_TEST_TTY", "1")], ["scan", root.as_str()]);
        assert_eq!(tty.code, Some(0), "{}", tty.stderr);
        assert!(tty.stderr.contains('\r'), "{:?}", tty.stderr);
        assert!(
            tty.stderr.contains("scanning: 3 files, 3 hashed"),
            "{:?}",
            tty.stderr
        );
        // No file names, ids or paths in the progress line.
        for secret in ["a.txt", "b.txt", "c.txt", root.as_str(), R1, "018bcfe5"] {
            assert!(!tty.stderr.contains(secret), "{secret}: {:?}", tty.stderr);
        }
        assert!(!tty.stdout.contains("scanning"));

        fresh(&env);
        let no_tty = env.run_env(&[("HALI_TEST_TTY", "0")], ["scan", root.as_str()]);
        assert_eq!(no_tty.code, Some(0), "{}", no_tty.stderr);
        assert_eq!(no_tty.stderr, "");
        assert_eq!(
            no_tty.stdout, tty.stdout,
            "stdout is the same with and without progress"
        );

        fresh(&env);
        let plain = env.run(["scan", root.as_str()]);
        assert_eq!(plain.stderr, "");
        assert_eq!(plain.stdout, tty.stdout);

        // `--json` turns progress off even on a terminal.
        fresh(&env);
        let json_tty = env.run_env(&[("HALI_TEST_TTY", "1")], ["--json", "scan", root.as_str()]);
        assert_eq!(json_tty.code, Some(0), "{}", json_tty.stderr);
        assert_eq!(json_tty.stderr, "");
        fresh(&env);
        let json_plain = env.run(["--json", "scan", root.as_str()]);
        assert_eq!(json_tty.stdout, json_plain.stdout);
    }

    // ---- criterion 17: nothing is written under the root --------------------------------------------

    #[test]
    fn root_tree_unchanged() {
        let env = Env::new();
        env.put("a.txt", b"a\n");
        env.put("sub/b.txt", b"b\n");
        env.put("sub/.hidden", b"h\n");
        env.put(".ignore", b"*.log\n");
        env.put("x.log", b"log\n");
        std::fs::create_dir(env.root().join("empty")).unwrap();
        symlink("a.txt", env.root().join("link")).unwrap();
        let root = env.root_str();
        let before = tree_print(&env.root());

        let check = |what: &str, args: &[&str]| {
            let out = env.run(args);
            assert_eq!(out.code, Some(0), "{what}: {}", out.stderr);
            assert_eq!(tree_print(&env.root()), before, "{what}: the tree changed");
        };
        check("dry run", &["--dry-run", "scan", root.as_str()]);
        check("real run", &["scan", root.as_str()]);
        check("rescan", &["scan", root.as_str()]);
        check(
            "everything",
            &["scan", root.as_str(), "--include-hidden", "--no-ignore"],
        );
        check(
            "dry run again",
            &["--json", "--dry-run", "scan", root.as_str()],
        );
        assert!(env.data().exists());
    }

    #[test]
    fn data_dir_under_root_pruned() {
        // A data dir inside the root is never entered: not its files, and not its store.
        let mut env = Env::new();
        env.data = env.root().join("store");
        std::fs::create_dir_all(env.data()).unwrap();
        std::fs::write(env.data().join("junk.txt"), "junk\n").unwrap();
        env.put("a.txt", b"a\n");
        let root = env.root_str();

        let v = ok_json(&env.run(["--json", "scan", root.as_str()]));
        assert_eq!(
            record_paths(&v["added"]),
            vec![format!("{root}/a.txt")],
            "{v}"
        );
        assert!(v["skipped"].as_array().unwrap().is_empty());
        let stored = listing(&env.data());
        assert!(stored.as_ref().unwrap().contains_key("resources.ttl"));

        // The store now exists inside the root; a rescan does not catalog it.
        let v = ok_json(&env.run(["--json", "scan", root.as_str()]));
        assert_eq!(counts(&v), [0, 0, 1, 0, 0], "{v}");
        assert_eq!(listing(&env.data()), stored);

        // Through a symlink to the data dir.
        let link = env.base.join("datalink");
        symlink(env.data(), &link).unwrap();
        env.data = link;
        let v = ok_json(&env.run(["--json", "scan", root.as_str()]));
        assert_eq!(counts(&v), [0, 0, 1, 0, 0], "{v}");
    }

    // ---- criterion 25: the walk flags and the config reach the walk --------------------------------

    #[test]
    fn walk_flags_and_config() {
        let build = || {
            let env = Env::new();
            env.put("vis.txt", b"v\n");
            env.put(".hidden.txt", b"h\n");
            env.put("skip.tmp", b"t\n");
            env.put("Downloads/d.txt", b"d\n");
            env.put("ig.log", b"l\n");
            env.put(".ignore", b"*.log\n");
            env.put("sub/keep.txt", b"k\n");
            let outside = env.base.join("outside.txt");
            std::fs::write(&outside, "o\n").unwrap();
            symlink(&outside, env.root().join("outside-link")).unwrap();
            symlink(env.root().join("sub"), env.root().join("dirlink")).unwrap();
            env
        };
        let run = |config: Option<&str>, flags: &[&str]| -> (Vec<String>, Vec<(String, String)>) {
            let env = build();
            if let Some(text) = config {
                env.config(text);
            }
            let root = env.root_str();
            let mut args = vec!["--json", "scan", root.as_str()];
            args.extend_from_slice(flags);
            let v = ok_json(&env.run(&args));
            let added = record_paths(&v["added"])
                .into_iter()
                .map(|p| p.strip_prefix(&format!("{root}/")).unwrap().to_string())
                .collect();
            let skipped = skips(&v)
                .into_iter()
                .map(|(p, r)| (p.strip_prefix(&format!("{root}/")).unwrap().to_string(), r))
                .collect();
            (added, skipped)
        };
        let link_skip = vec![("outside-link".to_string(), "outside_root".to_string())];
        let names = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();

        // Hidden files and `.ignore` rules apply; a symlinked directory is silent.
        let (added, skipped) = run(None, &[]);
        assert_eq!(
            added,
            names(&["Downloads/d.txt", "skip.tmp", "sub/keep.txt", "vis.txt"])
        );
        assert_eq!(skipped, link_skip);

        let (added, _) = run(None, &["--include-hidden"]);
        assert_eq!(
            added,
            names(&[
                ".hidden.txt",
                ".ignore",
                "Downloads/d.txt",
                "skip.tmp",
                "sub/keep.txt",
                "vis.txt"
            ])
        );

        let (added, _) = run(None, &["--no-ignore"]);
        assert_eq!(
            added,
            names(&[
                "Downloads/d.txt",
                "ig.log",
                "skip.tmp",
                "sub/keep.txt",
                "vis.txt"
            ])
        );

        let (added, _) = run(None, &["--exclude=*.tmp", "--exclude=Downloads/**"]);
        assert_eq!(added, names(&["sub/keep.txt", "vis.txt"]));

        // The config's excludes and include_hidden apply, and the flag adds to them.
        let (added, _) = run(
            Some("[scan]\nexclude = [\"*.tmp\"]\ninclude_hidden = true\n"),
            &["--exclude=Downloads/**"],
        );
        assert_eq!(
            added,
            names(&[".hidden.txt", ".ignore", "sub/keep.txt", "vis.txt"])
        );

        // An invalid line in an ignore file: its valid lines apply, and it is reported.
        let env = Env::new();
        env.put("vis.txt", b"v\n");
        env.put("x.log", b"l\n");
        env.put(".ignore", b"[\n*.log\n");
        let root = env.root_str();
        let v = ok_json(&env.run(["--json", "scan", root.as_str()]));
        assert_eq!(record_paths(&v["added"]), vec![format!("{root}/vis.txt")]);
        assert_eq!(
            skips(&v),
            vec![(format!("{root}/.ignore"), "bad_ignore_file".to_string())]
        );
    }

    // ---- criterion 26: --yes ------------------------------------------------------------------------

    #[test]
    fn yes_ignored_stdin_unread() {
        let env = Env::new();
        env.put("a.txt", b"a\n");
        let root = env.root_str();
        let mut outputs = Vec::new();
        for args in [
            vec!["--json", "scan", root.as_str()],
            vec!["--json", "scan", root.as_str(), "--yes"],
            vec!["--json", "--yes", "scan", root.as_str()],
        ] {
            let _ = std::fs::remove_dir_all(env.data());
            let mut child = env.spawn(&[], &args);
            // The pipe stays open and empty: a scan that reads stdin never returns.
            let stdin = child.stdin.take().unwrap();
            let started = Instant::now();
            let status = loop {
                if let Some(status) = child.try_wait().unwrap() {
                    break status;
                }
                if started.elapsed() > Duration::from_secs(30) {
                    let _ = child.kill();
                    panic!("{args:?}: scan did not finish; does it read stdin?");
                }
                std::thread::sleep(Duration::from_millis(50));
            };
            drop(stdin);
            let mut stdout = String::new();
            child
                .stdout
                .take()
                .unwrap()
                .read_to_string(&mut stdout)
                .unwrap();
            let mut stderr = String::new();
            child
                .stderr
                .take()
                .unwrap()
                .read_to_string(&mut stderr)
                .unwrap();
            assert_eq!(status.code(), Some(0), "{args:?}: {stderr}");
            assert_eq!(stderr, "", "{args:?}");
            outputs.push(stdout);
        }
        assert_eq!(outputs[0], outputs[1]);
        assert_eq!(outputs[0], outputs[2]);
    }

    // ---- criterion 27: the store survives a killed process ----------------------------------------

    #[test]
    fn killed_mid_walk_store_unchanged() {
        let env = Env::new();
        copy_fixture("store", &env.data());
        env.put("a.txt", b"a\n");
        let before = listing(&env.data());
        // The walk reads git's global config, so a FIFO there blocks `walk()`: the process is
        // stuck mid-scan, before the write.
        let fifo = env.base.join("fifo-config");
        mkfifo(&fifo);
        let root = env.root_str();
        let mut child = env.spawn(
            &[("GIT_CONFIG_GLOBAL", fifo.to_str().unwrap())],
            &["scan", root.as_str()],
        );
        let kill = |child: &mut std::process::Child| {
            let _ = child.kill();
            let _ = child.wait();
        };
        for _ in 0..15 {
            std::thread::sleep(Duration::from_millis(100));
            if let Some(status) = child.try_wait().unwrap() {
                panic!(
                    "scan exited ({status}) instead of blocking on the FIFO git config: \
                     the precondition (the process is stuck mid-walk) does not hold"
                );
            }
        }
        kill(&mut child);
        assert_eq!(listing(&env.data()), before, "the data dir changed");
    }

    // ---- criterion 28: deep trees -------------------------------------------------------------------

    #[test]
    fn deep_tree_time_bound() {
        // 200 nested directories with the three ignore files in each: a hang detector for the
        // walk's cost in depth (spec §7, CPU in deep trees).
        let env = Env::new();
        std::fs::create_dir(env.root().join(".git")).unwrap();
        let mut dir = env.root();
        env.put("f.txt", b"top\n");
        for _ in 0..200 {
            dir = dir.join("d");
            std::fs::create_dir(&dir).unwrap();
            for name in [".gitignore", ".ignore", ".haliignore"] {
                std::fs::write(dir.join(name), "*.log\n").unwrap();
            }
            std::fs::write(dir.join("f.txt"), "level\n").unwrap();
        }
        let started = Instant::now();
        let out = env.exec(None, &[], 15, ["--json", "scan", env.root_str().as_str()]);
        let elapsed = started.elapsed();
        let v = ok_json(&out);
        assert_eq!(counts(&v), [201, 0, 0, 0, 0], "{elapsed:?}");
        assert!(elapsed < Duration::from_secs(15), "{elapsed:?}");
    }

    // ---- criterion 18: the schema list ---------------------------------------------------------------

    #[test]
    fn schema_lists_scan() {
        let env = Env::new();
        let out = env.run(["schema", "--json"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!("cli_scan__schema_list_json", out.stdout);
        });
    }
}
