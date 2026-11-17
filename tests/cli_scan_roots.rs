//! Goal tests for the remembered scan roots through the real binary (#23, spec §6, §7, §9.1
//! **Scan:**, §9.4, §9.8). Locked golden snapshots sit in tests/snapshots/ (hand-written from
//! the approved contract; the ids are those `hali scan` mints with `HALI_TEST_ID_SEED=1` for a
//! fresh store, as in tests/cli_scan.rs): stdout `cli_scan_roots__bare_scan_json`,
//! `cli_scan_roots__scan_list_json`, `cli_scan_roots__forget_root_json`,
//! `cli_dry_run__scan_forget_root_dry_run_json`, and the stderr envelope
//! `cli_scan_roots__bare_prune_confirmation_required_json_envelope`.
//!
//! The contract the tests fix (the approved contract leaves these open; change them, with the
//! human, if the wording should differ):
//! - `roots.json` is compact JSON and a newline: `{"version":1,"roots":[<entry>…]}`, an entry
//!   `{"root","include_hidden","no_ignore","exclude","max_size","media_types"}` in that order;
//!   `exclude` and `media_types` as given on the command line (`media_types` lowercase),
//!   `max_size` in bytes or `null`.
//! - The bare scan's JSON is the single-scan object with `"roots":[…]` (the real paths scanned,
//!   in byte order, the unavailable ones not included) where `"root"` was, followed at once by
//!   `"unavailable":[{"root","reason"}…]` (in byte order of the root; `reason` any non-empty
//!   text), then `counts` and the rest as for one root. With no remembered roots every list is
//!   empty and every count 0. In text mode every root scanned and every unavailable root appears
//!   in the output.
//! - A bare `--prune` prompt counts the roots it scanned: `forget <n> missing resources under
//!   <k> scan roots? [y/N] ` (no singular form); its retry is `hali scan --prune [--force]
//!   --yes`.
//! - `scan --list`: JSON `{"schema_version":1,"roots":[<entry>…]}`; text one line per root, the
//!   root first, then one tab-separated field per setting that is not the default, in this
//!   order: `--include-hidden`, `--no-ignore`, `--exclude=<glob>` (each), `--max-size=<bytes>`,
//!   `--media-type=<pattern>` (each). No roots: no output.
//! - `scan --forget-root <root>`: JSON `{"schema_version":1,"root":<entry as it was>}`, with
//!   `--dry-run` also `"dry_run":true,"changes":[]`. The usage errors name a flag involved
//!   (`--list`, `--forget-root` or the scan flag) in stderr; the unknown root's mentions `hali
//!   scan --list`.
//! - A scan that cannot read `roots.json` is an error of rule `STORE_IO` or `STORE_UNPARSEABLE`
//!   (exit 3) whose message names the file, before anything is written.
//!
//! Isolation (the walker honours ignore files in the root's parents and the user's git
//! configuration, so a test must not see the host's): the tree is built under the system temp
//! dir, never under `target/`; `HOME`, `XDG_STATE_HOME`, `XDG_CONFIG_HOME`, `HALI_DATA_DIR`,
//! `GIT_CONFIG_GLOBAL` and `GIT_CONFIG_SYSTEM` point at temp paths, the rest of the environment
//! is cleared; every test asserts that no ancestor of the temp dir holds a `.gitignore`,
//! `.ignore`, `.haliignore`, `.git` or `.jj`. The real `~/.local/state` is never touched.
//!
//! Every run pins `HALI_TEST_NOW` and `HALI_TEST_ID_SEED=1`; setup runs use `setup`, which
//! gives each run its own seed so that ids never repeat.

mod goal {
    use assert_cmd::Command;
    use serde_json::{Value, json};
    use std::cell::Cell;
    use std::collections::BTreeMap;
    use std::ffi::{CString, OsStr, OsString};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime};
    use tempfile::TempDir;

    const NOW: &str = "2026-10-24T09:30:00Z";
    const PINNED_SECS: u64 = 1_792_504_931; // 2026-10-20T14:02:11Z
    const R1: &str = "018bcfe5-6801-710a-adec-89025cc1beeb";
    const R2: &str = "018bcfe5-6804-7099-ac6c-d7363ca585e7";
    const R3: &str = "018bcfe5-6806-7775-9c77-01564f619afc";

    struct Env {
        _tmp: TempDir,
        base: PathBuf,
        data: PathBuf,
        seed: Cell<u32>,
    }

    struct Out {
        code: Option<i32>,
        stdout: String,
        stderr: String,
    }

    /// Restores a directory to mode 0755 on drop so the temp dir can go.
    struct Restore(PathBuf);

    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
        }
    }

    fn privileged() -> bool {
        // SAFETY: geteuid has no preconditions.
        unsafe { libc::geteuid() == 0 }
    }

    fn pin(path: &Path, secs: u64) {
        std::fs::File::open(path)
            .unwrap()
            .set_modified(SystemTime::UNIX_EPOCH + Duration::new(secs, 0))
            .unwrap();
    }

    /// An entry of `roots.json` / `--list`.
    fn entry(
        root: &str,
        include_hidden: bool,
        no_ignore: bool,
        exclude: &[&str],
        max_size: Option<u64>,
        media_types: &[&str],
    ) -> Value {
        json!({
            "root": root,
            "include_hidden": include_hidden,
            "no_ignore": no_ignore,
            "exclude": exclude,
            "max_size": max_size,
            "media_types": media_types,
        })
    }

    fn plain(root: &str) -> Value {
        entry(root, false, false, &[], None, &[])
    }

    /// The canonical contents of `roots.json`.
    fn roots_text(entries: &[Value]) -> String {
        format!("{}\n", json!({"version": 1, "roots": entries}))
    }

    impl Env {
        /// A fresh temp dir with the roots `a/` and `b/`, `cfg/` and `home/`; the data dir
        /// `data/` and the state dir `state/hali/` are not created.
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
            for d in ["a", "b", "cfg", "home"] {
                std::fs::create_dir_all(base.join(d)).unwrap();
            }
            let data = base.join("data");
            Env {
                _tmp: tmp,
                base,
                data,
                seed: Cell::new(10),
            }
        }

        fn dir(&self, name: &str) -> PathBuf {
            self.base.join(name)
        }

        /// `<base>/<name>` as a string.
        fn s(&self, name: &str) -> String {
            self.dir(name).to_str().unwrap().to_string()
        }

        fn data(&self) -> PathBuf {
            self.data.clone()
        }

        /// The state dir `XDG_STATE_HOME` names in every run unless a test says otherwise.
        fn state_dir(&self) -> PathBuf {
            self.base.join("state/hali")
        }

        fn roots_file(&self) -> PathBuf {
            self.state_dir().join("roots.json")
        }

        fn roots_bytes(&self) -> Option<Vec<u8>> {
            std::fs::read(self.roots_file()).ok()
        }

        /// Writes `roots.json` by hand (creating the state dir).
        fn set_roots(&self, entries: &[Value]) {
            std::fs::create_dir_all(self.state_dir()).unwrap();
            std::fs::write(self.roots_file(), roots_text(entries)).unwrap();
        }

        /// Writes `<base>/<dir>/<rel>` (creating parents), mtime pinned.
        fn put(&self, dir: &str, rel: &str, bytes: &[u8]) -> PathBuf {
            let path = self.dir(dir).join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, bytes).unwrap();
            pin(&path, PINNED_SECS);
            path
        }

        fn config(&self, text: &str) {
            let dir = self.base.join("cfg/hali");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("config.toml"), text).unwrap();
        }

        fn vars(&self, extra: &[(&str, &str)], unset: &[&str]) -> Vec<(OsString, OsString)> {
            let mut vars: Vec<(OsString, OsString)> = vec![
                ("HOME".into(), self.base.join("home").into()),
                ("XDG_STATE_HOME".into(), self.base.join("state").into()),
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
            vars.retain(|(name, _)| !unset.iter().any(|u| name == OsStr::new(u)));
            vars
        }

        #[allow(clippy::too_many_arguments)]
        fn exec<I, S>(
            &self,
            cwd: Option<&Path>,
            extra: &[(&str, &str)],
            unset: &[&str],
            timeout: u64,
            stdin: &[u8],
            args: I,
        ) -> Out
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            let mut cmd = Command::cargo_bin("hali").unwrap();
            cmd.env_clear()
                .envs(self.vars(extra, unset))
                .timeout(Duration::from_secs(timeout))
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
            self.exec(None, &[], &[], 60, b"", args)
        }

        fn run_env<I, S>(&self, extra: &[(&str, &str)], args: I) -> Out
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            self.exec(None, extra, &[], 60, b"", args)
        }

        fn run_in<I, S>(&self, cwd: &Path, args: I) -> Out
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            self.exec(Some(cwd), &[], &[], 60, b"", args)
        }

        /// A terminal (`HALI_TEST_TTY=1`) whose user types `answer`.
        fn tty<I, S>(&self, answer: &[u8], args: I) -> Out
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            self.exec(None, &[("HALI_TEST_TTY", "1")], &[], 60, answer, args)
        }

        /// A setup run: it must succeed, and mints ids from a seed no other run uses.
        fn setup<I, S>(&self, args: I) -> Value
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            let seed = self.seed.get();
            self.seed.set(seed + 1);
            let out = self.run_env(&[("HALI_TEST_ID_SEED", seed.to_string().as_str())], args);
            assert_eq!(out.code, Some(0), "setup failed: {}", out.stderr);
            if out.stdout.starts_with('{') {
                serde_json::from_str(&out.stdout).unwrap()
            } else {
                Value::Null
            }
        }

        /// Catalogs and remembers the root `<base>/<name>` with extra scan flags.
        fn scan_root(&self, name: &str, flags: &[&str]) -> Value {
            let root = self.s(name);
            let mut args = vec!["--json", "scan", root.as_str()];
            args.extend_from_slice(flags);
            self.setup(args)
        }

        /// The output with the temp dir shown as `/TMP`.
        fn clean(&self, text: &str) -> String {
            text.replace(self.base.to_str().unwrap(), "/TMP")
        }

        fn resources(&self) -> String {
            std::fs::read_to_string(self.data.join("resources.ttl")).unwrap()
        }

        /// The `find` total (every Resource cataloged).
        fn find_total(&self) -> u64 {
            let out = self.run(["--json", "find", "--limit", "1"]);
            assert_eq!(out.code, Some(0), "{}", out.stderr);
            serde_json::from_str::<Value>(&out.stdout).unwrap()["total"]
                .as_u64()
                .unwrap()
        }
    }

    /// Relative path -> bytes of every file under `dir`; `None` when it does not exist.
    fn tree(dir: &Path) -> Option<BTreeMap<String, Vec<u8>>> {
        fn walk(dir: &Path, base: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
            for e in std::fs::read_dir(dir).unwrap() {
                let path = e.unwrap().path();
                let name = path.strip_prefix(base).unwrap().display().to_string();
                let meta = std::fs::symlink_metadata(&path).unwrap();
                if meta.is_dir() {
                    out.insert(format!("{name}/"), Vec::new());
                    walk(&path, base, out);
                } else if meta.is_file() {
                    out.insert(name, std::fs::read(&path).unwrap());
                } else {
                    // A FIFO or a symlink: never opened.
                    out.insert(name, b"<special>".to_vec());
                }
            }
        }
        if !dir.exists() {
            return None;
        }
        let mut out = BTreeMap::new();
        walk(dir, dir, &mut out);
        Some(out)
    }

    /// Asserts exit 0, empty stderr and one compact JSON line; returns it parsed.
    fn ok_json(out: &Out) -> Value {
        assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
        assert_eq!(out.stderr, "");
        assert_eq!(out.stdout.matches('\n').count(), 1, "{:?}", out.stdout);
        serde_json::from_str(&out.stdout)
            .unwrap_or_else(|e| panic!("not JSON ({e}): {:?}", out.stdout))
    }

    /// Asserts exit 0 and returns the JSON on stdout (stderr may carry notes).
    fn json_out(out: &Out) -> Value {
        assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
        serde_json::from_str(&out.stdout)
            .unwrap_or_else(|e| panic!("not JSON ({e}): {:?}", out.stdout))
    }

    fn envelope(out: &Out) -> Value {
        let line = out.stderr.lines().last().unwrap();
        serde_json::from_str(&line[line.find('{').unwrap()..])
            .unwrap_or_else(|e| panic!("no error envelope ({e}): {:?}", out.stderr))
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

    fn record_paths(list: &Value) -> Vec<String> {
        list.as_array()
            .unwrap()
            .iter()
            .map(|r| r["path"].as_str().unwrap().to_string())
            .collect()
    }

    fn counts8(v: &Value) -> [u64; 8] {
        let c = &v["counts"];
        [
            "added",
            "refreshed",
            "relinked",
            "ambiguous",
            "unchanged",
            "missing",
            "pruned",
            "skipped",
        ]
        .map(|k| c[k].as_u64().unwrap())
    }

    // ---- row 1, 3: where the roots are kept ----------------------------------------------------

    /// Row 1, 3: The roots are kept in `$XDG_STATE_HOME/hali/roots.json`, else
    /// `$HOME/.local/state/hali/roots.json` (a relative or empty `XDG_STATE_HOME` is ignored),
    /// the directory 0700 and the file 0600.
    #[test]
    fn state_dir_resolution() {
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;

        let env = Env::new();
        env.put("a", "x.txt", b"x\n");
        env.setup(["scan", env.s("a").as_str()]);
        assert!(env.roots_file().is_file());
        assert_eq!(mode(&env.state_dir()), 0o700);
        assert_eq!(mode(&env.roots_file()), 0o600);
        assert!(!env.base.join("home/.local").exists());

        for value in ["", "relative/state"] {
            let env = Env::new();
            env.put("a", "x.txt", b"x\n");
            let a = env.s("a");
            let out = env.exec(
                Some(&env.base),
                &[("XDG_STATE_HOME", value)],
                &[],
                60,
                b"",
                ["scan", a.as_str()],
            );
            assert_eq!(out.code, Some(0), "{value:?}: {}", out.stderr);
            let file = env.base.join("home/.local/state/hali/roots.json");
            assert!(file.is_file(), "{value:?}");
            assert_eq!(mode(file.parent().unwrap()), 0o700, "{value:?}");
            assert_eq!(mode(&file), 0o600, "{value:?}");
            assert!(!env.base.join("state").exists(), "{value:?}");
            assert!(!env.base.join("relative").exists(), "{value:?}");
        }
    }

    /// Row 1: The state dir is not the data dir: the store never holds `roots.json`.
    #[test]
    fn roots_json_is_outside_the_store() {
        let env = Env::new();
        env.put("a", "x.txt", b"x\n");
        env.setup(["scan", env.s("a").as_str()]);
        assert!(env.roots_file().is_file(), "the scan remembered its root");
        let data = tree(&env.data()).unwrap();
        assert!(!data.contains_key("roots.json"), "{:?}", data.keys());
        assert!(data.contains_key("resources.ttl"));
    }

    // ---- row 4: what is remembered, and when ----------------------------------------------------

    /// Row 4: A real `scan <root>` remembers the root with the flags given on the command line
    /// (not the config's), a rescan replaces its settings, the file is sorted by root, a root
    /// is remembered by its real path, and a scan that changes nothing remembers too.
    #[test]
    fn remembers_after_real_scan_only() {
        let env = Env::new();
        env.put("a", "x.txt", b"x\n");
        env.put("b", "y.txt", b"y\n");
        std::fs::create_dir_all(env.dir("c")).unwrap();
        let (a, b, c) = (env.s("a"), env.s("b"), env.s("c"));

        // Every flag, given in an odd order; the config must not leak into the entry.
        env.config("[scan]\nmax_size = \"2MiB\"\nexclude = [\"*.log\"]\ninclude_hidden = true\n");
        env.setup([
            "scan",
            "--media-type=TEXT/*",
            b.as_str(),
            "--max-size=1KiB",
            "--exclude=*.tmp",
            "--no-ignore",
            "--exclude=a b",
            "--include-hidden",
            "--media-type=application/pdf",
        ]);
        assert_eq!(
            String::from_utf8(env.roots_bytes().unwrap()).unwrap(),
            roots_text(&[entry(
                &b,
                true,
                true,
                &["*.tmp", "a b"],
                Some(1024),
                &["text/*", "application/pdf"]
            )])
        );

        // No flags: the config's values are not stored, so the entry is all defaults.
        env.setup(["scan", a.as_str()]);
        assert_eq!(
            String::from_utf8(env.roots_bytes().unwrap()).unwrap(),
            roots_text(&[
                plain(&a),
                entry(
                    &b,
                    true,
                    true,
                    &["*.tmp", "a b"],
                    Some(1024),
                    &["text/*", "application/pdf"]
                )
            ]),
            "sorted by root: a before b although b was scanned first"
        );
        env.config("");

        // A scan that changes nothing still remembers its root (an empty directory, here).
        env.setup(["scan", c.as_str()]);
        // Rescanning a root with other flags replaces its settings.
        env.setup(["scan", a.as_str(), "--max-size=5", "--exclude=*.bak"]);
        assert_eq!(
            String::from_utf8(env.roots_bytes().unwrap()).unwrap(),
            roots_text(&[
                entry(&a, false, false, &["*.bak"], Some(5), &[]),
                entry(
                    &b,
                    true,
                    true,
                    &["*.tmp", "a b"],
                    Some(1024),
                    &["text/*", "application/pdf"]
                ),
                plain(&c),
            ])
        );
        // ...and a rescan with no flags puts the defaults back.
        env.setup(["scan", a.as_str()]);
        assert_eq!(
            String::from_utf8(env.roots_bytes().unwrap()).unwrap(),
            roots_text(&[
                plain(&a),
                entry(
                    &b,
                    true,
                    true,
                    &["*.tmp", "a b"],
                    Some(1024),
                    &["text/*", "application/pdf"]
                ),
                plain(&c),
            ])
        );
    }

    /// Row 4: The root is remembered by its real path whatever the argument: relative, with
    /// dots, or a symlink.
    #[test]
    fn a_root_is_remembered_by_its_real_path() {
        for arg in ["a", "./a/../a/", "link"] {
            let env = Env::new();
            env.put("a", "x.txt", b"x\n");
            symlink(env.dir("a"), env.base.join("link")).unwrap();
            let out = env.run_in(&env.base, ["scan", arg]);
            assert_eq!(out.code, Some(0), "{arg}: {}", out.stderr);
            assert_eq!(
                String::from_utf8(env.roots_bytes().unwrap()).unwrap(),
                roots_text(&[plain(&env.s("a"))]),
                "{arg}"
            );
        }
    }

    /// Row 4: Not remembered: a dry run, a scan that fails (usage, the data dir, SHACL), a prune
    /// that needs a confirmation it does not get, a prune that is declined. A root already
    /// remembered keeps its settings through them.
    #[test]
    fn not_remembered_unless_a_real_scan_succeeds() {
        // A dry run on a fresh machine writes no state at all.
        let env = Env::new();
        env.put("a", "x.txt", b"x\n");
        let out = env.run(["--dry-run", "scan", env.s("a").as_str()]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert!(!env.base.join("state").exists(), "dry run made a state dir");
        assert!(!env.data().exists());

        // Failures on a fresh machine: no state dir either.
        let fresh_fails = |args: &dyn Fn(&Env) -> Vec<String>, want: i32, prep: &dyn Fn(&Env)| {
            let env = Env::new();
            env.put("a", "x.txt", b"x\n");
            prep(&env);
            let args = args(&env);
            let out = env.run(&args);
            assert_eq!(out.code, Some(want), "{args:?}: {}", out.stderr);
            assert!(
                !env.base.join("state").exists(),
                "{args:?}: a failed scan remembered its root"
            );
        };
        fresh_fails(
            &|e: &Env| vec!["scan".into(), e.s("missing")],
            2,
            &|_: &Env| {},
        );
        fresh_fails(
            &|e: &Env| vec!["scan".into(), e.s("a"), "--max-size=abc".into()],
            2,
            &|_: &Env| {},
        );
        fresh_fails(
            &|e: &Env| vec!["scan".into(), e.s("data")],
            2,
            &|e: &Env| copy_fixture("store", &e.data()),
        );
        fresh_fails(&|e: &Env| vec!["scan".into(), e.s("a")], 1, &|e: &Env| {
            copy_fixture("scan/needs-description", &e.data())
        });

        // A root that is remembered keeps its settings when a later scan of it does not
        // succeed, or is only a preview.
        let env = Env::new();
        env.put("a", "x.txt", b"x\n");
        env.put("a", "y.txt", b"y\n");
        let a = env.s("a");
        env.setup(["scan", a.as_str(), "--max-size=1KiB"]);
        let kept = env.roots_bytes().unwrap();
        std::fs::remove_file(env.dir("a").join("y.txt")).unwrap();

        // Needs a confirmation and has no terminal.
        let out = env.run(["scan", a.as_str(), "--prune", "--include-hidden"]);
        assert_eq!(out.code, Some(1), "{}", out.stderr);
        assert_eq!(env.roots_bytes().unwrap(), kept);
        // Declined at a terminal.
        let out = env.tty(b"n\n", ["scan", a.as_str(), "--prune", "--include-hidden"]);
        assert_eq!(out.code, Some(1), "{}", out.stderr);
        assert_eq!(env.roots_bytes().unwrap(), kept);
        // A usage error.
        let out = env.run(["scan", a.as_str(), "--include-hidden", "--exclude=!x"]);
        assert_eq!(out.code, Some(2), "{}", out.stderr);
        assert_eq!(env.roots_bytes().unwrap(), kept);
        // A dry run of a rescan with other flags, and of a new root.
        let out = env.run(["--dry-run", "scan", a.as_str(), "--include-hidden"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(env.roots_bytes().unwrap(), kept);
        let out = env.run(["--dry-run", "scan", env.s("b").as_str()]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(env.roots_bytes().unwrap(), kept);

        // The same prune with a confirmation succeeds and then remembers the new settings.
        let out = env.run(["scan", a.as_str(), "--prune", "--include-hidden", "--yes"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(
            String::from_utf8(env.roots_bytes().unwrap()).unwrap(),
            roots_text(&[entry(&a, true, false, &[], None, &[])])
        );
    }

    // ---- row 5: a bare scan --------------------------------------------------------------------

    /// The bare-scan scene: roots a (max size 10 bytes) and b (the default), remembered by hand
    /// into a fresh store.
    fn bare_scene() -> Env {
        let env = Env::new();
        env.put("a", "a.txt", b"alpha\n");
        env.put("a", "big.txt", b"this is far too big\n");
        env.put("b", "b.txt", b"beta\n");
        env.put("b", "big.txt", b"this is far too big\n");
        env.set_roots(&[
            entry(&env.s("a"), false, false, &[], Some(10), &[]),
            plain(&env.s("b")),
        ]);
        env
    }

    /// Row 5: A bare `scan` scans every remembered root with its own settings, as one run:
    /// the same file is too large for one root and registered under the other.
    #[test]
    fn bare_scan_uses_each_roots_settings() {
        let env = bare_scene();
        let before = env.roots_bytes().unwrap();
        let out = env.run(["--json", "scan"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!("cli_scan_roots__bare_scan_json", env.clean(&out.stdout));
        });
        let v: Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(
            record_paths(&v["added"]),
            [
                env.s("a") + "/a.txt",
                env.s("b") + "/b.txt",
                env.s("b") + "/big.txt"
            ]
        );
        assert_eq!(
            v["added"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r["id"].as_str().unwrap())
                .collect::<Vec<_>>(),
            [R1, R2, R3]
        );
        // Q7: a bare scan never rewrites roots.json.
        assert_eq!(env.roots_bytes().unwrap(), before);
        let ttl = env.resources();
        assert!(ttl.contains(&format!("file://{}/b/big.txt", env.base.display())));
        assert!(!ttl.contains(&format!("file://{}/a/big.txt", env.base.display())));
    }

    /// Row 5: Each root's other settings apply to that root only, and the config merges in as
    /// it does for a scan of one root.
    #[test]
    fn bare_scan_applies_every_setting_per_root() {
        let env = Env::new();
        for dir in ["a", "b"] {
            env.put(dir, "x.txt", b"x\n");
            env.put(dir, "x.log", b"l\n");
            env.put(dir, ".h.txt", b"h\n");
            env.put(dir, "y.tmp", b"t\n");
            env.put(dir, "doc.pdf", b"%PDF-1.4\n");
        }
        env.config("[scan]\nexclude = [\"*.tmp\"]\n");
        env.set_roots(&[
            entry(&env.s("a"), false, false, &["*.log"], None, &["text/*"]),
            entry(&env.s("b"), true, false, &[], None, &[]),
        ]);
        let v = ok_json(&env.run(["--json", "scan"]));
        let (a, b) = (env.s("a"), env.s("b"));
        let mut got = record_paths(&v["added"]);
        got.sort();
        assert_eq!(
            got,
            [
                format!("{a}/x.txt"),
                format!("{b}/.h.txt"),
                format!("{b}/doc.pdf"),
                format!("{b}/x.log"),
                format!("{b}/x.txt"),
            ],
            "a: no hidden, no *.log, text only; b: hidden, everything; *.tmp from the config in both"
        );
    }

    /// Row 5: A bare `--dry-run scan` reports what the real run does and writes nothing, not even
    /// the state.
    #[test]
    fn bare_scan_dry_run_writes_nothing() {
        let env = bare_scene();
        let roots = env.roots_bytes().unwrap();
        let state = tree(&env.state_dir());
        let out = env.run(["--json", "--dry-run", "scan"]);
        let v = ok_json(&out);
        assert_eq!(v["dry_run"], true);
        assert_eq!(v["roots"], json!([env.s("a"), env.s("b")]));
        assert_eq!(v["counts"]["added"], 3);
        assert!(!v["changes"].as_array().unwrap().is_empty());
        assert!(!env.data().exists(), "a dry run creates no data dir");
        assert_eq!(env.roots_bytes().unwrap(), roots);
        assert_eq!(tree(&env.state_dir()), state);
    }

    /// Row 5: Text mode names every root scanned and the counts.
    #[test]
    fn bare_scan_text() {
        let env = bare_scene();
        env.set_roots(&[
            entry(&env.s("a"), false, false, &[], Some(10), &[]),
            plain(&env.s("b")),
            plain(&env.s("gone")),
        ]);
        let out = env.run(["scan"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        let text = out.stdout;
        for root in [env.s("a"), env.s("b"), env.s("gone")] {
            assert!(text.contains(&root), "{root} is not in:\n{text}");
        }
        assert!(text.lines().any(|l| l == "added\t3"), "{text}");
        assert!(text.contains("unavailable"), "{text}");
    }

    // ---- row 6: moves between roots ------------------------------------------------------------

    /// The move scene: roots a and b remembered and cataloged; a/f.txt moves to b/f.txt and
    /// a/gone.txt is deleted.
    fn moved_scene() -> (Env, String) {
        let env = Env::new();
        let f = env.put("a", "f.txt", b"fff\n");
        env.put("a", "gone.txt", b"ggg\n");
        env.put("a", "stay.txt", b"sss\n");
        env.put("b", "other.txt", b"ooo\n");
        let a = env.scan_root("a", &[]);
        env.scan_root("b", &[]);
        let f_id = a["added"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["path"].as_str().unwrap().ends_with("/a/f.txt"))
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        let to = env.dir("b").join("f.txt");
        std::fs::rename(f, &to).unwrap();
        std::fs::remove_file(env.dir("a").join("gone.txt")).unwrap();
        (env, f_id)
    }

    /// Row 6: A bare `scan --prune` relinks a file that moved from one root to another instead
    /// of forgetting its Resource, and forgets the one that really vanished.
    #[test]
    fn bare_prune_relinks_a_cross_root_move() {
        let (env, moved_id) = moved_scene();
        let before = env.roots_bytes().unwrap();
        let v = ok_json(&env.run(["--json", "scan", "--prune", "--yes"]));
        assert_eq!(counts8(&v), [0, 0, 1, 0, 2, 0, 1, 0], "{v}");
        assert_eq!(v["relinked"][0]["resource"]["id"], moved_id.as_str());
        assert_eq!(v["relinked"][0]["from"], env.s("a") + "/f.txt");
        assert_eq!(v["relinked"][0]["resource"]["path"], env.s("b") + "/f.txt");
        assert_eq!(record_paths(&v["pruned"]), [env.s("a") + "/gone.txt"]);
        assert_eq!(v["missing"], json!([]));
        let ttl = env.resources();
        assert!(ttl.contains(&moved_id), "the moved Resource is kept");
        assert!(ttl.contains("b/f.txt") && !ttl.contains("a/f.txt"));
        assert!(!ttl.contains("a/gone.txt"));
        assert_eq!(env.roots_bytes().unwrap(), before);
    }

    /// Row 6: Without `--prune` the same move is relinked and the vanished file only reported;
    /// with `--dry-run --prune` the same is previewed and nothing is written, no question asked.
    #[test]
    fn bare_scan_relinks_a_cross_root_move_without_prune() {
        let (env, _) = moved_scene();
        let v = ok_json(&env.run(["--json", "scan"]));
        assert_eq!(counts8(&v), [0, 0, 1, 0, 2, 1, 0, 0], "{v}");
        assert_eq!(record_paths(&v["missing"]), [env.s("a") + "/gone.txt"]);

        let (env, _) = moved_scene();
        let data = tree(&env.data());
        let state = tree(&env.state_dir());
        let v = ok_json(&env.run(["--json", "--dry-run", "scan", "--prune"]));
        assert_eq!(counts8(&v), [0, 0, 1, 0, 2, 0, 1, 0], "{v}");
        assert_eq!(v["dry_run"], true);
        assert_eq!(tree(&env.data()), data);
        assert_eq!(tree(&env.state_dir()), state);
    }

    /// Row 6, 13: The prompt of a bare prune counts the roots scanned; `y` confirms, anything
    /// else declines and writes nothing.
    #[test]
    fn bare_prune_prompt_names_the_roots() {
        const PROMPT: &str = "forget 1 missing resources under 2 scan roots? [y/N] ";
        let (env, _) = moved_scene();
        let out = env.tty(b"y\n", ["--json", "scan", "--prune"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, PROMPT);
        let v: Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(v["counts"]["pruned"], 1);

        let (env, _) = moved_scene();
        let data = tree(&env.data());
        let state = tree(&env.state_dir());
        let out = env.tty(b"n\n", ["--json", "scan", "--prune"]);
        assert_eq!(out.code, Some(1), "{}", out.stderr);
        assert_eq!(out.stdout, "");
        assert!(out.stderr.starts_with(PROMPT), "{:?}", out.stderr);
        let e = envelope(&out);
        assert_eq!(e["error"]["rule"], "CONFIRMATION_REQUIRED");
        assert_eq!(e["error"]["details"]["command"], "scan");
        assert_eq!(e["error"]["details"]["declined"], true);
        assert_eq!(tree(&env.data()), data, "declining writes nothing at all");
        assert_eq!(tree(&env.state_dir()), state);
    }

    /// Row 13: Without terminals and without `--yes` a bare prune that would forget something
    /// is `CONFIRMATION_REQUIRED`, and the retry is `hali scan --prune [--force] --yes`.
    #[test]
    fn bare_prune_needs_yes_without_a_terminal() {
        let env = Env::new();
        env.put("a", "x.txt", b"xx\n");
        env.put("a", "y.txt", b"yy\n");
        env.scan_root("a", &["--max-size=1KiB"]);
        std::fs::remove_file(env.dir("a").join("y.txt")).unwrap();
        let data = tree(&env.data());
        let state = tree(&env.state_dir());

        let out = env.run(["--json", "scan", "--prune"]);
        assert_eq!(out.code, Some(1), "{}", out.stderr);
        assert_eq!(out.stdout, "");
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!(
                "cli_scan_roots__bare_prune_confirmation_required_json_envelope",
                env.clean(&out.stderr)
            );
        });
        let e = envelope(&out);
        assert_eq!(e["error"]["rule"], "CONFIRMATION_REQUIRED");
        assert_eq!(e["error"]["details"]["command"], "scan");
        assert_eq!(e["error"]["details"]["declined"], false);
        assert_eq!(tree(&env.data()), data);
        assert_eq!(tree(&env.state_dir()), state);

        let out = env.run(["--json", "scan", "--prune", "--force"]);
        assert_eq!(out.code, Some(1), "{}", out.stderr);
        assert_eq!(
            envelope(&out)["error"]["suggested_command"],
            "hali scan --prune --force --yes"
        );

        // The command it suggests works.
        let out = env.run(["--json", "scan", "--prune", "--yes"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);

        // A bare prune that would forget nothing never asks.
        let out = env.run(["--json", "scan", "--prune"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
    }

    // ---- row 7: roots that are gone -------------------------------------------------------------

    /// The scene of a vanished root: a and b cataloged and remembered, one file of each.
    fn two_roots() -> Env {
        let env = Env::new();
        env.put("a", "a1.txt", b"a one\n");
        env.put("a", "a2.txt", b"a two\n");
        env.put("b", "b1.txt", b"b one\n");
        env.scan_root("a", &[]);
        env.scan_root("b", &[]);
        env
    }

    /// Row 7: A remembered root that is gone, is a file now, or cannot be read is listed in
    /// `unavailable`, its Resources are never pruned and it stays remembered; the other roots
    /// are scanned and pruned as usual.
    #[test]
    fn vanished_root_never_pruned() {
        for how in ["deleted", "a file", "unreadable"] {
            if how == "unreadable" && privileged() {
                continue;
            }
            let env = two_roots();
            let b = env.dir("b");
            let _restore = Restore(b.clone());
            match how {
                "deleted" => std::fs::remove_dir_all(&b).unwrap(),
                "a file" => {
                    std::fs::remove_dir_all(&b).unwrap();
                    std::fs::write(&b, "now a file\n").unwrap();
                }
                _ => std::fs::set_permissions(&b, std::fs::Permissions::from_mode(0)).unwrap(),
            }
            std::fs::remove_file(env.dir("a").join("a2.txt")).unwrap();
            env.put("a", "a3.txt", b"a three\n");
            let roots = env.roots_bytes().unwrap();

            let v = json_out(&env.run(["--json", "scan", "--prune", "--yes"]));
            let unavailable = v["unavailable"].as_array().unwrap();
            assert_eq!(unavailable.len(), 1, "{how}: {v}");
            assert_eq!(unavailable[0]["root"], env.s("b"), "{how}");
            assert!(
                !unavailable[0]["reason"].as_str().unwrap().is_empty(),
                "{how}"
            );
            assert_eq!(v["roots"], json!([env.s("a")]), "{how}");
            assert_eq!(record_paths(&v["added"]), [env.s("a") + "/a3.txt"], "{how}");
            assert_eq!(
                record_paths(&v["pruned"]),
                [env.s("a") + "/a2.txt"],
                "{how}"
            );
            let ttl = env.resources();
            assert!(ttl.contains("b/b1.txt"), "{how}: b's Resource was pruned");
            assert!(!ttl.contains("a/a2.txt"), "{how}");
            assert_eq!(env.roots_bytes().unwrap(), roots, "{how}: still remembered");

            // Text mode names it too, and so does a scan without --prune.
            let out = env.run(["scan"]);
            assert_eq!(out.code, Some(0), "{how}: {}", out.stderr);
            assert!(out.stdout.contains(&env.s("b")), "{how}: {}", out.stdout);
            assert!(env.resources().contains("b/b1.txt"), "{how}");
        }
    }

    /// Row 7: When every remembered root is unavailable the scan still exits 0 and prunes
    /// nothing, even under `--prune --force`.
    #[test]
    fn all_roots_vanished_prunes_nothing() {
        let env = two_roots();
        std::fs::remove_dir_all(env.dir("a")).unwrap();
        std::fs::remove_dir_all(env.dir("b")).unwrap();
        let data = tree(&env.data());
        let v = json_out(&env.run(["--json", "scan", "--prune", "--force"]));
        assert_eq!(v["roots"], json!([]));
        assert_eq!(v["unavailable"].as_array().unwrap().len(), 2);
        assert_eq!(counts8(&v)[6], 0);
        assert_eq!(tree(&env.data()), data);
    }

    // ---- row 8: no roots ------------------------------------------------------------------------

    /// Row 8: With no remembered roots a bare scan exits 0 with empty results and creates
    /// nothing, whether `roots.json` is missing or lists no root.
    #[test]
    fn bare_scan_no_roots() {
        let want = json!({
            "schema_version": 1,
            "roots": [],
            "unavailable": [],
            "counts": {"added": 0, "refreshed": 0, "relinked": 0, "ambiguous": 0,
                "unchanged": 0, "missing": 0, "pruned": 0, "skipped": 0},
            "media_types": {},
            "added": [], "refreshed": [], "relinked": [], "ambiguous": [], "missing": [],
            "pruned": [], "skipped": [], "truncated": false
        });
        for state in ["none", "empty list"] {
            let env = Env::new();
            if state == "empty list" {
                env.set_roots(&[]);
            }
            let before = tree(&env.state_dir());
            assert_eq!(ok_json(&env.run(["--json", "scan"])), want, "{state}");
            assert_eq!(
                ok_json(&env.run(["--json", "scan", "--prune"])),
                want,
                "{state}"
            );
            let out = env.run(["scan"]);
            assert_eq!(out.code, Some(0), "{state}: {}", out.stderr);
            assert!(!env.data().exists(), "{state}: no data dir is created");
            assert_eq!(tree(&env.state_dir()), before, "{state}");
        }
    }

    // ---- row 9: usage errors --------------------------------------------------------------------

    /// Row 9: Scan flags without a root, and `--list` or `--forget-root` with anything else, are
    /// usage errors (exit 2) that write nothing and come before the store is read.
    #[test]
    fn usage_errors() {
        let env = Env::new();
        env.put("a", "x.txt", b"x\n");
        env.scan_root("a", &[]);
        garbage_store(&env.data());
        let data = tree(&env.data());
        let state = tree(&env.state_dir());
        let a = env.s("a");
        let b = env.s("b");

        let cases: Vec<(Vec<&str>, Vec<&str>)> = vec![
            // Scan flags need a root.
            (vec!["--include-hidden"], vec!["--include-hidden"]),
            (vec!["--no-ignore"], vec!["--no-ignore"]),
            (vec!["--exclude=*.tmp"], vec!["--exclude"]),
            (vec!["--max-size=1KiB"], vec!["--max-size"]),
            (vec!["--media-type=text/*"], vec!["--media-type"]),
            (
                vec!["--prune", "--include-hidden"],
                vec!["--include-hidden"],
            ),
            (vec!["--force"], vec!["--force"]),
            // --list stands alone.
            (vec![a.as_str(), "--list"], vec!["--list"]),
            (vec!["--list", "--prune"], vec!["--list", "--prune"]),
            (vec!["--list", "--force"], vec!["--list", "--force"]),
            (
                vec!["--list", "--include-hidden"],
                vec!["--list", "--include-hidden"],
            ),
            (vec!["--list", "--no-ignore"], vec!["--list", "--no-ignore"]),
            (vec!["--list", "--exclude=x"], vec!["--list", "--exclude"]),
            (vec!["--list", "--max-size=1"], vec!["--list", "--max-size"]),
            (
                vec!["--list", "--media-type=text/*"],
                vec!["--list", "--media-type"],
            ),
            (
                vec!["--list", "--forget-root", a.as_str()],
                vec!["--list", "--forget-root"],
            ),
            // --forget-root stands alone.
            (vec!["--forget-root"], vec!["--forget-root"]),
            (
                vec!["--forget-root", a.as_str(), b.as_str()],
                vec!["--forget-root"],
            ),
            (
                vec!["--forget-root", a.as_str(), "--list"],
                vec!["--list", "--forget-root"],
            ),
            (
                vec!["--forget-root", a.as_str(), "--include-hidden"],
                vec!["--forget-root", "--include-hidden"],
            ),
            (
                vec!["--forget-root", a.as_str(), "--no-ignore"],
                vec!["--forget-root", "--no-ignore"],
            ),
            (
                vec!["--forget-root", a.as_str(), "--exclude=x"],
                vec!["--forget-root", "--exclude"],
            ),
            (
                vec!["--forget-root", a.as_str(), "--max-size=1"],
                vec!["--forget-root", "--max-size"],
            ),
            (
                vec!["--forget-root", a.as_str(), "--media-type=text/*"],
                vec!["--forget-root", "--media-type"],
            ),
        ];
        for (rest, any_of) in &cases {
            for json_mode in [false, true] {
                let mut args: Vec<&str> = Vec::new();
                if json_mode {
                    args.push("--json");
                }
                args.push("scan");
                args.extend(rest.iter().copied());
                let out = env.run(&args);
                assert_eq!(out.code, Some(2), "{args:?}: {}", out.stderr);
                assert!(
                    any_of.iter().any(|w| out.stderr.contains(w)),
                    "{args:?} should name one of {any_of:?}: {:?}",
                    out.stderr
                );
                assert_eq!(out.stdout, "", "{args:?}");
                assert_eq!(tree(&env.data()), data, "{args:?}: the store was touched");
                assert_eq!(tree(&env.state_dir()), state, "{args:?}: the state changed");
            }
        }
        // With valid arguments scan reads the store and fails: the store really is unreadable.
        let out = env.run(["scan"]);
        assert_eq!(out.code, Some(3), "{}", out.stderr);
    }

    // ---- row 10: --list --------------------------------------------------------------------------

    /// The list scene: b (the defaults) is remembered before a (every flag but two).
    fn list_scene() -> Env {
        let env = Env::new();
        env.put("a", "x.txt", b"x\n");
        env.put("b", "y.txt", b"y\n");
        env.scan_root("b", &[]);
        env.scan_root(
            "a",
            &[
                "--include-hidden",
                "--exclude=*.tmp",
                "--exclude=a b",
                "--max-size=10B",
                "--media-type=TEXT/*",
            ],
        );
        env
    }

    /// Row 10: `scan --list` prints the remembered roots with their settings, sorted by root.
    #[test]
    fn list_json() {
        let env = list_scene();
        let out = env.run(["--json", "scan", "--list"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!("cli_scan_roots__scan_list_json", env.clean(&out.stdout));
        });
        let v: Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(
            v["roots"],
            json!([
                entry(
                    &env.s("a"),
                    true,
                    false,
                    &["*.tmp", "a b"],
                    Some(10),
                    &["text/*"]
                ),
                plain(&env.s("b")),
            ])
        );
    }

    /// Row 10: Text mode: one line per root, the root first, then the settings that are not the
    /// defaults, tab-separated, as the flags that give them.
    #[test]
    fn list_text() {
        let env = list_scene();
        let out = env.run(["scan", "--list"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        assert_eq!(
            env.clean(&out.stdout),
            "/TMP/a\t--include-hidden\t--exclude=*.tmp\t--exclude=a b\t--max-size=10\t--media-type=text/*\n/TMP/b\n"
        );

        let env = Env::new();
        env.put("a", "x.txt", b"x\n");
        env.scan_root("a", &["--no-ignore"]);
        let out = env.run(["scan", "--list"]);
        assert_eq!(env.clean(&out.stdout), "/TMP/a\t--no-ignore\n");
    }

    /// Row 10: Roots are sorted in byte order (`B` before `a`), however they were added.
    #[test]
    fn list_is_sorted_in_byte_order() {
        let env = Env::new();
        for name in ["b", "a", "B", "a-b"] {
            std::fs::create_dir_all(env.dir(name)).unwrap();
            env.scan_root(name, &[]);
        }
        let v = ok_json(&env.run(["--json", "scan", "--list"]));
        let roots: Vec<String> = v["roots"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["root"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(roots, [env.s("B"), env.s("a"), env.s("a-b"), env.s("b")]);
    }

    /// Row 10: With no roots `--list` is an empty list, in JSON and in text, and it changes
    /// nothing.
    #[test]
    fn list_empty() {
        let env = Env::new();
        let v = ok_json(&env.run(["--json", "scan", "--list"]));
        assert_eq!(v, json!({"schema_version": 1, "roots": []}));
        let out = env.run(["scan", "--list"]);
        assert_eq!(
            (out.code, out.stdout.as_str(), out.stderr.as_str()),
            (Some(0), "", "")
        );
        assert!(!env.base.join("state").exists());
        assert!(!env.data().exists());

        env.set_roots(&[]);
        let v = ok_json(&env.run(["--json", "scan", "--list"]));
        assert_eq!(v, json!({"schema_version": 1, "roots": []}));
    }

    /// Row 10: `--list` is read-only: neither `roots.json` nor the store changes.
    #[test]
    fn list_writes_nothing() {
        let env = list_scene();
        let state = tree(&env.state_dir());
        let data = tree(&env.data());
        let mtime = std::fs::metadata(env.roots_file())
            .unwrap()
            .modified()
            .unwrap();
        for args in [vec!["--json", "scan", "--list"], vec!["scan", "--list"]] {
            let out = env.run(&args);
            assert_eq!(out.code, Some(0), "{args:?}: {}", out.stderr);
        }
        assert_eq!(tree(&env.state_dir()), state);
        assert_eq!(tree(&env.data()), data);
        assert_eq!(
            std::fs::metadata(env.roots_file())
                .unwrap()
                .modified()
                .unwrap(),
            mtime
        );
    }

    // ---- row 11: --forget-root ------------------------------------------------------------------

    /// Row 11: `--forget-root` stops remembering a root and touches nothing else: `resources.ttl`
    /// is byte-identical, `find` still counts every Resource, and it never asks.
    #[test]
    fn forget_root_keeps_resources() {
        let env = Env::new();
        env.put("a", "x.txt", b"x\n");
        env.put("a", "y.txt", b"yy\n");
        env.put("b", "z.txt", b"zzz\n");
        env.scan_root("a", &["--exclude=*.tmp", "--max-size=10B"]);
        env.scan_root("b", &[]);
        let ttl = std::fs::read(env.data().join("resources.ttl")).unwrap();
        let data = tree(&env.data());
        let total = env.find_total();
        assert_eq!(total, 3);

        let out = env.run(["--json", "scan", "--forget-root", env.s("a").as_str()]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!("cli_scan_roots__forget_root_json", env.clean(&out.stdout));
        });
        assert_eq!(
            String::from_utf8(env.roots_bytes().unwrap()).unwrap(),
            roots_text(&[plain(&env.s("b"))])
        );
        assert_eq!(
            std::fs::read(env.data().join("resources.ttl")).unwrap(),
            ttl
        );
        assert_eq!(tree(&env.data()), data);
        assert_eq!(env.find_total(), total);
        let v = ok_json(&env.run(["--json", "scan", "--list"]));
        assert_eq!(v["roots"], json!([plain(&env.s("b"))]));

        // A forgotten root is no longer scanned: its Resources are not pruned or refreshed.
        std::fs::remove_file(env.dir("a").join("x.txt")).unwrap();
        let v = ok_json(&env.run(["--json", "scan", "--prune", "--yes"]));
        assert_eq!(v["roots"], json!([env.s("b")]));
        assert_eq!(counts8(&v)[6], 0);
        assert_eq!(env.find_total(), total);

        // Text mode works the same and never asks either.
        let out = env.run(["scan", "--forget-root", env.s("b").as_str()]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        let v = ok_json(&env.run(["--json", "scan", "--list"]));
        assert_eq!(v["roots"], json!([]), "the last root can be forgotten too");
        assert_eq!(env.find_total(), total);
    }

    /// Row 11: `--dry-run` shows what would be forgotten and writes nothing.
    #[test]
    fn forget_root_dry_run() {
        let env = Env::new();
        env.put("a", "x.txt", b"x\n");
        env.scan_root("a", &["--exclude=*.tmp", "--max-size=10B"]);
        let state = tree(&env.state_dir());
        let data = tree(&env.data());
        let out = env.run([
            "--json",
            "--dry-run",
            "scan",
            "--forget-root",
            env.s("a").as_str(),
        ]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!(
                "cli_dry_run__scan_forget_root_dry_run_json",
                env.clean(&out.stdout)
            );
        });
        assert_eq!(tree(&env.state_dir()), state);
        assert_eq!(tree(&env.data()), data);
    }

    /// Row 11: The root given is matched on its real path: relative, with dots or a symlink.
    #[test]
    fn forget_root_matches_the_real_path() {
        for arg in ["a", "./b/../a/", "link"] {
            let env = Env::new();
            env.put("a", "x.txt", b"x\n");
            env.scan_root("a", &[]);
            symlink(env.dir("a"), env.base.join("link")).unwrap();
            let out = env.run_in(&env.base, ["--json", "scan", "--forget-root", arg]);
            let v = ok_json(&out);
            assert_eq!(v["root"]["root"], env.s("a"), "{arg}");
            let v = ok_json(&env.run(["--json", "scan", "--list"]));
            assert_eq!(v["roots"], json!([]), "{arg}");
        }
    }

    // ---- row 12: unknown and deleted roots --------------------------------------------------------

    /// Row 12: A root that is not remembered, existing or not, is a usage error (exit 2) with
    /// the hint `hali scan --list`, and nothing changes.
    #[test]
    fn forget_root_unknown() {
        let env = Env::new();
        env.put("a", "x.txt", b"x\n");
        env.scan_root("a", &[]);
        let state = tree(&env.state_dir());
        let data = tree(&env.data());
        for arg in [
            env.s("b"),
            env.s("never/was"),
            env.s("a/sub"),
            "a".to_string(),
        ] {
            std::fs::create_dir_all(env.dir("a/sub")).unwrap();
            for json_mode in [false, true] {
                let mut args = vec![];
                if json_mode {
                    args.push("--json");
                }
                args.extend(["scan", "--forget-root", arg.as_str()]);
                // `a` is relative to the temp dir's parent here, which is nothing remembered.
                let out = env.exec(Some(&env.base.join("home")), &[], &[], 60, b"", &args);
                assert_eq!(out.code, Some(2), "{args:?}: {}", out.stderr);
                assert!(
                    out.stderr.contains("hali scan --list"),
                    "{args:?}: {:?}",
                    out.stderr
                );
                assert_eq!(out.stdout, "", "{args:?}");
                assert_eq!(tree(&env.state_dir()), state, "{args:?}");
                assert_eq!(tree(&env.data()), data, "{args:?}");
            }
        }
        // With nothing remembered at all it is the same error.
        let env = Env::new();
        let out = env.run(["scan", "--forget-root", env.s("a").as_str()]);
        assert_eq!(out.code, Some(2), "{}", out.stderr);
        assert!(out.stderr.contains("hali scan --list"), "{}", out.stderr);
    }

    /// Row 12: A root whose directory is gone can still be forgotten, by the path it was
    /// remembered under.
    #[test]
    fn forget_root_deleted_dir() {
        let env = Env::new();
        env.put("a", "x.txt", b"x\n");
        env.put("b", "y.txt", b"y\n");
        env.scan_root("a", &["--max-size=1KiB"]);
        env.scan_root("b", &[]);
        let ttl = std::fs::read(env.data().join("resources.ttl")).unwrap();
        std::fs::remove_dir_all(env.dir("a")).unwrap();
        std::fs::remove_dir_all(env.dir("b")).unwrap();

        // Given absolute.
        let v = ok_json(&env.run(["--json", "scan", "--forget-root", env.s("a").as_str()]));
        assert_eq!(
            v["root"],
            entry(&env.s("a"), false, false, &[], Some(1024), &[])
        );
        // Given relative: made absolute against the current directory.
        let v = ok_json(&env.run_in(&env.base, ["--json", "scan", "--forget-root", "b"]));
        assert_eq!(v["root"], plain(&env.s("b")));
        let v = ok_json(&env.run(["--json", "scan", "--list"]));
        assert_eq!(v["roots"], json!([]));
        assert_eq!(
            std::fs::read(env.data().join("resources.ttl")).unwrap(),
            ttl
        );
    }

    // ---- row 14: relink --------------------------------------------------------------------------

    /// The relink scene: `scan a` remembers a; a/f.txt then moves to a/sub/f.txt (or elsewhere).
    fn relink_scene() -> Env {
        let env = Env::new();
        env.put("a", "f.txt", b"hello\n");
        env.scan_root("a", &[]);
        env
    }

    fn mv(env: &Env, from: &str, to: &str) {
        let to = env.base.join(to);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::rename(env.base.join(from), to).unwrap();
    }

    /// Row 14: `relink` with no `--root` searches the remembered roots (`files.dir` is not a
    /// directory here, and skipped silently).
    #[test]
    fn default_roots_include_remembered() {
        let env = relink_scene();
        mv(&env, "a/f.txt", "a/sub/f.txt");
        let v = ok_json(&env.run(["--json", "relink"]));
        assert_eq!(v["roots"], json!([env.s("a")]));
        assert_eq!(v["counts"]["relinked"], 1);
        assert_eq!(
            v["relinked"][0]["resource"]["path"],
            env.s("a") + "/sub/f.txt"
        );
    }

    /// Row 14: `files.dir` comes first, then the remembered roots in byte order, without repeats.
    #[test]
    fn default_roots_order_and_repeats() {
        let env = Env::new();
        for name in ["docs", "b", "a"] {
            env.put(name, "x.txt", format!("{name}\n").as_bytes());
            env.scan_root(name, &[]);
        }
        env.config(&format!("[files]\ndir = \"{}\"\n", env.s("docs")));
        let v = ok_json(&env.run(["--json", "relink"]));
        assert_eq!(
            v["roots"],
            json!([env.s("docs"), env.s("a"), env.s("b")]),
            "files.dir first, once, then the rest in byte order"
        );

        // files.dir that is not remembered still comes first.
        env.config(&format!("[files]\ndir = \"{}\"\n", env.s("home")));
        let v = ok_json(&env.run(["--json", "relink"]));
        assert_eq!(
            v["roots"],
            json!([env.s("home"), env.s("a"), env.s("b"), env.s("docs")])
        );
    }

    /// Row 14: `--root` replaces every default, the remembered roots included.
    #[test]
    fn explicit_root_replaces_remembered() {
        let env = relink_scene();
        assert!(env.roots_file().is_file(), "the scan of a remembered it");
        mv(&env, "a/f.txt", "a/sub/f.txt");
        let v = ok_json(&env.run(["--json", "relink", "--root", env.s("b").as_str()]));
        assert_eq!(v["roots"], json!([env.s("b")]));
        assert_eq!(v["counts"]["relinked"], 0);
        assert_eq!(v["counts"]["unmatched"], 1);
    }

    /// Row 14: A remembered root that is gone, is not a directory or is the data dir is skipped
    /// silently.
    #[test]
    fn unusable_remembered_roots_are_skipped_silently() {
        let env = relink_scene();
        std::fs::create_dir_all(env.data()).unwrap();
        std::fs::create_dir_all(env.dir("gone")).unwrap();
        std::fs::write(env.dir("file"), "not a dir\n").unwrap();
        let roots = [
            plain(&env.s("a")),
            plain(&env.s("data")),
            plain(&env.s("file")),
            plain(&env.s("gone")),
        ];
        std::fs::remove_dir(env.dir("gone")).unwrap();
        env.set_roots(&roots);
        mv(&env, "a/f.txt", "a/sub/f.txt");
        let out = env.run(["--json", "relink"]);
        assert_eq!(out.stderr, "", "silently");
        let v = ok_json(&out);
        assert_eq!(v["roots"], json!([env.s("a")]));
        assert_eq!(v["counts"]["relinked"], 1);
    }

    /// Row 14: Relink walks with its own settings, not the ones a root was remembered with: a
    /// root remembered with `--include-hidden` is still searched without hidden names.
    #[test]
    fn relink_ignores_the_remembered_settings() {
        let env = Env::new();
        env.put("a", "f.txt", b"hello\n");
        env.scan_root("a", &["--include-hidden"]);
        mv(&env, "a/f.txt", "a/.hidden/f.txt");
        let v = ok_json(&env.run(["--json", "relink"]));
        assert_eq!(v["roots"], json!([env.s("a")]));
        assert_eq!(
            v["counts"]["relinked"], 0,
            "hidden names are not walked: {v}"
        );
        assert_eq!(v["counts"]["unmatched"], 1);
    }

    // ---- row 15: a roots.json that cannot be used ------------------------------------------------

    /// Puts a roots.json of the kind `how` in the state dir; returns its bytes if a file.
    fn plant(env: &Env, how: &str) {
        std::fs::create_dir_all(env.state_dir()).unwrap();
        let file = env.roots_file();
        let valid = roots_text(&[plain(&env.s("a"))]);
        match how {
            "not json" => std::fs::write(&file, "not json\n").unwrap(),
            "truncated" => std::fs::write(&file, &valid[..valid.len() / 2]).unwrap(),
            "version 2" => {
                std::fs::write(&file, valid.replace("\"version\":1", "\"version\":2")).unwrap()
            }
            "relative root" => {
                std::fs::write(&file, valid.replace(&env.s("a"), "relative/dir")).unwrap()
            }
            "empty root" => std::fs::write(&file, valid.replace(&env.s("a"), "")).unwrap(),
            "oversized" => {
                let mut text = valid.trim_end().to_string();
                text.push_str(&" ".repeat((1 << 20) + 1));
                std::fs::write(&file, text).unwrap();
            }
            "non-utf8" => std::fs::write(
                &file,
                b"{\"version\":1,\"roots\":[{\"root\":\"/\xff\",\"include_hidden\":false,\
                  \"no_ignore\":false,\"exclude\":[],\"max_size\":null,\"media_types\":[]}]}\n",
            )
            .unwrap(),
            "directory" => std::fs::create_dir(&file).unwrap(),
            "symlink" => {
                let real = env.base.join("real-roots.json");
                std::fs::write(&real, valid).unwrap();
                symlink(&real, &file).unwrap();
            }
            "fifo" => {
                let c = CString::new(file.as_os_str().as_bytes()).unwrap();
                // SAFETY: `c` is a valid NUL-terminated path.
                assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
            }
            other => panic!("unknown kind {other}"),
        }
    }

    /// Row 15: A `roots.json` that cannot be read or parsed, is not a regular file, is over
    /// 1 MiB, or holds a relative or non-UTF-8 path makes a scan, `--list` and a default-root
    /// relink fail with exit 3 (`STORE_IO` or `STORE_UNPARSEABLE`) naming the file, and nothing
    /// is written.
    #[test]
    fn corrupt_state_file_exits_3() {
        for how in [
            "not json",
            "truncated",
            "version 2",
            "relative root",
            "empty root",
            "oversized",
            "non-utf8",
            "directory",
            "symlink",
            "fifo",
        ] {
            let env = Env::new();
            env.put("a", "x.txt", b"x\n");
            plant(&env, how);
            let file = env.roots_file();
            let state = tree(&env.state_dir());
            let a = env.s("a");
            let commands: Vec<Vec<&str>> = vec![
                vec!["--json", "scan"],
                vec!["--json", "scan", "--list"],
                vec!["--json", "relink"],
                vec!["--json", "scan", a.as_str()],
                vec!["--json", "scan", "--forget-root", a.as_str()],
            ];
            for args in commands {
                let out = env.exec(None, &[], &[], 20, b"", &args);
                assert_eq!(out.code, Some(3), "{how} {args:?}: {}", out.stderr);
                assert_eq!(out.stdout, "", "{how} {args:?}");
                let e = envelope(&out);
                let rule = e["error"]["rule"].as_str().unwrap();
                assert!(
                    rule == "STORE_IO" || rule == "STORE_UNPARSEABLE",
                    "{how} {args:?}: {rule}"
                );
                assert!(
                    out.stderr.contains(file.to_str().unwrap()),
                    "{how} {args:?} must name {}: {}",
                    file.display(),
                    out.stderr
                );
                assert!(
                    !env.data().exists(),
                    "{how} {args:?}: the store was written"
                );
                assert_eq!(tree(&env.state_dir()), state, "{how} {args:?}");
            }
        }
    }

    /// Row 15: A state dir that cannot be resolved (no `XDG_STATE_HOME`, no `HOME`) is
    /// `STORE_IO`, exit 3, before the store is read: an unreadable store does not change the
    /// error.
    #[test]
    fn unresolvable_state_dir_exits_3() {
        let env = Env::new();
        env.put("a", "x.txt", b"x\n");
        garbage_store(&env.data());
        let data = tree(&env.data());
        let a = env.s("a");
        let commands: Vec<Vec<&str>> = vec![
            vec!["--json", "scan"],
            vec!["--json", "scan", "--list"],
            vec!["--json", "scan", a.as_str()],
            vec!["--json", "scan", "--forget-root", a.as_str()],
            vec!["--json", "relink"],
        ];
        for args in commands {
            let out = env.exec(None, &[], &["HOME", "XDG_STATE_HOME"], 20, b"", &args);
            assert_eq!(out.code, Some(3), "{args:?}: {}", out.stderr);
            assert_eq!(out.stdout, "", "{args:?}");
            assert_eq!(envelope(&out)["error"]["rule"], "STORE_IO", "{args:?}");
            assert_eq!(tree(&env.data()), data, "{args:?}");
        }
    }

    // ---- row 18: the state dir is never walked --------------------------------------------------

    /// Row 18: A scan whose root holds the state dir never catalogs `roots.json` or anything in
    /// the state dir, with the dir named by `XDG_STATE_HOME` or under `$HOME`, in a scan of the
    /// root and in a bare scan that remembered it.
    #[test]
    fn state_dir_pruned_from_walk() {
        for xdg in [true, false] {
            let env = Env::new();
            env.put("a", "x.txt", b"x\n");
            let extra: Vec<(&str, &str)> = if xdg {
                vec![]
            } else {
                vec![("XDG_STATE_HOME", "")]
            };
            let state_dir = if xdg {
                env.state_dir()
            } else {
                env.base.join("home/.local/state/hali")
            };
            let run = |args: &[&str]| {
                let seed = env.seed.get();
                env.seed.set(seed + 1);
                let seed = seed.to_string();
                let mut vars = extra.clone();
                vars.push(("HALI_TEST_ID_SEED", seed.as_str()));
                let out = env.exec(None, &vars, &[], 60, b"", args);
                assert_eq!(out.code, Some(0), "xdg {xdg} {args:?}: {}", out.stderr);
                out
            };
            // Create the state dir first, so a walk has something to find in it.
            run(&["scan", env.s("a").as_str()]);
            assert!(state_dir.join("roots.json").is_file());
            std::fs::write(state_dir.join("notes.txt"), "stray\n").unwrap();

            let base = env.base.to_str().unwrap().to_string();
            let v: Value = serde_json::from_str(
                &run(&["--json", "scan", base.as_str(), "--include-hidden"]).stdout,
            )
            .unwrap();
            let skipped: Vec<&str> = v["skipped"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s["path"].as_str().unwrap())
                .collect();
            let under_state = |p: &str| p.starts_with(state_dir.to_str().unwrap());
            assert!(
                !record_paths(&v["added"]).iter().any(|p| under_state(p)),
                "xdg {xdg}: {v}"
            );
            assert!(!skipped.iter().any(|p| under_state(p)), "xdg {xdg}: {v}");

            // The root holding the state dir is remembered now: a bare scan walks it too.
            let v: Value = serde_json::from_str(&run(&["--json", "scan"]).stdout).unwrap();
            assert!(
                !record_paths(&v["added"]).iter().any(|p| under_state(p)),
                "xdg {xdg}: {v}"
            );
            let ttl = env.resources();
            assert!(
                !ttl.contains(state_dir.to_str().unwrap()),
                "xdg {xdg}: something under the state dir was cataloged"
            );
            assert!(!ttl.contains("roots.json") && !ttl.contains("notes.txt"));
        }
    }

    /// Row 18: A relink never searches the state dir, with `--root` over its parent or with the
    /// parent remembered: a file that has the content of a Missing Resource and sits there is no
    /// move.
    #[test]
    fn relink_never_searches_the_state_dir() {
        let env = Env::new();
        let path = env.put("a", "m.txt", b"mover\n");
        env.setup(["add", path.to_str().unwrap()]);
        env.scan_root("b", &[]);
        std::fs::remove_file(&path).unwrap();
        std::fs::write(env.state_dir().join("copy.txt"), "mover\n").unwrap();
        let base = env.base.to_str().unwrap().to_string();

        let v = ok_json(&env.run(["--json", "relink", "--root", base.as_str()]));
        assert_eq!(v["counts"]["relinked"], 0, "{v}");
        assert_eq!(v["counts"]["unmatched"], 1, "{v}");

        // The parent remembered (remembering an empty scan of it by hand), no --root.
        env.set_roots(&[plain(&base)]);
        std::fs::write(env.state_dir().join("copy.txt"), "mover\n").unwrap();
        let v = ok_json(&env.run(["--json", "relink"]));
        assert_eq!(v["roots"], json!([base]));
        assert_eq!(v["counts"]["relinked"], 0, "{v}");
        assert_eq!(v["counts"]["unmatched"], 1, "{v}");
    }

    // ---- rows 19-23: review findings ------------------------------------------------------------

    /// Row 19: Nested remembered roots with different settings: a file the outer root rules out
    /// (its media types, or its size limit) is still added by a bare scan, as `scan <inner>` alone
    /// would add it; the outer root's own files stay out.
    #[test]
    fn bare_scan_nested_roots_add_what_the_inner_root_would() {
        for flags in [vec!["--media-type", "image/*"], vec!["--max-size=1"]] {
            let env = Env::new();
            env.put("a/b", "old.txt", b"old file\n");
            env.scan_root("a", &flags);
            env.scan_root("a/b", &[]);
            env.put("a/b", "doc.txt", b"a document\n");
            env.put("a", "top.txt", b"top level\n");
            let alone = json_out(&env.run(["--json", "--dry-run", "scan", env.s("a/b").as_str()]));
            assert_eq!(record_paths(&alone["added"]), [env.s("a/b") + "/doc.txt"]);

            let v = json_out(&env.run(["--json", "scan"]));
            assert_eq!(
                record_paths(&v["added"]),
                record_paths(&alone["added"]),
                "{flags:?}: {v}"
            );
            assert_eq!(v["roots"], json!([env.s("a"), env.s("a/b")]), "{flags:?}");
            assert!(env.resources().contains("doc.txt"), "{flags:?}");
            assert!(!env.resources().contains("top.txt"), "{flags:?}");
            // One outcome per path: doc.txt is added, so it is not also skipped.
            for s in v["skipped"].as_array().unwrap() {
                let path = s["path"].as_str().unwrap();
                assert!(!path.ends_with("/doc.txt"), "{flags:?}: {v}");
            }
        }
    }

    /// Row 20: The state dir, or a directory inside it, is not a scan root: `scan` is a usage
    /// error that catalogs nothing.
    #[test]
    fn scan_refuses_the_state_dir_as_a_root() {
        let env = Env::new();
        env.set_roots(&[]);
        let sub = env.state_dir().join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("s.txt"), "stray\n").unwrap();
        std::fs::write(env.state_dir().join("t.txt"), "stray\n").unwrap();
        let roots = env.roots_bytes().unwrap();
        for root in [env.state_dir(), sub] {
            let out = env.run(["--json", "scan", root.to_str().unwrap()]);
            assert_eq!(out.code, Some(2), "{}: {}", root.display(), out.stderr);
            assert_eq!(out.stdout, "");
            assert!(
                out.stderr.contains("state dir or inside it"),
                "{}",
                out.stderr
            );
            assert!(!env.data().join("resources.ttl").exists());
            assert_eq!(env.roots_bytes().unwrap(), roots);
        }
    }

    /// Row 20: A remembered root that is the state dir (or inside it), written by hand, is
    /// `unavailable` in a bare scan and never walked.
    #[test]
    fn bare_scan_never_walks_a_state_dir_root() {
        let env = Env::new();
        env.put("a", "x.txt", b"x\n");
        env.set_roots(&[plain(&env.s("a"))]);
        let sub = env.state_dir().join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("s.txt"), "stray\n").unwrap();
        std::fs::write(env.state_dir().join("t.txt"), "stray\n").unwrap();
        let state = env.state_dir().to_str().unwrap().to_string();
        let inside = sub.to_str().unwrap().to_string();
        env.set_roots(&[plain(&env.s("a")), plain(&state), plain(&inside)]);

        let v = json_out(&env.run(["--json", "scan"]));
        let mut unavailable: Vec<&str> = v["unavailable"]
            .as_array()
            .unwrap()
            .iter()
            .map(|u| u["root"].as_str().unwrap())
            .collect();
        unavailable.sort();
        assert_eq!(unavailable, [state.as_str(), inside.as_str()], "{v}");
        assert_eq!(v["roots"], json!([env.s("a")]), "{v}");
        assert_eq!(record_paths(&v["added"]), [env.s("a") + "/x.txt"], "{v}");
        let ttl = env.resources();
        assert!(!ttl.contains("t.txt") && !ttl.contains("s.txt"));
    }

    /// Row 20: `relink --root` refuses the state dir (exit 2); the default roots skip a
    /// remembered root that is the state dir, silently.
    #[test]
    fn relink_refuses_the_state_dir_as_a_root() {
        let env = Env::new();
        env.put("a", "x.txt", b"x\n");
        env.scan_root("a", &[]);
        let sub = env.state_dir().join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        for root in [env.state_dir(), sub] {
            let out = env.run(["--json", "relink", "--root", root.to_str().unwrap()]);
            assert_eq!(out.code, Some(2), "{}: {}", root.display(), out.stderr);
            assert!(
                out.stderr.contains("state dir or inside it"),
                "{}",
                out.stderr
            );
        }
        let state = env.state_dir().to_str().unwrap().to_string();
        env.set_roots(&[plain(&env.s("a")), plain(&state)]);
        let out = env.run(["--json", "relink"]);
        let v = json_out(&out);
        assert_eq!(v["roots"], json!([env.s("a")]), "{v}");
        assert_eq!(out.stderr, "");
    }

    /// Row 21: When the state dir cannot be written, a scan exits 3 `STORE_IO` and the store is
    /// not saved (the state dir is checked before the store is written), with or without a
    /// prune.
    #[test]
    fn unwritable_state_dir_leaves_the_store_alone() {
        if privileged() {
            return;
        }
        for prune in [false, true] {
            let env = Env::new();
            env.put("a", "x.txt", b"x\n");
            env.scan_root("a", &[]);
            env.put("a", "y.txt", b"y\n");
            if prune {
                std::fs::remove_file(env.dir("a").join("x.txt")).unwrap();
            }
            let ttl = std::fs::read(env.data().join("resources.ttl")).unwrap();
            let roots = env.roots_bytes().unwrap();
            let _restore = Restore(env.state_dir());
            std::fs::set_permissions(env.state_dir(), std::fs::Permissions::from_mode(0o500))
                .unwrap();
            let mut args = vec!["--json", "scan"];
            let a = env.s("a");
            args.push(a.as_str());
            if prune {
                args.extend(["--prune", "--yes"]);
            }
            let out = env.run(args);
            assert_eq!(out.code, Some(3), "prune {prune}: {}", out.stderr);
            assert_eq!(envelope(&out)["error"]["rule"], "STORE_IO");
            assert_eq!(
                std::fs::read(env.data().join("resources.ttl")).unwrap(),
                ttl,
                "prune {prune}"
            );
            assert_eq!(env.roots_bytes().unwrap(), roots);
        }
    }

    /// Row 22: `--forget-root <root>/` with a trailing slash works on a deleted root.
    #[test]
    fn forget_root_with_a_trailing_slash() {
        let env = Env::new();
        env.put("a", "x.txt", b"x\n");
        env.scan_root("a", &[]);
        std::fs::remove_dir_all(env.dir("a")).unwrap();
        let arg = format!("{}/", env.s("a"));
        let v = ok_json(&env.run(["--json", "scan", "--forget-root", arg.as_str()]));
        assert_eq!(v["root"], plain(&env.s("a")));
        let v = ok_json(&env.run(["--json", "scan", "--list"]));
        assert_eq!(v["roots"], json!([]));
    }

    /// Row 23: A bare scan lists each real root once, in byte order of the real paths: a
    /// remembered root that is a symlink to another remembered root is not scanned twice, and
    /// sorts by where it points.
    #[test]
    fn bare_scan_roots_are_deduplicated_and_in_real_path_order() {
        let env = Env::new();
        env.put("a", "a.txt", b"alpha\n");
        env.put("b", "b.txt", b"beta\n");
        let link = env.dir("0link");
        symlink(env.dir("b"), &link).unwrap();
        env.set_roots(&[
            plain(&env.s("0link")),
            plain(&env.s("a")),
            plain(&env.s("b")),
        ]);
        let v = json_out(&env.run(["--json", "scan"]));
        assert_eq!(v["roots"], json!([env.s("a"), env.s("b")]), "{v}");
        assert_eq!(v["counts"]["added"], 2, "{v}");
    }
}
