//! Goal tests for the directory walker behind `hali scan` (#131): `hali::storage::walk`.
//!
//! Assumed API (the contract in #131 fixes the behaviour; these names and types are the
//! test-author's choice and the implementer must provide exactly them):
//!
//! ```text
//! hali::storage::walk::WalkOptions {
//!     pub include_hidden: bool, pub no_ignore: bool,
//!     pub excludes: Vec<String>, pub prune: Vec<PathBuf>,   // prune: the data dir (absolute)
//! }
//! WalkOptions::new(include_hidden: bool, no_ignore: bool, excludes: Vec<String>,
//!                  prune: Vec<PathBuf>) -> Result<WalkOptions, String>
//!     // Err(the offending pattern) for an invalid glob, one starting with `!`, or an empty or
//!     // whitespace-only one
//! hali::storage::walk::walk(root: &Path, opts: &WalkOptions) -> impl Iterator<Item = Walked>
//! #[derive(Debug, Clone, PartialEq, Eq)]
//! enum Walked {
//!     File { path: String /* real, UTF-8 */, byte_size: u64, mtime: std::time::SystemTime },
//!     Skipped { path: String /* lossy when not UTF-8 */, reason: SkipReason },
//! }
//! #[derive(Debug, Clone, Copy, PartialEq, Eq)]
//! enum SkipReason { TooLarge, Unreadable, NotRegular, NonUtf8, OutsideRoot, BadIgnoreFile }
//! ```
//!
//! `root` is an absolute, canonical directory (the tests canonicalize their temp dirs).
//! A `Skipped` for a symlink carries the symlink's own path; a `Skipped { BadIgnoreFile }`
//! carries the ignore file's path; a `Skipped` for an unreadable directory carries the
//! directory. Entries come depth first, each directory's entries sorted by file name
//! (byte order); a directory symlink is neither followed nor reported, and a file symlink
//! whose real path is inside the root is silent (the real file is walked directly).
//! The walker never opens a regular file other than an ignore-file candidate (readability is
//! `inspect`'s business in #22), so a mode-000 file is yielded as a `File`.
//!
//! Tests that need a memory cap run the walk in a child process (see `run_capped`): RLIMIT_AS
//! is per process, so setting it in a thread of the test binary would also cap every other test.
//!
//! Isolation: `ignore` finds the global gitignore through HOME, XDG_CONFIG_HOME and
//! GIT_CONFIG_GLOBAL/GIT_CONFIG_SYSTEM, which are process-global, so every test builds its
//! tree in a `Sandbox` that holds one static mutex while it points those variables at its own
//! temp dirs and restores them afterwards. Trees live under the system temp dir, not
//! `CARGO_TARGET_TMPDIR`: that is inside the repository checkout, whose `.gitignore` (`/target`)
//! and `.git` would reach the walk through "the root's parents" and ignore the whole tree, and
//! the long checkout path would overflow the 108-byte limit of a socket path. Nothing here
//! runs `git`; a repository is a `.git` directory holding a `HEAD` file.

#[cfg(feature = "goal")]
mod goal {
    use hali::storage::walk::{SkipReason, WalkOptions, Walked, walk};
    use std::ffi::{OsStr, OsString};
    use std::fs;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
    use std::os::unix::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::sync::{Mutex, MutexGuard, mpsc};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    static ENV: Mutex<()> = Mutex::new(());

    const VARS: [&str; 4] = [
        "HOME",
        "XDG_CONFIG_HOME",
        "GIT_CONFIG_GLOBAL",
        "GIT_CONFIG_SYSTEM",
    ];

    /// A sandbox, or return from the test (it printed why).
    macro_rules! sandbox {
        () => {
            match Sandbox::new() {
                Some(sb) => sb,
                None => return,
            }
        };
    }

    struct Sandbox {
        dir: tempfile::TempDir,
        root: PathBuf,
        home: PathBuf,
        xdg: PathBuf,
        saved: Vec<(&'static str, Option<OsString>)>,
        _guard: MutexGuard<'static, ()>,
    }

    impl Sandbox {
        /// `<tmp>/root` (the tree), `<tmp>/home` (HOME), `<tmp>/xdg` (XDG_CONFIG_HOME).
        ///
        /// `None` (after a loud NOTE) when an ancestor of the sandbox holds an ignore file, a
        /// `.git` or a `.jj` (a repository marker too: it switches `.gitignore` on): `ignore`
        /// reads those through "the root's parents", so the expectations of a test would not
        /// hold, and a failure would be about the machine, not the walker.
        fn new() -> Option<Sandbox> {
            let guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
            let dir = tempfile::Builder::new()
                .prefix("hali-walk-")
                .tempdir()
                .unwrap();
            let base = dir.path().canonicalize().unwrap();
            for ancestor in base.ancestors().skip(1) {
                for name in [".gitignore", ".ignore", ".haliignore", ".git", ".jj"] {
                    let found = ancestor.join(name);
                    if fs::symlink_metadata(&found).is_ok() {
                        eprintln!(
                            "NOTE: SKIPPED: {found:?} exists above the sandbox {base:?}; the \
                             walk would honour it as a parent's ignore file or repository, so \
                             this test proves nothing here. Set TMPDIR to a clean directory."
                        );
                        return None;
                    }
                }
            }
            let (root, home, xdg) = (base.join("root"), base.join("home"), base.join("xdg"));
            for d in [&root, &home, &xdg] {
                fs::create_dir(d).unwrap();
            }
            let saved = VARS.iter().map(|v| (*v, std::env::var_os(v))).collect();
            // SAFETY: every test that reads or writes the environment holds `ENV`.
            unsafe {
                std::env::set_var("HOME", &home);
                std::env::set_var("XDG_CONFIG_HOME", &xdg);
                std::env::set_var("GIT_CONFIG_GLOBAL", home.join(".gitconfig"));
                std::env::set_var("GIT_CONFIG_SYSTEM", base.join("no-system-gitconfig"));
            }
            Some(Sandbox {
                dir,
                root,
                home,
                xdg,
                saved,
                _guard: guard,
            })
        }

        fn base(&self) -> PathBuf {
            self.dir.path().canonicalize().unwrap()
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            for (var, old) in &self.saved {
                // SAFETY: `ENV` is still held (`_guard` drops after this).
                unsafe {
                    match old {
                        Some(v) => std::env::set_var(var, v),
                        None => std::env::remove_var(var),
                    }
                }
            }
        }
    }

    fn write(base: &Path, rel: &str, content: &str) {
        let path = base.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    /// A repository skeleton: a `.git` directory with a `HEAD` file.
    fn make_repo(dir: &Path) {
        fs::create_dir_all(dir.join(".git/info")).unwrap();
        fs::write(dir.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    }

    fn opts() -> WalkOptions {
        WalkOptions::new(false, false, vec![], vec![])
            .ok()
            .expect("default options are valid")
    }

    /// The whole walk, in a thread with a 30 s timeout: a walker that opens a FIFO hangs.
    fn run(root: &Path, opts: WalkOptions) -> Vec<Walked> {
        let (tx, rx) = mpsc::channel();
        let root = root.to_path_buf();
        std::thread::spawn(move || {
            let all: Vec<Walked> = walk(&root, &opts).collect();
            let _ = tx.send(all);
        });
        rx.recv_timeout(Duration::from_secs(30))
            .expect("the walk did not finish within 30 s (it hung, or panicked)")
    }

    fn rel(root: &Path, path: &str) -> String {
        let prefix = format!("{}/", root.to_str().unwrap());
        path.strip_prefix(&prefix)
            .unwrap_or_else(|| panic!("{path:?} is not under {prefix:?}"))
            .to_string()
    }

    /// The yielded files, as paths relative to the root, in yielded order.
    fn files(root: &Path, all: &[Walked]) -> Vec<String> {
        all.iter()
            .filter_map(|w| match w {
                Walked::File { path, .. } => Some(rel(root, path)),
                Walked::Skipped { .. } => None,
            })
            .collect()
    }

    /// The skipped entries, as (relative path, reason), sorted.
    fn skipped(root: &Path, all: &[Walked]) -> Vec<(String, SkipReason)> {
        let mut out: Vec<(String, SkipReason)> = all
            .iter()
            .filter_map(|w| match w {
                Walked::Skipped { path, reason } => Some((rel(root, path), *reason)),
                Walked::File { .. } => None,
            })
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    fn sorted(mut v: Vec<String>) -> Vec<String> {
        v.sort();
        v
    }

    fn strs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// Walk the sandbox root with `opts`; its files sorted, and the skipped entries.
    fn outcome(sb: &Sandbox, opts: WalkOptions) -> (Vec<String>, Vec<(String, SkipReason)>) {
        let all = run(&sb.root, opts);
        (sorted(files(&sb.root, &all)), skipped(&sb.root, &all))
    }

    fn mkfifo(path: &Path) {
        let c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        // SAFETY: a valid NUL-terminated path.
        assert_eq!(
            unsafe { libc::mkfifo(c.as_ptr(), 0o644) },
            0,
            "mkfifo {path:?}"
        );
    }

    fn is_root() -> bool {
        // SAFETY: geteuid has no preconditions.
        let uid = unsafe { libc::geteuid() };
        uid == 0
    }

    /// The skipped entries as (absolute path, reason), in yielded order: for ignore files
    /// outside the root, where `skipped` (relative to the root) does not apply.
    fn skipped_abs(all: &[Walked]) -> Vec<(String, SkipReason)> {
        all.iter()
            .filter_map(|w| match w {
                Walked::Skipped { path, reason } => Some((path.clone(), *reason)),
                Walked::File { .. } => None,
            })
            .collect()
    }

    fn path_str(p: &Path) -> String {
        p.to_str().unwrap().to_string()
    }

    const CAP_BYTES: u64 = 1 << 30;

    /// Runs the test `test` of this very binary as a child process, with `key=value` in its
    /// environment, whose address space is capped at 1 GiB (RLIMIT_AS) and which is killed after
    /// 30 s. Returns its stdout, or what went wrong: an allocation failure or a stack overflow
    /// aborts the child, so a walker that reads a device or a huge file into memory, or that
    /// recurses without bound, fails the test instead of taking the run down. The cap is set
    /// between fork and exec, so only the child has it. The child inherits the sandbox's HOME
    /// and XDG_CONFIG_HOME.
    fn spawn_capped(test: &str, key: &str, value: &OsStr) -> Result<String, String> {
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args([test, "--exact", "--nocapture", "--test-threads=1"])
            .env(key, value)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // SAFETY: only setrlimit, which is async-signal-safe, runs between fork and exec.
        unsafe {
            cmd.pre_exec(|| {
                let limit = libc::rlimit {
                    rlim_cur: CAP_BYTES,
                    rlim_max: CAP_BYTES,
                };
                if libc::setrlimit(libc::RLIMIT_AS, &limit) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = cmd.spawn().unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break Some(status);
            }
            if std::time::Instant::now() > deadline {
                let _ = child.kill();
                break None;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let out = child.wait_with_output().unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        match status {
            None => Err("the walk did not finish within 30 s".to_string()),
            Some(s) if !s.success() => Err(format!(
                "the walk died under the 1 GiB address-space cap ({s}); stderr: {}",
                stderr.lines().last().unwrap_or("")
            )),
            Some(_) => Ok(stdout.into_owned()),
        }
    }

    /// The default-options walk of `root` in a capped child process (`capped_child`, with the root
    /// in the environment). Returns the walk, or what went wrong.
    fn run_capped(root: &Path) -> Result<Vec<Walked>, String> {
        let stdout = spawn_capped(
            "goal::capped_child",
            "HALI_WALK_CAPPED_ROOT",
            root.as_os_str(),
        )?;
        Ok(stdout
            .lines()
            .filter_map(|l| l.split_once("WALKED ").map(|(_, rest)| rest))
            .map(|l| match l.split_once(' ').unwrap() {
                ("F", path) => Walked::File {
                    path: path.to_string(),
                    byte_size: 0,
                    mtime: UNIX_EPOCH,
                },
                (reason, path) => Walked::Skipped {
                    path: path.to_string(),
                    reason: match reason {
                        "Unreadable" => SkipReason::Unreadable,
                        "NotRegular" => SkipReason::NotRegular,
                        "NonUtf8" => SkipReason::NonUtf8,
                        "OutsideRoot" => SkipReason::OutsideRoot,
                        "BadIgnoreFile" => SkipReason::BadIgnoreFile,
                        other => panic!("unexpected reason {other}"),
                    },
                },
            })
            .collect())
    }

    /// The child side of `run_capped`: it does nothing unless the parent started it. (libtest
    /// prints `test ... ` before the first line, hence the split on the marker in the parent.)
    #[test]
    fn capped_child() {
        let Some(root) = std::env::var_os("HALI_WALK_CAPPED_ROOT") else {
            return;
        };
        for w in walk(Path::new(&root), &opts()) {
            match w {
                Walked::File { path, .. } => println!("WALKED F {path}"),
                Walked::Skipped { path, reason } => println!("WALKED {reason:?} {path}"),
            }
        }
    }

    /// The child side of `exclude_nesting_bounded`: builds an exclude of `n` nested brace groups
    /// (`n` from the environment) and says whether `WalkOptions::new` refused it.
    #[test]
    fn capped_exclude_child() {
        let Some(n) = std::env::var_os("HALI_WALK_CAPPED_EXCLUDE_DEPTH") else {
            return;
        };
        let n: usize = n.to_str().unwrap().parse().unwrap();
        match WalkOptions::new(false, false, vec![nested_braces(n)], vec![]) {
            Err(pattern) => println!("EXCLUDE refused {}", pattern.len()),
            Ok(_) => println!("EXCLUDE accepted"),
        }
    }

    /// `n` nested brace groups around one name: `{{{a}}}`.
    fn nested_braces(n: usize) -> String {
        format!("{}a{}", "{".repeat(n), "}".repeat(n))
    }

    // ---- 1. hidden, and the shape of a yielded file ----

    #[test]
    fn hidden() {
        let sb = sandbox!();
        write(&sb.root, "a.txt", "12345");
        write(&sb.root, ".hidden.txt", "h");
        write(&sb.root, ".hid/inner.txt", "i");
        write(&sb.root, "sub/b.txt", "b");
        write(&sb.root, "sub/.dot", "d");
        write(&sb.root, "sub/.hid2/deep.txt", "x");
        // A File carries the real UTF-8 path, the size and the mtime.
        let when = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        fs::OpenOptions::new()
            .write(true)
            .open(sb.root.join("a.txt"))
            .unwrap()
            .set_modified(when)
            .unwrap();

        let all = run(&sb.root, opts());
        assert_eq!(files(&sb.root, &all), ["a.txt", "sub/b.txt"]);
        assert!(
            skipped(&sb.root, &all).is_empty(),
            "hidden names are skipped silently"
        );
        let a = sb.root.join("a.txt");
        assert_eq!(
            all[0],
            Walked::File {
                path: a.to_str().unwrap().to_string(),
                byte_size: 5,
                mtime: when,
            }
        );

        let (got, skips) = outcome(
            &sb,
            WalkOptions {
                include_hidden: true,
                ..opts()
            },
        );
        assert_eq!(
            got,
            [
                ".hid/inner.txt",
                ".hidden.txt",
                "a.txt",
                "sub/.dot",
                "sub/.hid2/deep.txt",
                "sub/b.txt"
            ]
        );
        assert!(skips.is_empty());

        // The root itself is always walked, even when its own name is hidden.
        let hidden_root = sb.base().join(".hidden-root");
        write(&hidden_root, "x.txt", "x");
        write(&hidden_root, "sub/y.txt", "y");
        let all = run(&hidden_root, opts());
        assert_eq!(files(&hidden_root, &all), ["sub/y.txt", "x.txt"]);
    }

    // ---- 2. ignore files ----

    #[test]
    fn ignore_files() {
        // `.gitignore` is honoured only inside a git repository; `.ignore` and `.haliignore`
        // always; the ignore files themselves are hidden, so they are never yielded.
        {
            let sb = sandbox!();
            write(&sb.root, ".gitignore", "from-gitignore.txt\n");
            write(&sb.root, ".ignore", "from-ignore.txt\n");
            write(&sb.root, ".haliignore", "from-haliignore.txt\n");
            for n in ["from-gitignore", "from-ignore", "from-haliignore", "keep"] {
                write(&sb.root, &format!("{n}.txt"), "x");
            }
            // A .gitignore in a parent of the root, outside any repository, is ignored too.
            write(&sb.base(), ".gitignore", "from-parent-gitignore.txt\n");
            write(&sb.root, "from-parent-gitignore.txt", "x");
            let (got, skips) = outcome(&sb, opts());
            assert_eq!(
                got,
                [
                    "from-gitignore.txt",
                    "from-parent-gitignore.txt",
                    "keep.txt"
                ]
            );
            assert!(skips.is_empty());
        }

        // Inside a repository: .gitignore at any depth, .git/info/exclude, .ignore, .haliignore
        // (also in subdirectories), with directory patterns, anchoring and negation.
        {
            let sb = sandbox!();
            make_repo(&sb.root);
            write(
                &sb.root,
                ".gitignore",
                "g.txt\nbuild/\n/only-top.txt\n*.o\n!keep.o\n",
            );
            write(&sb.root, ".git/info/exclude", "e.txt\n");
            write(&sb.root, ".ignore", "i.txt\n");
            write(&sb.root, ".haliignore", "h.txt\n");
            write(&sb.root, "sub/.gitignore", "sg.txt\n");
            write(&sb.root, "sub/.haliignore", "sh.txt\n");
            write(&sb.root, "sub/.ignore", "si.txt\n");
            for rel in [
                "g.txt",
                "e.txt",
                "i.txt",
                "h.txt",
                "only-top.txt",
                "x.o",
                "keep.o",
                "keep.txt",
                "build/b.txt",
                "sub/g.txt",
                "sub/only-top.txt",
                "sub/sg.txt",
                "sub/sh.txt",
                "sub/si.txt",
                "sub/ok.txt",
                "other/sg.txt",
                "other/build",
            ] {
                write(&sb.root, rel, "x");
            }
            let (got, skips) = outcome(&sb, opts());
            // sub/g.txt etc. are ignored by the root's patterns; other/sg.txt is outside
            // sub/'s .gitignore; other/build is a file, not the directory `build/`.
            assert_eq!(
                got,
                [
                    "keep.o",
                    "keep.txt",
                    "other/build",
                    "other/sg.txt",
                    "sub/ok.txt",
                    "sub/only-top.txt"
                ]
            );
            assert!(skips.is_empty());
        }

        // The global gitignore: $XDG_CONFIG_HOME/git/ignore ...
        {
            let sb = sandbox!();
            make_repo(&sb.root);
            write(&sb.xdg, "git/ignore", "xdg.txt\n");
            for n in ["xdg.txt", "keep.txt"] {
                write(&sb.root, n, "x");
            }
            let (got, _) = outcome(&sb, opts());
            assert_eq!(got, ["keep.txt"]);
        }
        // ... unless core.excludesFile in ~/.gitconfig names another one, which then wins.
        {
            let sb = sandbox!();
            make_repo(&sb.root);
            let custom = sb.base().join("custom-ignore");
            fs::write(&custom, "cfg.txt\n").unwrap();
            write(&sb.xdg, "git/ignore", "xdg.txt\n");
            fs::write(
                sb.home.join(".gitconfig"),
                format!("[core]\n\texcludesFile = {}\n", custom.display()),
            )
            .unwrap();
            for n in ["cfg.txt", "xdg.txt", "keep.txt"] {
                write(&sb.root, n, "x");
            }
            let (got, _) = outcome(&sb, opts());
            assert_eq!(got, ["keep.txt", "xdg.txt"]);
        }

        // The root's parents: .ignore and .haliignore of any ancestor, and the .gitignore and
        // .git/info/exclude of an ancestor that is a repository.
        {
            let sb = sandbox!();
            let top = sb.base();
            make_repo(&top);
            write(&top, ".gitignore", "pg.txt\n");
            write(&top, ".git/info/exclude", "pe.txt\n");
            write(&top, ".ignore", "pi.txt\n");
            write(&top, ".haliignore", "ph.txt\n");
            write(&sb.root, "sub/.haliignore", "!ph.txt\n");
            for n in ["pg", "pe", "pi", "ph", "keep"] {
                write(&sb.root, &format!("{n}.txt"), "x");
            }
            write(&sb.root, "sub/ph.txt", "x");
            let (got, skips) = outcome(&sb, opts());
            // A child directory's .haliignore overrides a parent's.
            assert_eq!(got, ["keep.txt", "sub/ph.txt"]);
            assert!(skips.is_empty());
        }
    }

    #[test]
    fn no_ignore() {
        let sb = sandbox!();
        make_repo(&sb.root);
        write(&sb.root, ".gitignore", "g.txt\n");
        write(&sb.root, ".git/info/exclude", "e.txt\n");
        write(&sb.root, ".ignore", "i.txt\n");
        write(&sb.root, ".haliignore", "h.txt\n{bad\n");
        write(&sb.xdg, "git/ignore", "glob.txt\n");
        write(&sb.base(), ".ignore", "p.txt\n");
        for n in [
            "g.txt", "e.txt", "i.txt", "h.txt", "glob.txt", "p.txt", "keep.txt", "x.log", ".dot",
        ] {
            write(&sb.root, n, "x");
        }
        write(&sb.root, "node_modules/m.js", "x");

        // It turns off exactly the five ignore-file sources: hidden names, the user excludes
        // and the built-in excludes still apply, and the ignore files are not even read, so
        // there is no bad_ignore_file.
        let (got, skips) = outcome(
            &sb,
            WalkOptions {
                no_ignore: true,
                excludes: strs(&["*.log"]),
                ..opts()
            },
        );
        assert_eq!(
            got,
            [
                "e.txt", "g.txt", "glob.txt", "h.txt", "i.txt", "keep.txt", "p.txt"
            ]
        );
        assert!(skips.is_empty(), "{skips:?}");

        // With include_hidden as well, the ignore files themselves appear as files.
        let (got, _) = outcome(
            &sb,
            WalkOptions {
                no_ignore: true,
                include_hidden: true,
                ..opts()
            },
        );
        assert_eq!(
            got,
            [
                ".dot",
                ".gitignore",
                ".haliignore",
                ".ignore",
                "e.txt",
                "g.txt",
                "glob.txt",
                "h.txt",
                "i.txt",
                "keep.txt",
                "p.txt",
                "x.log"
            ]
        );
    }

    #[test]
    fn bad_ignore_file() {
        let sb = sandbox!();
        make_repo(&sb.root);
        // Two invalid lines in one file are reported once; the valid lines still apply.
        write(
            &sb.root,
            ".haliignore",
            "valid-h.txt\n{bad1\nalso-valid-h.txt\n[z-a]\n",
        );
        write(&sb.root, "sub/.gitignore", "valid-g.txt\n{bad3\n");
        write(&sb.root, "clean/.ignore", "valid-i.txt\n");
        for rel in [
            "valid-h.txt",
            "also-valid-h.txt",
            "keep.txt",
            "sub/valid-g.txt",
            "sub/keep.txt",
            "clean/valid-i.txt",
            "clean/keep.txt",
        ] {
            write(&sb.root, rel, "x");
        }
        let (got, skips) = outcome(&sb, opts());
        assert_eq!(got, ["clean/keep.txt", "keep.txt", "sub/keep.txt"]);
        assert_eq!(
            skips,
            [
                (".haliignore".to_string(), SkipReason::BadIgnoreFile),
                ("sub/.gitignore".to_string(), SkipReason::BadIgnoreFile),
            ]
        );
    }

    // ---- 3. built-in and user excludes ----

    #[test]
    fn builtin_excludes() {
        let sb = sandbox!();
        make_repo(&sb.root);
        for rel in [
            "keep.txt",
            "node_modules/x.js",
            "a/node_modules/y.js",
            "target/t.o",
            "a/b/target/t.o",
            "__pycache__/p.pyc",
            "a/__pycache__/p.pyc",
            "venv/bin/python",
            "deep/venv/v.txt",
            "sub/.git/config",
            "state/hali/resources.ttl",
            // near misses and files with a built-in's name are not excluded
            "targets/ok.txt",
            "my_target/ok.txt",
            "node_modules2/ok.txt",
            "sub/target",
            "sub/venv",
            "state2/ok.txt",
        ] {
            write(&sb.root, rel, "x");
        }
        let prune = vec![sb.root.join("state/hali")];
        for (include_hidden, no_ignore) in
            [(false, false), (true, true), (true, false), (false, true)]
        {
            let (got, skips) = outcome(
                &sb,
                WalkOptions::new(include_hidden, no_ignore, vec![], prune.clone())
                    .ok()
                    .unwrap(),
            );
            assert_eq!(
                got,
                [
                    "keep.txt",
                    "my_target/ok.txt",
                    "node_modules2/ok.txt",
                    "state2/ok.txt",
                    "sub/target",
                    "sub/venv",
                    "targets/ok.txt"
                ],
                "include_hidden={include_hidden} no_ignore={no_ignore}"
            );
            assert!(
                skips.is_empty(),
                "excluded directories are not reported: {skips:?}"
            );
        }

        // The data dir is pruned wherever it is, and is not entered; the root's own name is
        // never excluded, even when it is `target`.
        let root = sb.base().join("target");
        write(&root, "t.txt", "x");
        write(&root, "data/d.ttl", "x");
        let all = run(
            &root,
            WalkOptions::new(false, false, vec![], vec![root.join("data")])
                .ok()
                .unwrap(),
        );
        assert_eq!(files(&root, &all), ["t.txt"]);
    }

    #[test]
    fn user_excludes() {
        let sb = sandbox!();
        for rel in [
            "a.log",
            "sub/b.log",
            "top.txt",
            "sub/top.txt",
            "build/x.o",
            "sub/build/y.o",
            "p/build",
            "docs/a.tmp",
            "docs/a.md",
            "other/docs/b.tmp",
            "keep.txt",
            "skipme/z.txt",
            "a/skipme/z.txt",
        ] {
            write(&sb.root, rel, "x");
        }
        let excludes = strs(&["*.log", "/top.txt", "build/", "docs/*.tmp", "skipme"]);
        let (got, skips) = outcome(
            &sb,
            WalkOptions::new(false, false, excludes, vec![])
                .ok()
                .unwrap(),
        );
        // `*.log` and `skipme` match at any depth; `/top.txt` and `docs/*.tmp` are anchored at
        // the root; `build/` matches directories only.
        assert_eq!(
            got,
            [
                "docs/a.md",
                "keep.txt",
                "other/docs/b.tmp",
                "p/build",
                "sub/top.txt"
            ]
        );
        assert!(skips.is_empty());
    }

    #[test]
    fn bad_exclude() {
        let ok = WalkOptions::new(
            true,
            false,
            strs(&["*.log", "/top", "a/**/b"]),
            vec![PathBuf::from("/d")],
        )
        .ok()
        .expect("valid globs");
        assert!(ok.include_hidden && !ok.no_ignore);
        assert_eq!(ok.excludes, ["*.log", "/top", "a/**/b"]);
        assert_eq!(ok.prune, [PathBuf::from("/d")]);

        let err = |patterns: &[&str]| WalkOptions::new(false, false, strs(patterns), vec![]).err();
        assert_eq!(err(&["{bad"]), Some("{bad".to_string()));
        assert_eq!(err(&["x", "[z-a]"]), Some("[z-a]".to_string()));
        assert_eq!(err(&["*.log", "{bad"]), Some("{bad".to_string()));
        assert_eq!(err(&["!keep.txt"]), Some("!keep.txt".to_string()));
        assert_eq!(err(&["ok", "!neg", "{bad"]), Some("!neg".to_string()));
        assert_eq!(err(&["{bad", "!neg"]), Some("{bad".to_string()));
        // An empty or whitespace-only pattern is refused (the crate would take it as a pattern
        // that matches nothing, or everything).
        assert_eq!(err(&[""]), Some(String::new()));
        assert_eq!(err(&[" "]), Some(" ".to_string()));
        assert_eq!(err(&["\t \n"]), Some("\t \n".to_string()));
        assert_eq!(err(&["ok", "", "{bad"]), Some(String::new()));
        assert_eq!(err(&["{bad", " "]), Some("{bad".to_string()));
        assert_eq!(err(&["x", " ", "!neg"]), Some(" ".to_string()));
        assert_eq!(err(&[" a", "b "]), None);
        assert_eq!(err(&["*.log", "build/"]), None);
        assert_eq!(err(&[]), None);

        // `{` nesting deeper than 100 levels is refused (globset recurses once per level, so a
        // hostile `[scan].exclude` would overflow the stack): 100 are accepted, 101 refused. It
        // is the depth that counts, not the running total, and escapes and classes hold no braces
        // (the same counter as for ignore files).
        let (deep100, deep101) = (nested_braces(100), nested_braces(101));
        assert_eq!(err(&[&deep100]), None, "100 levels are accepted");
        assert_eq!(err(&[&deep101]), Some(deep101.clone()), "101 are refused");
        let inner = format!("x/{deep101}/y");
        assert_eq!(err(&[&inner]), Some(inner.clone()));
        assert_eq!(err(&["ok", &deep101, "{bad"]), Some(deep101.clone()));
        assert_eq!(err(&["{bad", &deep101]), Some("{bad".to_string()));
        assert_eq!(
            err(&["ok", &deep100, &deep100, &deep100]),
            None,
            "depth, not total"
        );
        assert_eq!(
            err(&[&"{a}".repeat(200)]),
            None,
            "side by side is not nested"
        );
        assert_eq!(
            err(&[&format!("{}a", "\\{".repeat(101))]),
            None,
            "escaped braces"
        );
        assert_eq!(
            err(&[&format!("{}a", "[{]".repeat(101))]),
            None,
            "braces in a class"
        );
    }

    #[test]
    fn exclude_nesting_bounded() {
        // 100,000 nested braces: without the bound, `OverrideBuilder::add` recurses once per
        // level and overflows the stack, aborting the process. So it runs in the capped child,
        // where an abort fails this test cleanly.
        let out = spawn_capped(
            "goal::capped_exclude_child",
            "HALI_WALK_CAPPED_EXCLUDE_DEPTH",
            OsStr::new("100000"),
        )
        .unwrap_or_else(|e| panic!("a 100,000-level exclude: {e}"));
        assert!(
            out.contains("EXCLUDE refused 200001"),
            "the exclude is refused as given: {out}"
        );
    }

    // ---- 4. symlinks ----

    #[test]
    fn symlinks() {
        let sb = sandbox!();
        let outside = sb.base().join("outside");
        write(&outside, "o.txt", "o");
        write(&outside, "inner/oo.txt", "o");
        write(&sb.root, "real/f.txt", "f");
        write(&sb.root, "top.txt", "t");
        write(&sb.root, "real/.hidden-target", "h");
        symlink("real", sb.root.join("dlink")).unwrap(); // directory inside the root
        symlink("real/f.txt", sb.root.join("alink")).unwrap(); // file inside, seen before it
        symlink("real/f.txt", sb.root.join("zlink")).unwrap(); // file inside, seen after it
        symlink("zlink", sb.root.join("zchain")).unwrap(); // a chain to the same file
        symlink("top.txt", sb.root.join("link-to-top")).unwrap();
        symlink(outside.join("o.txt"), sb.root.join("out-file")).unwrap(); // file outside
        symlink("../outside/inner", sb.root.join("out-dir")).unwrap(); // directory outside
        symlink("missing.txt", sb.root.join("broken")).unwrap(); // broken
        symlink("zchain-loop", sb.root.join("loop-a")).unwrap(); // a loop of links
        symlink("loop-a", sb.root.join("zchain-loop")).unwrap();
        // A link in a subdirectory: a skipped link carries its own path.
        write(&sb.root, "sub/s.txt", "s");
        symlink(outside.join("o.txt"), sb.root.join("sub/out2")).unwrap();
        symlink("../real/f.txt", sb.root.join("sub/in2")).unwrap();

        let (got, skips) = outcome(&sb, opts());
        // A file symlink whose real path is inside the root is silent: the real file is
        // walked directly, once, however many links lead to it. Nothing is yielded under a
        // directory symlink, and directory symlinks are not reported at all.
        assert_eq!(got, ["real/f.txt", "sub/s.txt", "top.txt"]);
        assert_eq!(
            skips,
            [
                ("broken".to_string(), SkipReason::NotRegular),
                ("loop-a".to_string(), SkipReason::NotRegular),
                ("out-file".to_string(), SkipReason::OutsideRoot),
                ("sub/out2".to_string(), SkipReason::OutsideRoot),
                ("zchain-loop".to_string(), SkipReason::NotRegular),
            ]
        );
        // The yielded paths are the real ones.
        for w in run(&sb.root, opts()) {
            if let Walked::File { path, .. } = w {
                assert!(!path.contains("link") && !path.contains("in2"), "{path}");
            }
        }
    }

    #[test]
    fn symlink_no_bypass() {
        // A file symlink inside the root can never yield a file the walk would not yield at
        // its real path: the link is silent, and the real file keeps its own treatment.
        let sb = sandbox!();
        write(&sb.root, ".haliignore", "ign.txt\n");
        for rel in [
            "keep.txt",
            "secret.log",
            ".hid/h.txt",
            "ign.txt",
            "node_modules/n.js",
            "deep/target/t.o",
            "state/hali/d.ttl",
        ] {
            write(&sb.root, rel, "x");
        }
        for (target, link) in [
            ("keep.txt", "l-keep"),
            ("secret.log", "l-log"),
            (".hid/h.txt", "l-hid"),
            ("ign.txt", "l-ign"),
            ("node_modules/n.js", "l-nm"),
            ("deep/target/t.o", "l-target"),
            ("state/hali/d.ttl", "l-data"),
        ] {
            symlink(target, sb.root.join(link)).unwrap();
        }
        // The same from a subdirectory, with relative targets going up.
        write(&sb.root, "sub/ok.txt", "x");
        for (target, link) in [
            ("../secret.log", "sub/s-log"),
            ("../.hid/h.txt", "sub/s-hid"),
            ("../ign.txt", "sub/s-ign"),
            ("../node_modules/n.js", "sub/s-nm"),
            ("../state/hali/d.ttl", "sub/s-data"),
        ] {
            symlink(target, sb.root.join(link)).unwrap();
        }
        let excludes = strs(&["*.log"]);
        let prune = vec![sb.root.join("state/hali")];

        let (got, skips) = outcome(
            &sb,
            WalkOptions::new(false, false, excludes.clone(), prune.clone())
                .ok()
                .unwrap(),
        );
        assert_eq!(got, ["keep.txt", "sub/ok.txt"]);
        assert!(skips.is_empty(), "{skips:?}");

        // With hidden names included, the hidden target is walked directly, once; the link to
        // it stays silent; the excluded, ignored, built-in and pruned targets stay out.
        let (got, skips) = outcome(
            &sb,
            WalkOptions::new(true, false, excludes, prune).ok().unwrap(),
        );
        assert_eq!(got, [".haliignore", ".hid/h.txt", "keep.txt", "sub/ok.txt"]);
        assert!(skips.is_empty(), "{skips:?}");

        // With the ignore files off, the formerly ignored file is walked once, at its real path.
        let (got, skips) = outcome(
            &sb,
            WalkOptions::new(
                false,
                true,
                strs(&["*.log"]),
                vec![sb.root.join("state/hali")],
            )
            .ok()
            .unwrap(),
        );
        assert_eq!(got, ["ign.txt", "keep.txt", "sub/ok.txt"]);
        assert!(skips.is_empty(), "{skips:?}");
    }

    #[test]
    fn symlinked_root_alias() {
        // The walk is built from the canonical root, so a symlink alias of the root gives the
        // same result as the root: one file, at its real path (not `alias/f.txt` as well).
        let sb = sandbox!();
        write(&sb.root, "f.txt", "f");
        symlink("f.txt", sb.root.join("z-link")).unwrap();
        let alias = sb.base().join("alias");
        symlink(&sb.root, &alias).unwrap();

        let all = run(&alias, opts());
        assert_eq!(
            skipped_abs(&all),
            [],
            "no skips expected: {:?}",
            skipped_abs(&all)
        );
        let paths: Vec<&str> = all
            .iter()
            .filter_map(|w| match w {
                Walked::File { path, .. } => Some(path.as_str()),
                Walked::Skipped { .. } => None,
            })
            .collect();
        assert_eq!(paths, [path_str(&sb.root.join("f.txt"))]);
    }

    #[test]
    fn prune_via_alias() {
        // A data dir given as a symlinked alias is still pruned: it is compared canonicalized.
        // (Without the canonicalization the alias never equals the walked path.)
        let sb = sandbox!();
        write(&sb.root, "keep.txt", "x");
        write(&sb.root, "state/hali/d.ttl", "x");
        write(&sb.root, "state/hali/sub/e.ttl", "x");
        let alias = sb.base().join("data-alias");
        symlink(sb.root.join("state/hali"), &alias).unwrap();

        let (got, skips) = outcome(
            &sb,
            WalkOptions::new(false, false, vec![], vec![alias])
                .ok()
                .unwrap(),
        );
        assert_eq!(got, ["keep.txt"]);
        assert!(skips.is_empty(), "{skips:?}");
    }

    // ---- 5. special, unreadable and non-UTF-8 entries ----

    #[test]
    fn special_files() {
        let sb = sandbox!();
        write(&sb.root, "ok.txt", "ok");
        write(&sb.root, "sub/after.txt", "x");
        let fifo = std::ffi::CString::new(sb.root.join("pipe").as_os_str().as_bytes()).unwrap();
        // SAFETY: a valid NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o644) }, 0);
        let sub_fifo =
            std::ffi::CString::new(sb.root.join("sub/zpipe").as_os_str().as_bytes()).unwrap();
        // SAFETY: as above.
        assert_eq!(unsafe { libc::mkfifo(sub_fifo.as_ptr(), 0o644) }, 0);
        let _listener = std::os::unix::net::UnixListener::bind(sb.root.join("sock")).unwrap();
        symlink("pipe", sb.root.join("link-to-pipe")).unwrap();
        // A hard link is just another path to the same bytes.
        fs::hard_link(sb.root.join("ok.txt"), sb.root.join("ok-hard.txt")).unwrap();

        // `run` fails after 30 s: a walker that opened a FIFO would block there.
        let (got, skips) = outcome(&sb, opts());
        assert_eq!(got, ["ok-hard.txt", "ok.txt", "sub/after.txt"]);
        assert_eq!(
            skips,
            [
                ("link-to-pipe".to_string(), SkipReason::NotRegular),
                ("pipe".to_string(), SkipReason::NotRegular),
                ("sock".to_string(), SkipReason::NotRegular),
                ("sub/zpipe".to_string(), SkipReason::NotRegular),
            ]
        );
        let sizes: Vec<u64> = run(&sb.root, opts())
            .iter()
            .filter_map(|w| match w {
                Walked::File {
                    byte_size, path, ..
                } if path.contains("ok") => Some(*byte_size),
                _ => None,
            })
            .collect();
        assert_eq!(sizes, [2, 2]);
    }

    /// Restores permissions on drop so the temp dir can be removed even when an assert fails.
    struct Restore(Vec<PathBuf>);
    impl Drop for Restore {
        fn drop(&mut self) {
            for p in &self.0 {
                let _ = fs::set_permissions(p, fs::Permissions::from_mode(0o755));
            }
        }
    }

    #[test]
    fn unreadable() {
        // A mode-000 regular file is yielded (the walker never opens regular files; `inspect`
        // in #22 reports an unreadable one). A mode-000 directory is `Unreadable` and not
        // entered -- unless the user is root, for whom permissions do not apply: the test then
        // says so and asserts that the directory is walked.
        let root_user = is_root();
        if root_user {
            eprintln!(
                "NOTE: running as root: a mode-000 directory is readable, so this run asserts \
                 that it is walked, not that it is Unreadable"
            );
        }
        let sb = sandbox!();
        write(&sb.root, "a-ok.txt", "x");
        write(&sb.root, "secret.txt", "x");
        write(&sb.root, "locked/inner.txt", "x");
        write(&sb.root, "z-ok.txt", "x");
        write(&sb.root, "sub/deeper/last.txt", "x");
        let _restore = Restore(vec![sb.root.join("locked"), sb.root.join("secret.txt")]);
        fs::set_permissions(
            sb.root.join("secret.txt"),
            fs::Permissions::from_mode(0o000),
        )
        .unwrap();
        fs::set_permissions(sb.root.join("locked"), fs::Permissions::from_mode(0o000)).unwrap();

        let all = run(&sb.root, opts());
        let (got, skips) = (sorted(files(&sb.root, &all)), skipped(&sb.root, &all));
        // The walk continues past both. `secret.txt` is a File with its size.
        assert!(
            all.iter().any(|w| matches!(w,
                Walked::File { path, byte_size: 1, .. } if path.ends_with("/secret.txt"))),
            "a mode-000 regular file is yielded, not opened: {all:?}"
        );
        if root_user {
            assert_eq!(
                got,
                [
                    "a-ok.txt",
                    "locked/inner.txt",
                    "secret.txt",
                    "sub/deeper/last.txt",
                    "z-ok.txt"
                ]
            );
            assert!(skips.is_empty(), "{skips:?}");
        } else {
            // Nothing inside the locked directory is yielded.
            assert_eq!(
                got,
                ["a-ok.txt", "secret.txt", "sub/deeper/last.txt", "z-ok.txt"]
            );
            assert_eq!(skips, [("locked".to_string(), SkipReason::Unreadable)]);
        }
    }

    #[test]
    fn opens_no_regular_file() {
        // The walk may open ignore files and read directories, and nothing else: an inotify
        // IN_OPEN watch on every directory of the tree sees no open of any other name. (A
        // positive control proves the watch works here; if it cannot, the test says so and
        // returns rather than guessing.)
        let sb = sandbox!();
        make_repo(&sb.root);
        write(&sb.root, ".gitignore", "g.txt\n");
        write(&sb.root, ".haliignore", "h.txt\n");
        write(&sb.root, "sub/.ignore", "i.txt\n");
        for rel in [
            "a.txt",
            "g.txt",
            "h.txt",
            "z.txt",
            ".hid.txt",
            "sub/b.txt",
            "sub/i.txt",
            "sub/.dot",
        ] {
            write(&sb.root, rel, "x");
        }
        write(&sb.base(), "outside/o.txt", "x");
        symlink("a.txt", sb.root.join("link")).unwrap();
        symlink(sb.base().join("outside/o.txt"), sb.root.join("out")).unwrap();
        symlink("missing", sb.root.join("broken")).unwrap();
        mkfifo(&sb.root.join("pipe"));

        // SAFETY: plain syscalls on a descriptor owned here.
        let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
        if fd < 0 {
            eprintln!(
                "NOTE: skipped: inotify_init1 failed ({}); cannot watch opens here",
                std::io::Error::last_os_error()
            );
            return;
        }
        for dir in [sb.root.clone(), sb.root.join("sub")] {
            let c = std::ffi::CString::new(dir.as_os_str().as_bytes()).unwrap();
            // SAFETY: a valid descriptor and NUL-terminated path.
            let wd = unsafe { libc::inotify_add_watch(fd, c.as_ptr(), libc::IN_OPEN) };
            if wd < 0 {
                eprintln!(
                    "NOTE: skipped: inotify_add_watch failed ({})",
                    std::io::Error::last_os_error()
                );
                return;
            }
        }
        // The names of the open events that name an entry of a watched directory (an open of
        // a watched directory itself carries no name).
        let drain = || -> Vec<String> {
            let mut names = vec![];
            let mut buf = [0u8; 16384];
            loop {
                // SAFETY: reads into a buffer of the stated length.
                let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
                if n <= 0 {
                    break;
                }
                let n = n as usize;
                let mut off = 0;
                while off < n {
                    // SAFETY: the kernel wrote a whole inotify_event at `off`.
                    let ev = unsafe {
                        std::ptr::read_unaligned(
                            buf.as_ptr().add(off).cast::<libc::inotify_event>(),
                        )
                    };
                    let start = off + std::mem::size_of::<libc::inotify_event>();
                    let raw = &buf[start..start + ev.len as usize];
                    let end = raw.iter().position(|b| *b == 0).unwrap_or(raw.len());
                    if end > 0 && ev.mask & libc::IN_ISDIR == 0 {
                        names.push(String::from_utf8_lossy(&raw[..end]).into_owned());
                    }
                    off = start + ev.len as usize;
                }
            }
            names
        };

        fs::File::open(sb.root.join("a.txt")).unwrap();
        if drain() != ["a.txt"] {
            eprintln!("NOTE: skipped: the inotify watch did not report a control open");
            return;
        }

        let a = run(&sb.root, opts());
        let b = run(
            &sb.root,
            WalkOptions {
                include_hidden: true,
                ..opts()
            },
        );
        assert!(!a.is_empty() && !b.is_empty());
        let opened: Vec<String> = drain()
            .into_iter()
            .filter(|n| ![".gitignore", ".ignore", ".haliignore"].contains(&n.as_str()))
            .collect();
        assert!(
            opened.is_empty(),
            "the walk opened regular files (or a FIFO): {opened:?}"
        );
        // SAFETY: closing the descriptor opened above.
        unsafe { libc::close(fd) };
    }

    #[test]
    fn non_utf8() {
        let sb = sandbox!();
        write(&sb.root, "a.txt", "x");
        write(&sb.root, "z.txt", "x");
        let bad_file = OsStr::from_bytes(b"bad\xff.txt");
        fs::write(sb.root.join(bad_file), "x").unwrap();
        let bad_dir = sb.root.join(OsStr::from_bytes(b"d\xfe\xfd"));
        fs::create_dir(&bad_dir).unwrap();
        fs::write(bad_dir.join("in.txt"), "x").unwrap();
        fs::create_dir(bad_dir.join("nested")).unwrap();
        fs::write(bad_dir.join("nested/deep.txt"), "x").unwrap();
        // A UTF-8 directory holding a non-UTF-8 file: the walk enters it.
        write(&sb.root, "ok/fine.txt", "x");
        fs::write(sb.root.join("ok").join(OsStr::from_bytes(b"\xc3.txt")), "x").unwrap();

        let all = run(&sb.root, opts());
        assert_eq!(files(&sb.root, &all), ["a.txt", "ok/fine.txt", "z.txt"]);
        // The path is lossy (U+FFFD for each invalid byte); a directory is reported once and
        // not entered.
        assert_eq!(
            skipped(&sb.root, &all),
            [
                ("bad\u{fffd}.txt".to_string(), SkipReason::NonUtf8),
                ("d\u{fffd}\u{fffd}".to_string(), SkipReason::NonUtf8),
                ("ok/\u{fffd}.txt".to_string(), SkipReason::NonUtf8),
            ]
        );
    }

    // ---- 6. determinism and read-only ----

    #[test]
    fn determinism() {
        let sb = sandbox!();
        // Created in an order unrelated to the sorted one.
        for rel in [
            "z.txt",
            "m/n.txt",
            "m/a.txt",
            "a.txt",
            "b/c/d.txt",
            "B.txt",
            "b/c/a.txt",
            "b-x.txt",
        ] {
            write(&sb.root, rel, "x");
        }
        symlink("z.txt", sb.root.join("zz-link")).unwrap();
        symlink("missing", sb.root.join("n-broken")).unwrap();

        let first = run(&sb.root, opts());
        let second = run(&sb.root, opts());
        assert_eq!(first, second);
        // Depth first, each directory's entries sorted by file name (byte order, so `B.txt`
        // comes first, and the directory `b` comes before `b-x.txt`).
        assert_eq!(
            files(&sb.root, &first),
            [
                "B.txt",
                "a.txt",
                "b/c/a.txt",
                "b/c/d.txt",
                "b-x.txt",
                "m/a.txt",
                "m/n.txt",
                "z.txt"
            ]
        );
        // The link `zz-link` resolves to z.txt, already yielded: it is not yielded again.
        assert_eq!(
            skipped(&sb.root, &first),
            [("n-broken".to_string(), SkipReason::NotRegular)]
        );
    }

    /// (relative path, kind, size, mtime ns, mode, link target) of everything under `dir`.
    fn snapshot(dir: &Path) -> Vec<(String, u32, u64, i64, i64, u32, Option<PathBuf>)> {
        fn go(
            base: &Path,
            dir: &Path,
            out: &mut Vec<(String, u32, u64, i64, i64, u32, Option<PathBuf>)>,
        ) {
            let mut entries: Vec<_> = fs::read_dir(dir).unwrap().map(|e| e.unwrap()).collect();
            entries.sort_by_key(|e| e.file_name());
            for e in entries {
                let path = e.path();
                let md = fs::symlink_metadata(&path).unwrap();
                let target = fs::read_link(&path).ok();
                out.push((
                    path.strip_prefix(base)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    md.mode() & 0o170000,
                    md.size(),
                    md.mtime(),
                    md.mtime_nsec(),
                    md.mode() & 0o7777,
                    target,
                ));
                if md.is_dir() {
                    go(base, &path, out);
                }
            }
        }
        let mut out = vec![];
        go(dir, dir, &mut out);
        out
    }

    #[test]
    fn reads_only() {
        let sb = sandbox!();
        make_repo(&sb.root);
        write(&sb.root, ".gitignore", "g.txt\n{bad\n");
        write(&sb.root, ".haliignore", "h.txt\n");
        write(&sb.root, "a.txt", "aaa");
        write(&sb.root, "g.txt", "x");
        write(&sb.root, "sub/.hidden", "x");
        write(&sb.root, "sub/b.txt", "bbb");
        write(&sb.root, "node_modules/m.js", "x");
        write(&sb.xdg, "git/ignore", "glob.txt\n");
        symlink("a.txt", sb.root.join("link")).unwrap();
        symlink("sub", sb.root.join("dlink")).unwrap();
        fs::write(sb.home.join(".gitconfig"), "[user]\n\tname = x\n").unwrap();
        let fifo = std::ffi::CString::new(sb.root.join("pipe").as_os_str().as_bytes()).unwrap();
        // SAFETY: a valid NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o644) }, 0);

        let before = snapshot(&sb.base());
        assert!(before.len() > 10);
        let a = run(&sb.root, opts());
        let b = run(
            &sb.root,
            WalkOptions {
                include_hidden: true,
                no_ignore: true,
                ..opts()
            },
        );
        assert!(!a.is_empty() && !b.is_empty());
        assert_eq!(snapshot(&sb.base()), before, "the walk changed the tree");
        // The mtime it reports is the file's own, untouched.
        let mtime = fs::metadata(sb.root.join("a.txt"))
            .unwrap()
            .modified()
            .unwrap();
        assert!(a.iter().any(|w| matches!(w,
            Walked::File { path, mtime: m, .. } if path.ends_with("/a.txt") && *m == mtime)));
        let _: SystemTime = mtime;
    }

    // ---- 7. ignore files fail closed ----
    //
    // A candidate ignore file (`.gitignore`, `.ignore`, `.haliignore`), or a `.git` file, that
    // is not a regular file, is over 256 KiB, has `{` nesting deeper than 100 levels, is not
    // valid UTF-8 or cannot be read is yielded as `Skipped { BadIgnoreFile, path: that file }`
    // and its directory is not walked. The same when the crate cannot build the matcher for a
    // file that passed those checks. For the root or a parent of the root, one such skip is
    // yielded and nothing is walked.

    #[test]
    fn ignore_file_fifo_never_opened() {
        // `run` fails after 30 s: a walker that opened a FIFO named as an ignore file would
        // block there. The FIFO is a `.ignore` (honoured in any directory).
        // (1) in the root: nothing is walked.
        {
            let sb = sandbox!();
            write(&sb.root, "a.txt", "x");
            write(&sb.root, "sub/b.txt", "x");
            mkfifo(&sb.root.join(".ignore"));
            let all = run(&sb.root, opts());
            assert_eq!(
                all,
                [Walked::Skipped {
                    path: path_str(&sb.root.join(".ignore")),
                    reason: SkipReason::BadIgnoreFile
                }]
            );
        }
        // (2) in a subdirectory: that directory is not walked, the rest is.
        {
            let sb = sandbox!();
            write(&sb.root, "ok/y.txt", "x");
            write(&sb.root, "sub/x.txt", "x");
            write(&sb.root, "sub/deeper/z.txt", "x");
            write(&sb.root, "zz/last.txt", "x");
            mkfifo(&sb.root.join("sub/.ignore"));
            let (got, skips) = outcome(&sb, opts());
            assert_eq!(got, ["ok/y.txt", "zz/last.txt"]);
            assert_eq!(
                skips,
                [("sub/.ignore".to_string(), SkipReason::BadIgnoreFile)]
            );
        }
        // (3) in a grandparent of the root: one skip, nothing walked.
        {
            let sb = sandbox!();
            let gp = sb.base().join("gp");
            let root = gp.join("mid/root");
            write(&root, "a.txt", "x");
            mkfifo(&gp.join(".ignore"));
            let all = run(&root, opts());
            assert_eq!(
                all,
                [Walked::Skipped {
                    path: path_str(&gp.join(".ignore")),
                    reason: SkipReason::BadIgnoreFile
                }]
            );
        }
        // (4) a `.git` file `gitdir: <elsewhere>` whose commondir's `info/exclude` is a FIFO
        // (the crate follows the `.git` file to it). The skip names the `.git` file or the
        // FIFO.
        {
            let sb = sandbox!();
            let (elsewhere, common) = (sb.base().join("elsewhere"), sb.base().join("common"));
            write(&sb.root, "a.txt", "x");
            write(
                &sb.root,
                ".git",
                &format!("gitdir: {}\n", elsewhere.display()),
            );
            write(&elsewhere, "commondir", &format!("{}\n", common.display()));
            fs::create_dir_all(common.join("info")).unwrap();
            mkfifo(&common.join("info/exclude"));
            let all = run(&sb.root, opts());
            let skips = skipped_abs(&all);
            assert_eq!(all.len(), 1, "nothing is walked: {all:?}");
            assert_eq!(skips.len(), 1, "{all:?}");
            assert_eq!(skips[0].1, SkipReason::BadIgnoreFile);
            assert!(
                [sb.root.join(".git"), common.join("info/exclude")]
                    .iter()
                    .any(|p| path_str(p) == skips[0].0),
                "the skip names the .git file or the FIFO: {skips:?}"
            );
        }
    }

    #[test]
    fn ignore_file_device() {
        // `.haliignore -> /dev/zero` never ends. The walk runs in a child under a 1 GiB
        // address-space cap, so a walker that reads it dies there instead of eating the host.
        let sb = sandbox!();
        write(&sb.root, "a.txt", "x");
        symlink("/dev/zero", sb.root.join(".haliignore")).unwrap();
        let all = run_capped(&sb.root).unwrap_or_else(|e| panic!("root case: {e}"));
        assert_eq!(
            all,
            [Walked::Skipped {
                path: path_str(&sb.root.join(".haliignore")),
                reason: SkipReason::BadIgnoreFile
            }]
        );

        drop(sb);
        let sb = sandbox!();
        write(&sb.root, "ok/y.txt", "x");
        write(&sb.root, "sub/x.txt", "x");
        symlink("/dev/zero", sb.root.join("sub/.ignore")).unwrap();
        let all = run_capped(&sb.root).unwrap_or_else(|e| panic!("subdirectory case: {e}"));
        assert_eq!(files(&sb.root, &all), ["ok/y.txt"]);
        assert_eq!(
            skipped(&sb.root, &all),
            [("sub/.ignore".to_string(), SkipReason::BadIgnoreFile)]
        );
    }

    #[test]
    fn huge_ignore_file_bounded() {
        // A 10 MB single line: reported without being parsed, and the walk stays small.
        let sb = sandbox!();
        write(&sb.root, "ok.txt", "x");
        write(&sb.root, "sub/x.txt", "x");
        write(
            &sb.root,
            "sub/.haliignore",
            &format!("{}\n", "a".repeat(10_000_000)),
        );
        let all = run_capped(&sb.root).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(files(&sb.root, &all), ["ok.txt"]);
        assert_eq!(
            skipped(&sb.root, &all),
            [("sub/.haliignore".to_string(), SkipReason::BadIgnoreFile)]
        );
    }

    #[test]
    fn ignore_file_size_boundary() {
        // 256 KiB (262,144 bytes) exactly is accepted; one byte more is not. Comment lines keep
        // it cheap.
        let line = format!("#{}\n", "c".repeat(1022));
        assert_eq!(line.len(), 1024);
        let at_cap = line.repeat(256);
        assert_eq!(at_cap.len(), 262_144);

        let sb = sandbox!();
        write(&sb.root, "sub/x.txt", "x");
        write(&sb.root, "sub/.haliignore", &at_cap);
        let (got, skips) = outcome(&sb, opts());
        assert_eq!(got, ["sub/x.txt"]);
        assert!(skips.is_empty(), "{skips:?}");

        write(&sb.root, "sub/.haliignore", &format!("{at_cap}\n"));
        let (got, skips) = outcome(&sb, opts());
        assert!(got.is_empty(), "{got:?}");
        assert_eq!(
            skips,
            [("sub/.haliignore".to_string(), SkipReason::BadIgnoreFile)]
        );
    }

    #[test]
    fn ignore_file_deep_nesting() {
        // 100,000 nested braces fit in 200 KB, under the size cap. globset recurses once per
        // level, so unchecked this overflows the stack and aborts the whole process (SIGABRT).
        // Each placement runs in the capped child process: an abort there fails the test
        // cleanly instead of killing the test run.
        let deep = format!("{}\n", nested_braces(100_000));
        assert!(deep.len() < 262_144, "the file must pass the size cap");

        // (1) in a subdirectory: reported at that file, the directory is not walked, the
        // siblings are yielded and the process survives.
        {
            let sb = sandbox!();
            write(&sb.root, "a.txt", "x");
            write(&sb.root, "ok/y.txt", "x");
            write(&sb.root, "sub/x.txt", "x");
            write(&sb.root, "sub/deeper/z.txt", "x");
            write(&sb.root, "zz/last.txt", "x");
            write(&sb.root, "sub/.ignore", &deep);
            let all = run_capped(&sb.root).unwrap_or_else(|e| panic!("subdirectory case: {e}"));
            assert_eq!(
                sorted(files(&sb.root, &all)),
                ["a.txt", "ok/y.txt", "zz/last.txt"]
            );
            assert_eq!(
                skipped(&sb.root, &all),
                [("sub/.ignore".to_string(), SkipReason::BadIgnoreFile)]
            );
        }
        // (2) in the root: one skip, nothing walked.
        {
            let sb = sandbox!();
            write(&sb.root, "a.txt", "x");
            write(&sb.root, "sub/b.txt", "x");
            write(&sb.root, ".haliignore", &deep);
            let all = run_capped(&sb.root).unwrap_or_else(|e| panic!("root case: {e}"));
            assert_eq!(
                all,
                [Walked::Skipped {
                    path: path_str(&sb.root.join(".haliignore")),
                    reason: SkipReason::BadIgnoreFile
                }]
            );
        }
        // (3) in a grandparent of the root: one skip, nothing walked.
        {
            let sb = sandbox!();
            let gp = sb.base().join("gp");
            let root = gp.join("mid/root");
            write(&root, "a.txt", "x");
            write(&gp, ".ignore", &deep);
            let all = run_capped(&root).unwrap_or_else(|e| panic!("parent case: {e}"));
            assert_eq!(
                all,
                [Walked::Skipped {
                    path: path_str(&gp.join(".ignore")),
                    reason: SkipReason::BadIgnoreFile
                }]
            );
        }
        // (4) the boundary: nesting of exactly 100 is accepted (the rule applies: `a` is
        // ignored), 101 is rejected. Many groups side by side are not deep: 200 in a row
        // nest 1 level, and are accepted.
        {
            let sb = sandbox!();
            write(&sb.root, "sub/x.txt", "x");
            write(&sb.root, "sub/a", "x");
            write(
                &sb.root,
                "sub/.ignore",
                &format!("{}\n", nested_braces(100)),
            );
            let (got, skips) = outcome(&sb, opts());
            assert_eq!(got, ["sub/x.txt"], "nesting of 100 is accepted and applied");
            assert!(skips.is_empty(), "{skips:?}");

            write(
                &sb.root,
                "sub/.ignore",
                &format!("{}\n", nested_braces(101)),
            );
            let (got, skips) = outcome(&sb, opts());
            assert!(got.is_empty(), "nesting of 101 is rejected: {got:?}");
            assert_eq!(
                skips,
                [("sub/.ignore".to_string(), SkipReason::BadIgnoreFile)]
            );

            write(&sb.root, "sub/.ignore", &format!("{}\n", "{a}".repeat(200)));
            let (got, skips) = outcome(&sb, opts());
            assert_eq!(got, ["sub/a", "sub/x.txt"], "side by side is not nested");
            assert!(skips.is_empty(), "{skips:?}");
        }
    }

    /// An ignore file that passes the size cap and the nesting bound but whose matcher the
    /// crate cannot build: one alternation of about 20,000 patterns compiles past the regex
    /// engine's 10 MiB NFA limit. `secret*` comes first: the crate drops every rule of a file
    /// whose matcher fails to build, so a walker that carries on would yield `secret.key`.
    /// (Deep nesting cannot serve here: the crate's own nest limit is 250 levels, and the
    /// walker rejects more than 100 before the crate sees the file.)
    fn unbuildable_ignore() -> String {
        let mut alternation = String::from("{");
        let mut i = 0;
        while alternation.len() < 260_000 {
            alternation.push_str(&format!("*{i}*?,"));
            i += 1;
        }
        alternation.push_str("z}");
        let text = format!("secret*\n{alternation}\n");
        assert!(text.len() < 262_144, "the file must pass the size cap");
        text
    }

    #[test]
    fn bad_ignore_file_build_failure() {
        let unbuildable = unbuildable_ignore();

        // (1) in a subdirectory: the candidate file is reported (not the directory), `b` is not
        // walked, so `b/secret.key` is not yielded.
        {
            let sb = sandbox!();
            write(&sb.root, "ok/y.txt", "x");
            write(&sb.root, "b/secret.key", "x");
            write(&sb.root, "b/keep.txt", "x");
            write(&sb.root, "zz/last.txt", "x");
            write(&sb.root, "b/.ignore", &unbuildable);
            let (got, skips) = outcome(&sb, opts());
            assert_eq!(got, ["ok/y.txt", "zz/last.txt"]);
            assert_eq!(
                skips,
                [("b/.ignore".to_string(), SkipReason::BadIgnoreFile)]
            );
        }
        // (2) in the root: one skip at the file, nothing walked.
        {
            let sb = sandbox!();
            write(&sb.root, "secret.key", "x");
            write(&sb.root, "sub/b.txt", "x");
            write(&sb.root, ".ignore", &unbuildable);
            let all = run(&sb.root, opts());
            assert_eq!(
                all,
                [Walked::Skipped {
                    path: path_str(&sb.root.join(".ignore")),
                    reason: SkipReason::BadIgnoreFile
                }]
            );
        }
        // (3) in a grandparent of the root: one skip at the file, nothing walked.
        {
            let sb = sandbox!();
            let gp = sb.base().join("gp");
            let root = gp.join("mid/root");
            write(&root, "secret.key", "x");
            write(&gp, ".ignore", &unbuildable);
            let all = run(&root, opts());
            assert_eq!(
                all,
                [Walked::Skipped {
                    path: path_str(&gp.join(".ignore")),
                    reason: SkipReason::BadIgnoreFile
                }]
            );
        }
    }

    #[test]
    fn bad_ignore_file_invalid_utf8() {
        // `a.txt`, a byte that is not UTF-8, `b.txt`: the file is reported and its directory is
        // not walked (its valid lines are not applied).
        let sb = sandbox!();
        write(&sb.root, "ok.txt", "x");
        for n in ["a", "b", "c"] {
            write(&sb.root, &format!("sub/{n}.txt"), "x");
        }
        write(&sb.root, "sub/deeper/d.txt", "x");
        fs::write(sb.root.join("sub/.haliignore"), b"a.txt\n\xff\nb.txt\n").unwrap();
        let (got, skips) = outcome(&sb, opts());
        assert_eq!(got, ["ok.txt"]);
        assert_eq!(
            skips,
            [("sub/.haliignore".to_string(), SkipReason::BadIgnoreFile)]
        );

        // In the root: one skip, nothing walked; also for `.gitignore` inside a repository.
        drop(sb);
        let sb = sandbox!();
        make_repo(&sb.root);
        write(&sb.root, "ok.txt", "x");
        fs::write(sb.root.join(".gitignore"), b"a.txt\n\xff\nb.txt\n").unwrap();
        let all = run(&sb.root, opts());
        assert_eq!(
            all,
            [Walked::Skipped {
                path: path_str(&sb.root.join(".gitignore")),
                reason: SkipReason::BadIgnoreFile
            }]
        );
    }

    #[test]
    fn bad_ignore_file_in_parent() {
        // A parent of the root whose `.haliignore` is not UTF-8: one skip, nothing walked.
        let sb = sandbox!();
        write(&sb.root, "ok.txt", "x");
        write(&sb.root, "sub/b.txt", "x");
        fs::write(sb.base().join(".haliignore"), b"\xff\xfe\n").unwrap();
        let all = run(&sb.root, opts());
        assert_eq!(
            all,
            [Walked::Skipped {
                path: path_str(&sb.base().join(".haliignore")),
                reason: SkipReason::BadIgnoreFile
            }]
        );

        // A parent's ignore file with an invalid glob line keeps the old behaviour: reported
        // once, its valid lines still apply, the walk goes on.
        drop(sb);
        let sb = sandbox!();
        write(&sb.root, "keep.txt", "x");
        write(&sb.root, "p.txt", "x");
        write(&sb.base(), ".ignore", "p.txt\n{bad\n");
        let all = run(&sb.root, opts());
        assert_eq!(files(&sb.root, &all), ["keep.txt"]);
        assert_eq!(
            skipped_abs(&all),
            [(
                path_str(&sb.base().join(".ignore")),
                SkipReason::BadIgnoreFile
            )]
        );
    }

    #[test]
    fn unreadable_ignore_file_reported() {
        let sb = sandbox!();
        write(&sb.root, "ok.txt", "x");
        write(&sb.root, "sub/a.txt", "x");
        write(&sb.root, "sub/b.txt", "x");
        write(&sb.root, "sub/.haliignore", "a.txt\n");
        let ignore = sb.root.join("sub/.haliignore");
        let _restore = Restore(vec![ignore.clone()]);
        fs::set_permissions(&ignore, fs::Permissions::from_mode(0o000)).unwrap();
        let (got, skips) = outcome(&sb, opts());
        if is_root() {
            // Root can read a mode-000 file, so it is a good ignore file and applies.
            eprintln!(
                "NOTE: running as root: a mode-000 ignore file is readable, so this run asserts \
                 that it is applied, not that it is reported"
            );
            assert_eq!(got, ["ok.txt", "sub/b.txt"]);
            assert!(skips.is_empty(), "{skips:?}");
        } else {
            assert_eq!(got, ["ok.txt"]);
            assert_eq!(
                skips,
                [("sub/.haliignore".to_string(), SkipReason::BadIgnoreFile)]
            );
        }
    }

    // ---- 8. the ignore files behind `.git`, and other review round 2 cases ----

    /// A tree around `sub`: the files outside it are `OUTSIDE_SUB`.
    fn outside_sub(sb: &Sandbox) {
        write(&sb.root, "ok/y.txt", "x");
        write(&sb.root, "sub/x.txt", "x");
        write(&sb.root, "sub/deeper/z.txt", "x");
        write(&sb.root, "zz/last.txt", "x");
    }

    const OUTSIDE_SUB: [&str; 2] = ["ok/y.txt", "zz/last.txt"];

    /// Asserts one `BadIgnoreFile` skip, at any of `candidates` (the walker may name the
    /// `.git` file or the file it leads to), with only the files outside `sub` walked.
    fn assert_sub_skipped_at(sb: &Sandbox, all: &[Walked], candidates: &[PathBuf]) {
        assert_eq!(sorted(files(&sb.root, all)), OUTSIDE_SUB, "{all:?}");
        let skips = skipped_abs(all);
        assert_eq!(skips.len(), 1, "{all:?}");
        assert_eq!(skips[0].1, SkipReason::BadIgnoreFile);
        assert!(
            candidates.iter().any(|p| path_str(p) == skips[0].0),
            "the skip names one of {candidates:?}: {skips:?}"
        );
    }

    #[test]
    fn git_dir_info_exclude_fifo() {
        // `sub` is a repository with a real `.git` directory whose `info/exclude` is a FIFO.
        // The crate reads that file when it enters `sub`, so the walker must have looked first.
        {
            let sb = sandbox!();
            outside_sub(&sb);
            make_repo(&sb.root.join("sub"));
            mkfifo(&sb.root.join("sub/.git/info/exclude"));
            let (got, skips) = outcome(&sb, opts());
            assert_eq!(got, OUTSIDE_SUB);
            assert_eq!(
                skips,
                [(
                    "sub/.git/info/exclude".to_string(),
                    SkipReason::BadIgnoreFile
                )]
            );
        }
        // The root variant: one skip, nothing walked.
        {
            let sb = sandbox!();
            write(&sb.root, "a.txt", "x");
            make_repo(&sb.root);
            mkfifo(&sb.root.join(".git/info/exclude"));
            let all = run(&sb.root, opts());
            assert_eq!(
                all,
                [Walked::Skipped {
                    path: path_str(&sb.root.join(".git/info/exclude")),
                    reason: SkipReason::BadIgnoreFile
                }]
            );
        }
        // A device (`info/exclude -> /dev/zero`) is never read either: capped child.
        {
            let sb = sandbox!();
            outside_sub(&sb);
            make_repo(&sb.root.join("sub"));
            symlink("/dev/zero", sb.root.join("sub/.git/info/exclude")).unwrap();
            let all = run_capped(&sb.root).unwrap_or_else(|e| panic!("device case: {e}"));
            assert_eq!(sorted(files(&sb.root, &all)), OUTSIDE_SUB);
            assert_eq!(
                skipped(&sb.root, &all),
                [(
                    "sub/.git/info/exclude".to_string(),
                    SkipReason::BadIgnoreFile
                )]
            );
        }
    }

    #[test]
    fn git_commondir_fifo() {
        // `sub/.git` is a file `gitdir: <wt>`, and `<wt>/commondir` is itself a FIFO (the crate
        // reads it to find `info/exclude`). The skip names the `.git` file or the FIFO.
        {
            let sb = sandbox!();
            outside_sub(&sb);
            let wt = sb.base().join("wt");
            fs::create_dir_all(&wt).unwrap();
            write(&sb.root, "sub/.git", &format!("gitdir: {}\n", wt.display()));
            mkfifo(&wt.join("commondir"));
            let all = run(&sb.root, opts());
            assert_sub_skipped_at(&sb, &all, &[sb.root.join("sub/.git"), wt.join("commondir")]);
        }
        // The root variant: one skip, nothing walked.
        {
            let sb = sandbox!();
            write(&sb.root, "a.txt", "x");
            let wt = sb.base().join("wt");
            fs::create_dir_all(&wt).unwrap();
            write(&sb.root, ".git", &format!("gitdir: {}\n", wt.display()));
            mkfifo(&wt.join("commondir"));
            let all = run(&sb.root, opts());
            let skips = skipped_abs(&all);
            assert_eq!(all.len(), 1, "nothing is walked: {all:?}");
            assert_eq!(skips[0].1, SkipReason::BadIgnoreFile);
            assert!(
                [sb.root.join(".git"), wt.join("commondir")]
                    .iter()
                    .any(|p| path_str(p) == skips[0].0),
                "{skips:?}"
            );
        }
    }

    #[test]
    fn git_symlink_to_dir_with_fifo_exclude() {
        // `sub/.git` is a symlink to a directory outside the root, and that directory's
        // `info/exclude` is a FIFO. The crate follows the symlink to it.
        let sb = sandbox!();
        outside_sub(&sb);
        let store = sb.base().join("store");
        fs::create_dir_all(store.join("info")).unwrap();
        fs::write(store.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        mkfifo(&store.join("info/exclude"));
        symlink(&store, sb.root.join("sub/.git")).unwrap();
        let all = run(&sb.root, opts());
        assert_sub_skipped_at(
            &sb,
            &all,
            &[
                sb.root.join("sub/.git/info/exclude"),
                store.join("info/exclude"),
            ],
        );
    }

    #[test]
    fn git_entry_not_a_regular_file_or_directory() {
        // A `.git` that is a FIFO, a symlink to a file, or dangling is rejected, reported at
        // `.git`, and its directory is not walked.
        for kind in ["fifo", "symlink-to-file", "dangling"] {
            let sb = sandbox!();
            outside_sub(&sb);
            let git = sb.root.join("sub/.git");
            match kind {
                "fifo" => mkfifo(&git),
                "symlink-to-file" => {
                    write(&sb.base(), "plain-file", "gitdir: nowhere\n");
                    symlink(sb.base().join("plain-file"), &git).unwrap();
                }
                _ => symlink(sb.base().join("no-such-target"), &git).unwrap(),
            }
            let (got, skips) = outcome(&sb, opts());
            assert_eq!(got, OUTSIDE_SUB, "{kind}");
            assert_eq!(
                skips,
                [("sub/.git".to_string(), SkipReason::BadIgnoreFile)],
                "{kind}"
            );
        }
    }

    #[test]
    fn gitignore_fifo_outside_repo() {
        // Outside a repository the crate would not read `.gitignore`, but it is a candidate
        // like the others: a FIFO named so is reported and its directory not walked.
        {
            let sb = sandbox!();
            outside_sub(&sb);
            mkfifo(&sb.root.join("sub/.gitignore"));
            let (got, skips) = outcome(&sb, opts());
            assert_eq!(got, OUTSIDE_SUB);
            assert_eq!(
                skips,
                [("sub/.gitignore".to_string(), SkipReason::BadIgnoreFile)]
            );
        }
        {
            let sb = sandbox!();
            write(&sb.root, "a.txt", "x");
            mkfifo(&sb.root.join(".gitignore"));
            let all = run(&sb.root, opts());
            assert_eq!(
                all,
                [Walked::Skipped {
                    path: path_str(&sb.root.join(".gitignore")),
                    reason: SkipReason::BadIgnoreFile
                }]
            );
        }
    }

    #[test]
    fn ignore_file_in_0o111_dir() {
        // `locked` can be searched but not listed (mode 0o111): the walker can `lstat` the
        // FIFO `.ignore` inside it, so it reports that file, not an unreadable directory,
        // and opens nothing. As root permissions do not apply: skipped, loudly.
        if is_root() {
            eprintln!(
                "NOTE: SKIPPED ignore_file_in_0o111_dir: running as root, for whom a mode-0o111 \
                 directory is listable, so there is nothing to prove"
            );
            return;
        }
        let sb = sandbox!();
        write(&sb.root, "a.txt", "x");
        write(&sb.root, "locked/inner.txt", "x");
        write(&sb.root, "zz.txt", "x");
        mkfifo(&sb.root.join("locked/.ignore"));
        let _restore = Restore(vec![sb.root.join("locked")]);
        fs::set_permissions(sb.root.join("locked"), fs::Permissions::from_mode(0o111)).unwrap();
        let (got, skips) = outcome(&sb, opts());
        assert_eq!(got, ["a.txt", "zz.txt"]);
        assert_eq!(
            skips,
            [("locked/.ignore".to_string(), SkipReason::BadIgnoreFile)]
        );
    }

    #[test]
    fn symlink_to_locked_target() {
        // `link -> locked/f.txt`, with `locked` at mode 000: the link cannot be resolved for
        // lack of permission, which is `Unreadable`; `NotRegular` is for a link that leads to
        // nothing or to something that is not a regular file. As root permissions do not
        // apply: skipped, loudly.
        if is_root() {
            eprintln!(
                "NOTE: SKIPPED symlink_to_locked_target: running as root, for whom a mode-000 \
                 directory can be searched, so the link resolves"
            );
            return;
        }
        let sb = sandbox!();
        write(&sb.root, "a.txt", "x");
        write(&sb.root, "locked/f.txt", "x");
        symlink("locked/f.txt", sb.root.join("link")).unwrap();
        let _restore = Restore(vec![sb.root.join("locked")]);
        fs::set_permissions(sb.root.join("locked"), fs::Permissions::from_mode(0o000)).unwrap();
        let (got, skips) = outcome(&sb, opts());
        assert_eq!(got, ["a.txt"]);
        assert_eq!(
            skips,
            [
                ("link".to_string(), SkipReason::Unreadable),
                ("locked".to_string(), SkipReason::Unreadable),
            ]
        );
    }

    #[test]
    fn hidden_unhidden_by_whitelist_documented() {
        // Accepted and documented (spec §7): a `!` line in an ignore file can override the
        // hidden rule, so `!.*` un-hides `.env` although `include_hidden` is off. User excludes,
        // the built-ins and the data dir cannot be overridden that way. This test pins the
        // behaviour so a change to it is deliberate.
        let sb = sandbox!();
        let data = sb.root.join("data");
        write(
            &sb.root,
            ".ignore",
            "!.*\n!*.log\n!node_modules\n!data\n!target\n",
        );
        write(&sb.root, "a.txt", "x");
        write(&sb.root, ".env", "x");
        write(&sb.root, ".hid/h.txt", "x");
        write(&sb.root, "skip.log", "x");
        write(&sb.root, "node_modules/n.txt", "x");
        write(&sb.root, "sub/target/t.txt", "x");
        write(&sb.root, "data/d.txt", "x");
        let options = WalkOptions::new(false, false, vec!["*.log".to_string()], vec![data])
            .ok()
            .expect("valid options");
        let (got, skips) = outcome(&sb, options);
        assert_eq!(got, [".env", ".hid/h.txt", ".ignore", "a.txt"]);
        assert!(skips.is_empty(), "{skips:?}");

        // Without the whitelist line the hidden names are skipped again.
        write(&sb.root, ".ignore", "# nothing\n");
        let (got, _) = outcome(&sb, opts());
        assert_eq!(got, ["a.txt", "data/d.txt", "skip.log"]);
    }

    #[test]
    fn root_that_is_the_data_dir_is_walked() {
        // The crate never applies `filter_entry` at depth 0, so a root that is the data dir is
        // walked, not pruned. Accepted: #22 must refuse such a root. Pinned so a change is
        // deliberate.
        let sb = sandbox!();
        write(&sb.root, "a.txt", "x");
        write(&sb.root, "sub/b.txt", "x");
        let options = WalkOptions::new(false, false, vec![], vec![sb.root.clone()])
            .ok()
            .expect("valid options");
        let (got, skips) = outcome(&sb, options);
        assert_eq!(got, ["a.txt", "sub/b.txt"]);
        assert!(skips.is_empty(), "{skips:?}");
    }

    // ---- 9. the live ignore-rule budget ----
    //
    // The crate keeps the compiled matcher of every ancestor directory alive while the walk is
    // under it, so memory grows with depth. The walker keeps a running total of the bytes of
    // ignore files accepted along the CURRENT path (the root's parents included, each file once,
    // released on leaving a directory) and reports a candidate that would take it over 512 KiB
    // (524,288 bytes) as `bad_ignore_file` at that file, without walking its directory.
    // (The ignore files are dot files, so they are never yielded as files themselves.)

    /// A comment-only ignore file of exactly `n` bytes (`n >= 2`): it passes every check and
    /// costs nothing to compile, so only its size counts.
    fn comments(n: usize) -> String {
        assert!(n >= 2);
        format!("#{}\n", "c".repeat(n - 2))
    }

    /// An ignore file of about 250,000 bytes of `a*bN.x` lines (`N` distinct from every other
    /// `part`): it passes the size cap and the nesting bound, and it is expensive to compile
    /// (the crate holds the matcher for as long as the walk is under its directory).
    fn expensive_ignore(part: usize) -> String {
        let mut text = String::new();
        let mut n = part * 1_000_000;
        while text.len() < 249_990 {
            text.push_str(&format!("a*b{n}.x\n"));
            n += 1;
        }
        assert!(text.len() < 250_100, "{}", text.len());
        text
    }

    #[test]
    fn ignore_budget_nested() {
        // 8 nested directories, each with a ~250 KB accepted `.ignore`: the first two make
        // ~500 KB, the third crosses 524,288. Unbounded, the crate holds all eight compiled
        // matchers at once (gigabytes), so the capped child dies or hangs. Bounded, the walk
        // ends cleanly with one `bad_ignore_file` at the file that crosses the budget, and
        // nothing from that directory down is walked.
        let sb = sandbox!();
        write(&sb.root, "top.txt", "x");
        let mut dir = String::new();
        for i in 1..=8 {
            dir.push_str(&format!("d{i}/"));
            write(&sb.root, &format!("{dir}f.txt"), "x");
            write(&sb.root, &format!("{dir}.ignore"), &expensive_ignore(i));
        }
        let all = run_capped(&sb.root).unwrap_or_else(|e| panic!("nested case: {e}"));
        assert_eq!(
            sorted(files(&sb.root, &all)),
            ["d1/d2/f.txt", "d1/f.txt", "top.txt"]
        );
        assert_eq!(
            skipped(&sb.root, &all),
            [("d1/d2/d3/.ignore".to_string(), SkipReason::BadIgnoreFile)]
        );
    }

    #[test]
    fn ignore_budget_boundary() {
        // Exactly 524,288 bytes along one path is accepted, 524,289 is rejected, whichever
        // file takes the total there: one in a parent of the root, one in the root, one in a
        // subdirectory. (One file per directory: which file of a directory is counted first
        // is not part of the contract.)
        // (1) accepted: parent 262,144 + root 131,072 + sub 131,072 = 524,288.
        let sb = sandbox!();
        let gp = sb.base().join("gp");
        let root = gp.join("mid/root");
        write(&gp, ".ignore", &comments(262_144));
        write(&root, ".ignore", &comments(131_072));
        write(&root, "a.txt", "x");
        write(&root, "sub/x.txt", "x");
        write(&root, "sub/.ignore", &comments(131_072));
        let all = run(&root, opts());
        assert_eq!(sorted(files(&root, &all)), ["a.txt", "sub/x.txt"]);
        assert!(
            skipped_abs(&all).is_empty(),
            "exactly the budget passes: {all:?}"
        );

        // (2) one byte more in the subdirectory: reported there, `sub` not walked.
        write(&root, "sub/.ignore", &comments(131_073));
        let all = run(&root, opts());
        assert_eq!(files(&root, &all), ["a.txt"]);
        assert_eq!(
            skipped_abs(&all),
            [(
                path_str(&root.join("sub/.ignore")),
                SkipReason::BadIgnoreFile
            )]
        );

        // (3) one byte more in the root's own file (parent 262,144 + mid 131,072 + root
        // 131,073): one skip at the root's file, nothing walked.
        write(&gp, "mid/.ignore", &comments(131_072));
        write(&root, ".ignore", &comments(131_073));
        write(&root, "sub/.ignore", &comments(2));
        let all = run(&root, opts());
        assert_eq!(
            all,
            [Walked::Skipped {
                path: path_str(&root.join(".ignore")),
                reason: SkipReason::BadIgnoreFile
            }]
        );
        drop(sb);

        // (4) the file that crosses the budget is itself in a parent of the root: parent
        // 262,144 + mid 200,000 + mid2 62,145 = 524,289. One skip at mid2's file, nothing
        // walked.
        let sb = sandbox!();
        let gp = sb.base().join("gp");
        let root = gp.join("mid/mid2/root");
        write(&root, "a.txt", "x");
        write(&gp, ".ignore", &comments(262_144));
        write(&gp, "mid/.ignore", &comments(200_000));
        write(&gp, "mid/mid2/.ignore", &comments(62_145));
        let all = run(&root, opts());
        assert_eq!(
            all,
            [Walked::Skipped {
                path: path_str(&gp.join("mid/mid2/.ignore")),
                reason: SkipReason::BadIgnoreFile
            }]
        );
        // ... and 62,144 (exactly the budget) is accepted.
        write(&gp, "mid/mid2/.ignore", &comments(62_144));
        let all = run(&root, opts());
        assert_eq!(files(&root, &all), ["a.txt"]);
        assert!(skipped_abs(&all).is_empty(), "{all:?}");
    }

    #[test]
    fn ignore_budget_released() {
        // The budget is charged along the current path only. The root's own file (262,144)
        // stays charged all along; below it, eight sibling directories each hold two files of
        // 131,072 (a directory and its child: 262,144 + 262,144 = 524,288 exactly), and four
        // more siblings each hold one file at the per-file cap. Together that is far over
        // 524,288, so a walker that never gave a directory's bytes back would report most of
        // them; all of it is walked.
        let sb = sandbox!();
        write(&sb.root, ".ignore", &comments(262_144));
        write(&sb.root, "a.txt", "x");
        let mut expected = vec!["a.txt".to_string()];
        for i in 0..8 {
            write(&sb.root, &format!("s{i}/.ignore"), &comments(131_072));
            write(&sb.root, &format!("s{i}/inner/.ignore"), &comments(131_072));
            write(&sb.root, &format!("s{i}/inner/x.txt"), "x");
            expected.push(format!("s{i}/inner/x.txt"));
        }
        for i in 0..4 {
            write(&sb.root, &format!("k{i}/.haliignore"), &comments(262_144));
            write(&sb.root, &format!("k{i}/y.txt"), "x");
            expected.push(format!("k{i}/y.txt"));
        }
        let (got, skips) = outcome(&sb, opts());
        assert_eq!(got, sorted(expected));
        assert!(
            skips.is_empty(),
            "siblings are not charged for each other: {skips:?}"
        );
    }
}
