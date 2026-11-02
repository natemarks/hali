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
//!     // Err(the offending pattern) for an invalid glob or one starting with `!`
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
//! (byte order); a directory symlink is neither followed nor reported.
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
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, MutexGuard, mpsc};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    static ENV: Mutex<()> = Mutex::new(());

    const VARS: [&str; 4] = [
        "HOME",
        "XDG_CONFIG_HOME",
        "GIT_CONFIG_GLOBAL",
        "GIT_CONFIG_SYSTEM",
    ];

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
        fn new() -> Sandbox {
            let guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
            let dir = tempfile::Builder::new()
                .prefix("hali-walk-")
                .tempdir()
                .unwrap();
            let base = dir.path().canonicalize().unwrap();
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
            Sandbox {
                dir,
                root,
                home,
                xdg,
                saved,
                _guard: guard,
            }
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

    // ---- 1. hidden, and the shape of a yielded file ----

    #[test]
    fn hidden() {
        let sb = Sandbox::new();
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
            let sb = Sandbox::new();
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
            let sb = Sandbox::new();
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
            let sb = Sandbox::new();
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
            let sb = Sandbox::new();
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
            let sb = Sandbox::new();
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
        let sb = Sandbox::new();
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
        let sb = Sandbox::new();
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
        let sb = Sandbox::new();
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
        let sb = Sandbox::new();
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
        assert_eq!(err(&["*.log", "build/"]), None);
        assert_eq!(err(&[]), None);
    }

    // ---- 4. symlinks ----

    #[test]
    fn symlinks() {
        let sb = Sandbox::new();
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

        let (got, skips) = outcome(&sb, opts());
        // Each file once, at its real path, however many links lead to it; nothing is
        // yielded under a directory symlink; directory symlinks are not reported at all.
        assert_eq!(got, ["real/f.txt", "top.txt"]);
        assert_eq!(
            skips,
            [
                ("broken".to_string(), SkipReason::NotRegular),
                ("loop-a".to_string(), SkipReason::NotRegular),
                ("out-file".to_string(), SkipReason::OutsideRoot),
                ("zchain-loop".to_string(), SkipReason::NotRegular),
            ]
        );
        // The yielded paths are the real ones.
        for w in run(&sb.root, opts()) {
            if let Walked::File { path, .. } = w {
                assert!(!path.contains("link"), "{path}");
            }
        }
    }

    // ---- 5. special, unreadable and non-UTF-8 entries ----

    #[test]
    fn special_files() {
        let sb = Sandbox::new();
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
        // SAFETY: geteuid has no preconditions.
        if unsafe { libc::geteuid() } == 0 {
            eprintln!("skipped: root reads everything");
            return;
        }
        let sb = Sandbox::new();
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

        let (got, skips) = outcome(&sb, opts());
        // The walk continues past both, and nothing inside the locked directory is yielded.
        assert_eq!(got, ["a-ok.txt", "sub/deeper/last.txt", "z-ok.txt"]);
        assert_eq!(
            skips,
            [
                ("locked".to_string(), SkipReason::Unreadable),
                ("secret.txt".to_string(), SkipReason::Unreadable),
            ]
        );
    }

    #[test]
    fn non_utf8() {
        let sb = Sandbox::new();
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
        let sb = Sandbox::new();
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
        let sb = Sandbox::new();
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
}
