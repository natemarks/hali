//! Shared helpers of the #5 goal tests (store locking, atomic publish, store-file safety,
//! concurrency). A plain module, not a test target: callers declare `mod lockkit;`.
//!
//! Every run clears the environment and sets HALI_DATA_DIR / XDG_CONFIG_HOME / HOME to temp
//! dirs, so the real store and config are never read. The test process takes the store lock
//! itself with std's `File::lock*` (Rust 1.89), the way another hali process would.
#![allow(dead_code, unused_imports)]

use std::ffi::OsStr;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tempfile::TempDir;

pub const R1: &str = "0192f0c4-0000-7000-8000-000000000201";
pub const R2: &str = "0192f0c4-0000-7000-8000-000000000202";
pub const R3: &str = "0192f0c4-0000-7000-8000-000000000203";
pub const NOW: &str = "2026-10-24T09:30:00Z";
/// The most any one process of a test may run before the watchdog kills it.
pub const WATCHDOG: Duration = Duration::from_secs(90);

pub struct Out {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    /// From the spawn to the exit.
    pub elapsed: Duration,
}

pub struct Env {
    _tmp: TempDir,
    /// The canonical temp root; every path of a run lives below it.
    pub root: PathBuf,
}

pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

impl Env {
    /// An empty environment: the data dir does not exist.
    pub fn new() -> Env {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        for d in ["cfg", "home", "files"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        Env { _tmp: tmp, root }
    }

    /// The `store` fixture (vocabulary, resources and extensions) in a private data dir.
    pub fn with_store() -> Env {
        let env = Env::new();
        env.make_data_dir();
        for name in ["vocabulary.ttl", "resources.ttl", "extensions.ttl"] {
            let to = env.data().join(name);
            std::fs::copy(fixture("store").join(name), &to).unwrap();
            std::fs::set_permissions(&to, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        env
    }

    /// The `forget` fixture (three Resources R1..R3 and Concepts) with real files one.txt,
    /// two.txt and three.txt under files/.
    pub fn with_resources() -> Env {
        let env = Env::new();
        env.make_data_dir();
        for name in ["vocabulary.ttl", "resources.ttl"] {
            let text = std::fs::read_to_string(fixture("forget").join(name))
                .unwrap()
                .replace(
                    "file:///fixtures/forget/",
                    &format!("file://{}/files/", env.root.display()),
                );
            let to = env.data().join(name);
            std::fs::write(&to, text).unwrap();
            std::fs::set_permissions(&to, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        for name in ["one.txt", "two.txt", "three.txt"] {
            std::fs::write(env.root.join("files").join(name), b"hello\n").unwrap();
        }
        env
    }

    fn make_data_dir(&self) {
        std::fs::create_dir_all(self.data()).unwrap();
        std::fs::set_permissions(self.data(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }

    pub fn data(&self) -> PathBuf {
        self.root.join("data")
    }

    pub fn lock_path(&self) -> PathBuf {
        self.data().join(".lock")
    }

    pub fn file(&self, name: &str) -> PathBuf {
        self.root.join("files").join(name)
    }

    /// The command, with the environment cleared and pointed at the temp root. `HALI_TEST_NOW`
    /// and `HALI_TEST_ID_SEED` make a run deterministic.
    pub fn command<I, S>(&self, extra_env: &[(&str, &str)], args: I) -> Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_hali"));
        cmd.env_clear()
            .env("XDG_CONFIG_HOME", self.root.join("cfg"))
            .env("HOME", self.root.join("home"))
            .env("GIT_CONFIG_GLOBAL", self.root.join("gitconfig"))
            .env("GIT_CONFIG_SYSTEM", self.root.join("gitconfig-system"))
            .env("HALI_DATA_DIR", self.data())
            .env("HALI_TEST_NOW", NOW)
            .env("HALI_TEST_ID_SEED", "1")
            .current_dir(&self.root);
        for (k, v) in extra_env {
            cmd.env(k, v);
        }
        cmd.args(args);
        cmd
    }

    /// Starts a run with piped stdio; `finish` collects it.
    pub fn spawn<I, S>(&self, extra_env: &[(&str, &str)], args: I) -> Running
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut cmd = self.command(extra_env, args);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        Running {
            child: cmd.spawn().unwrap(),
            started: Instant::now(),
            root: self.root.to_str().unwrap().to_string(),
        }
    }

    /// Runs to the end with an empty stdin (no terminal).
    pub fn run<I, S>(&self, args: I) -> Out
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.run_env(&[], args)
    }

    pub fn run_env<I, S>(&self, extra_env: &[(&str, &str)], args: I) -> Out
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut running = self.spawn(extra_env, args);
        drop(running.child.stdin.take());
        running.finish()
    }

    /// Runs under `umask <mask>` (through `sh`, which then execs hali).
    pub fn run_umask<I, S>(&self, mask: &str, args: I) -> Out
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let inner = self.command(&[], args);
        let mut cmd = Command::new("/bin/sh");
        cmd.env_clear().current_dir(&self.root);
        for (k, v) in inner.get_envs() {
            if let Some(v) = v {
                cmd.env(k, v);
            }
        }
        cmd.arg("-c")
            .arg(format!("umask {mask}; exec \"$0\" \"$@\""))
            .arg(inner.get_program())
            .args(inner.get_args())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        Running {
            child: cmd.spawn().unwrap(),
            started: Instant::now(),
            root: self.root.to_str().unwrap().to_string(),
        }
        .finish()
    }

    /// Opens `<data>/.lock` (creating it, 0600) the way a test needs to hold it.
    pub fn open_lock(&self) -> File {
        if !self.data().exists() {
            self.make_data_dir();
        }
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(self.lock_path())
            .unwrap()
    }

    /// The test process takes the exclusive lock, as a writer would; dropping the file frees it.
    pub fn hold_exclusive(&self) -> File {
        let f = self.open_lock();
        f.lock().unwrap();
        f
    }

    /// ...and the shared lock, as a reader would.
    pub fn hold_shared(&self) -> File {
        let f = self.open_lock();
        f.lock_shared().unwrap();
        f
    }

    /// Names -> bytes of the files directly in the data dir, `.lock` left out.
    pub fn store_bytes(&self) -> std::collections::BTreeMap<String, Vec<u8>> {
        std::fs::read_dir(self.data())
            .unwrap()
            .map(|e| e.unwrap())
            .filter(|e| e.file_name() != ".lock")
            .map(|e| {
                (
                    e.file_name().into_string().unwrap(),
                    std::fs::read(e.path()).unwrap_or_default(),
                )
            })
            .collect()
    }

    pub fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.data().join(name)).unwrap()
    }
}

/// A started process.
pub struct Running {
    pub child: Child,
    started: Instant,
    root: String,
}

impl Running {
    /// Waits for the exit (a watchdog kills a process that runs past `WATCHDOG`, so a missing
    /// lock timeout fails the test instead of hanging it). The temp root is shown as `/TMP`.
    pub fn finish(self) -> Out {
        self.finish_within(WATCHDOG)
    }

    /// `finish` with a shorter watchdog, for runs that must not hang (a killed process has no
    /// exit code, so the caller's exit-code assertion fails).
    pub fn finish_within(self, limit: Duration) -> Out {
        let Running {
            child,
            started,
            root,
        } = self;
        let pid = child.id() as libc::pid_t;
        let done = Arc::new(AtomicBool::new(false));
        let flag = done.clone();
        let dog = std::thread::spawn(move || {
            let until = Instant::now() + limit;
            while Instant::now() < until {
                if flag.load(Ordering::SeqCst) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            if !flag.load(Ordering::SeqCst) {
                // SAFETY: signalling a child this test started and has not yet reaped.
                unsafe {
                    libc::kill(pid, libc::SIGKILL);
                }
            }
        });
        let out = child.wait_with_output().unwrap();
        let elapsed = started.elapsed();
        done.store(true, Ordering::SeqCst);
        dog.join().unwrap();
        let clean = |bytes: Vec<u8>| String::from_utf8_lossy(&bytes).replace(&root, "/TMP");
        Out {
            code: out.status.code(),
            stdout: clean(out.stdout),
            stderr: clean(out.stderr),
            elapsed,
        }
    }
}

/// The parsed one-line error envelope of a `--json` run, asserting nothing else is on stderr.
pub fn envelope(out: &Out) -> serde_json::Value {
    assert_eq!(out.stderr.matches('\n').count(), 1, "{:?}", out.stderr);
    serde_json::from_str(&out.stderr).unwrap_or_else(|e| panic!("not JSON ({e}): {:?}", out.stderr))
}

/// Asserts a `STORE_LOCKED` exit after the 5 s wait: exit 3, nothing on stdout, a JSON
/// envelope of that rule naming the lock file, and a wait of 5 s (a little less allowed for
/// the clock) up to 10 s.
pub fn assert_store_locked(out: &Out, what: &str) {
    assert_eq!(out.code, Some(3), "{what}: exit; stderr: {}", out.stderr);
    assert_eq!(out.stdout, "", "{what}");
    let v = envelope(out);
    assert_eq!(v["error"]["rule"], "STORE_LOCKED", "{what}: {v}");
    assert_eq!(v["error"]["details"]["path"], "/TMP/data/.lock", "{what}");
    assert_eq!(v["error"]["details"]["reason"], "timeout", "{what}");
    assert!(
        out.elapsed >= Duration::from_millis(4900),
        "{what}: gave up after only {:?}",
        out.elapsed
    );
    assert!(
        out.elapsed < Duration::from_secs(10),
        "{what}: took {:?}",
        out.elapsed
    );
}
