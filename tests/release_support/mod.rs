//! Shared support for the release goal tests (#33): a throwaway fixture repo with a local bare
//! `origin`, a `gh` network guard, and a fake `gh` for the scripts under test.
//!
//! Safety: nothing here ever contacts GitHub or touches this repository's refs. Every git command
//! runs inside a temporary directory with a temporary HOME; a `gh` shim on PATH exits 99, and the
//! scripts are pointed at `tests/fixtures/release/fake-gh` through `GH`.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::ffi::OsStr;
use std::fs;
use std::hash::{Hash, Hasher};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

/// Every fixture commit has this author and committer time (the tarball mtime is the commit time).
pub const GIT_DATE: &str = "2026-01-02T03:04:05+0000";
/// What `tar -tv` prints for that time when run with `TZ=UTC`.
pub const GIT_DATE_TAR: &str = "2026-01-02 03:04";
pub const ADR_PATH: &str = "docs/adr/0001-rust-turtle-canonical-oxigraph.md";
pub const FIXTURE_MAN: &str = "fixture man page\n";

pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn script(name: &str) -> PathBuf {
    repo_root().join("scripts").join(name)
}

pub fn fake_gh() -> PathBuf {
    repo_root().join("tests/fixtures/release/fake-gh")
}

pub fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

pub fn write_mode(path: &Path, content: &str, mode: u32) {
    write(path, content);
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

fn real_home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").expect("HOME is set"))
}

fn cargo_home() -> PathBuf {
    std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| real_home().join(".cargo"))
}

fn rustup_home() -> PathBuf {
    std::env::var_os("RUSTUP_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| real_home().join(".rustup"))
}

pub struct Run {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Run {
    pub fn ok(&self) -> bool {
        self.code == Some(0)
    }
    pub fn combined(&self) -> String {
        format!("{}{}", self.stdout, self.stderr)
    }
    pub fn dump(&self) -> String {
        format!(
            "exit {:?}\n--- stdout\n{}\n--- stderr\n{}",
            self.code, self.stdout, self.stderr
        )
    }
    /// The last non-empty stderr line: the message that says why a script stopped.
    pub fn last_stderr_line(&self) -> String {
        self.stderr
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("")
            .trim()
            .to_string()
    }
}

pub fn exec(cmd: &mut Command, what: &str) -> Run {
    let out = cmd
        .output()
        .unwrap_or_else(|e| panic!("cannot spawn {what}: {e}"));
    Run {
        code: out.status.code(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

/// A stable digest of a directory tree (relative path, kind, mode, content), `.git` and `target`
/// excluded unless asked for.
pub fn tree_listing(root: &Path, skip: &[&str]) -> BTreeMap<String, String> {
    fn walk(root: &Path, dir: &Path, skip: &[&str], out: &mut BTreeMap<String, String>) {
        let Ok(rd) = fs::read_dir(dir) else { return };
        for entry in rd {
            let entry = entry.unwrap();
            let path = entry.path();
            let rel = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            if skip.contains(&rel.as_str()) {
                continue;
            }
            let meta = fs::symlink_metadata(&path).unwrap();
            let mode = meta.permissions().mode() & 0o7777;
            if meta.file_type().is_symlink() {
                let target = fs::read_link(&path).unwrap();
                out.insert(rel, format!("symlink -> {}", target.display()));
            } else if meta.is_dir() {
                out.insert(rel, format!("dir {mode:o}"));
                walk(root, &path, skip, out);
            } else {
                let mut h = DefaultHasher::new();
                fs::read(&path).unwrap().hash(&mut h);
                out.insert(rel, format!("file {mode:o} {:x}", h.finish()));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, skip, &mut out);
    out
}

pub struct Opts {
    pub version: &'static str,
    /// Whether the ADR carries `## Benchmark results`.
    pub bench_heading: bool,
    /// Whether `test-hooks` is a default feature (so a plain release build has `HALI_TEST_`).
    pub default_hooks: bool,
}

pub struct Fixture {
    pub tmp: TempDir,
    pub work: PathBuf,
    pub origin: PathBuf,
    pub home: PathBuf,
    pub shim: PathBuf,
    pub gh_log: PathBuf,
    pub opts: Opts,
    /// Extra directories put on PATH ahead of the original PATH (after the `gh` guard).
    pub extra_path: Vec<PathBuf>,
    /// Extra environment for every command (answers for fake-gh, RELEASE_STATIC, ...).
    pub vars: Vec<(String, String)>,
    /// The commit the fake merged release PR points at (publish fixtures).
    pub merge_sha: String,
}

fn manifest(version: &str, default_hooks: bool) -> String {
    let default = if default_hooks {
        "default = [\"test-hooks\"]\n"
    } else {
        ""
    };
    format!(
        "[package]\nname = \"hali\"\nversion = \"{version}\"\nedition = \"2021\"\nlicense = \"MIT OR Apache-2.0\"\npublish = false\n\n[features]\ntest-hooks = []\n{default}\n[lints.rust]\nunexpected_cfgs = {{ level = \"allow\" }}\n"
    )
}

fn lock(version: &str) -> String {
    format!(
        "# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = 4\n\n[[package]]\nname = \"hali\"\nversion = \"{version}\"\n"
    )
}

const MAIN_RS: &str = r#"use std::env;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["--version"] => println!("hali {}", env!("CARGO_PKG_VERSION")),
        ["generate", "man"] => print!("fixture man page\n"),
        ["generate", "completions", shell] => println!("fixture completions for {shell}"),
        _ => {
            eprintln!("usage: hali --version | generate man | generate completions <shell>");
            std::process::exit(2);
        }
    }
    hook();
}

#[cfg(any(feature = "test-hooks", hali_hook))]
fn hook() {
    if env::var_os("HALI_FIXTURE_NEVER_SET").is_some() {
        println!("{}", std::hint::black_box("HALI_TEST_NOW"));
    }
}

#[cfg(not(any(feature = "test-hooks", hali_hook)))]
fn hook() {}
"#;

const MAKEFILE: &str = "static-check:\n\t@echo fixture static-check ok\n\nstatic-fail:\n\t@echo fixture static check failed >&2; exit 1\n\ndist:\n\t@\"$$HALI_REAL_SCRIPTS/dist.sh\"\n\n.PHONY: static-check static-fail dist\n";

impl Fixture {
    pub fn new(version: &'static str) -> Fixture {
        Fixture::with(Opts {
            version,
            bench_heading: true,
            default_hooks: false,
        })
    }

    pub fn with(opts: Opts) -> Fixture {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let work = root.join("work");
        let origin = root.join("origin.git");
        let home = root.join("home");
        let shim = root.join("shim");
        fs::create_dir_all(&work).unwrap();
        fs::create_dir_all(&home).unwrap();
        // The network guard: a `gh` on PATH that always fails with 99.
        write_mode(
            &shim.join("gh"),
            "#!/bin/sh\necho 'network guard: the real gh was called' >&2\nexit 99\n",
            0o755,
        );
        let fx = Fixture {
            gh_log: root.join("gh.log"),
            tmp,
            work,
            origin,
            home,
            shim,
            opts,
            extra_path: Vec::new(),
            vars: Vec::new(),
            merge_sha: String::new(),
        };
        fx.write_files();
        let mut init = fx.command("git");
        init.args(["init", "-q", "-b", "main"]);
        assert!(exec(&mut init, "git init").ok());
        fx.git(&["add", "-A"]);
        fx.git(&["commit", "-q", "-m", "initial"]);
        let origin = fx.origin.clone();
        let mut bare = fx.command("git");
        bare.args(["init", "-q", "--bare", "-b", "main"])
            .arg(&origin);
        assert!(exec(&mut bare, "git init --bare").ok());
        fx.git(&["remote", "add", "origin", origin.to_str().unwrap()]);
        fx.git(&["push", "-q", "-u", "origin", "main"]);
        fx
    }

    fn write_files(&self) {
        let w = &self.work;
        write(
            &w.join("Cargo.toml"),
            &manifest(self.opts.version, self.opts.default_hooks),
        );
        write(&w.join("Cargo.lock"), &lock(self.opts.version));
        write(&w.join("src/main.rs"), MAIN_RS);
        write(&w.join("Makefile"), MAKEFILE);
        write(&w.join(".gitignore"), "/target\n/dist\n");
        write(&w.join("README.md"), "fixture readme\n");
        write(&w.join("LICENSE-MIT"), "fixture MIT license\n");
        write(&w.join("LICENSE-APACHE"), "fixture Apache license\n");
        let adr = if self.opts.bench_heading {
            "# ADR 1\n\n## Benchmark results\n\nfixture numbers\n"
        } else {
            "# ADR 1\n\nno numbers yet\n"
        };
        write(&w.join(ADR_PATH), adr);
        let real = script("install.sh");
        let install = fs::read_to_string(&real)
            .unwrap_or_else(|_| "#!/bin/sh\necho 'fixture install.sh placeholder'\n".to_string());
        write_mode(&w.join("scripts/install.sh"), &install, 0o755);
    }

    pub fn path_var(&self) -> String {
        let mut dirs: Vec<PathBuf> = vec![self.shim.clone()];
        dirs.extend(self.extra_path.iter().cloned());
        let orig = std::env::var("PATH").unwrap_or_default();
        let mut s = dirs
            .iter()
            .map(|d| d.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(":");
        s.push(':');
        s.push_str(&orig);
        s
    }

    /// A command in the fixture work tree with a scrubbed environment and the network guards.
    pub fn command(&self, program: impl AsRef<OsStr>) -> Command {
        let mut c = Command::new(program);
        c.env_clear()
            .current_dir(&self.work)
            .env("PATH", self.path_var())
            .env("HOME", &self.home)
            .env("CARGO_HOME", cargo_home())
            .env("RUSTUP_HOME", rustup_home())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_AUTHOR_NAME", "Fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
            .env("GIT_COMMITTER_NAME", "Fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
            .env("GIT_AUTHOR_DATE", GIT_DATE)
            .env("GIT_COMMITTER_DATE", GIT_DATE)
            .env("TZ", "UTC")
            .env("LC_ALL", "C")
            .env("GH", fake_gh())
            .env("FAKE_GH_LOG", &self.gh_log)
            .env("FAKE_GH_ORIGIN", &self.origin)
            .env("RELEASE_STATIC", "static-check")
            .env("HALI_REAL_SCRIPTS", repo_root().join("scripts"));
        for (k, v) in &self.vars {
            c.env(k, v);
        }
        c
    }

    pub fn set_var(&mut self, key: &str, value: &str) {
        self.vars.retain(|(k, _)| k != key);
        self.vars.push((key.to_string(), value.to_string()));
    }

    /// Run a script of this repo (`scripts/<name>`) by absolute path in the fixture.
    pub fn run(&self, name: &str, vars: &[(&str, &str)]) -> Run {
        let path = script(name);
        let mut c = self.command(&path);
        for (k, v) in vars {
            c.env(k, v);
        }
        exec(
            &mut c,
            &format!("scripts/{name} (it must exist and be executable)"),
        )
    }

    pub fn git(&self, args: &[&str]) -> String {
        self.git_in(&self.work, args)
    }

    pub fn git_in(&self, dir: &Path, args: &[&str]) -> String {
        let mut c = self.command("git");
        c.current_dir(dir).args(args);
        let r = exec(&mut c, "git");
        assert!(r.ok(), "git {args:?} failed\n{}", r.dump());
        r.stdout.trim().to_string()
    }

    pub fn git_ok(&self, args: &[&str]) -> bool {
        let mut c = self.command("git");
        c.args(args);
        exec(&mut c, "git").ok()
    }

    pub fn origin_git(&self, args: &[&str]) -> String {
        let mut c = self.command("git");
        c.arg("--git-dir").arg(&self.origin).args(args);
        let r = exec(&mut c, "git");
        assert!(r.ok(), "origin git {args:?} failed\n{}", r.dump());
        r.stdout.trim().to_string()
    }

    pub fn head(&self) -> String {
        self.git(&["rev-parse", "HEAD"])
    }

    pub fn edit(&self, rel: &str, content: &str) {
        write(&self.work.join(rel), content);
    }

    pub fn commit_all(&self, msg: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", msg]);
    }

    pub fn push_main(&self) {
        self.git(&["push", "-q", "origin", "main"]);
    }

    pub fn commit_push(&self, msg: &str) {
        self.commit_all(msg);
        self.push_main();
    }

    pub fn set_version(&self, version: &str) {
        self.edit("Cargo.toml", &manifest(version, self.opts.default_hooks));
        self.edit("Cargo.lock", &lock(version));
    }

    /// The object (tag object for an annotated tag) a tag points at on origin, if any.
    pub fn origin_tag(&self, tag: &str) -> Option<String> {
        let mut c = self.command("git");
        c.arg("--git-dir").arg(&self.origin).args([
            "rev-parse",
            "-q",
            "--verify",
            &format!("refs/tags/{tag}"),
        ]);
        let r = exec(&mut c, "git");
        r.ok().then(|| r.stdout.trim().to_string())
    }

    pub fn local_tag(&self, tag: &str) -> Option<String> {
        let mut c = self.command("git");
        c.args(["rev-parse", "-q", "--verify", &format!("refs/tags/{tag}")]);
        let r = exec(&mut c, "git");
        r.ok().then(|| r.stdout.trim().to_string())
    }

    /// A second clone of origin, to push a commit that this work tree has not seen.
    pub fn push_from_other_clone(&self) {
        let other = self.tmp.path().join("other");
        let origin = self.origin.to_str().unwrap().to_string();
        let mut c = self.command("git");
        c.current_dir(self.tmp.path())
            .args(["clone", "-q", &origin, other.to_str().unwrap()]);
        assert!(exec(&mut c, "git clone").ok());
        write(&other.join("elsewhere.txt"), "pushed from another clone\n");
        self.git_in(&other, &["add", "-A"]);
        self.git_in(&other, &["commit", "-q", "-m", "elsewhere"]);
        self.git_in(&other, &["push", "-q", "origin", "main"]);
    }

    /// A publish-ready fixture: `version` is on `main` through a bump commit that a fake merged PR
    /// `release/v<version>` points at, with one more commit after it. With `prev`, tag `v<prev>`
    /// (annotated, pushed) marks the commit before the bump.
    pub fn publish_ready(
        version: &'static str,
        prev: Option<&'static str>,
        bench: bool,
    ) -> Fixture {
        let first = prev.unwrap_or(version);
        let mut fx = Fixture::with(Opts {
            version: first,
            bench_heading: bench,
            default_hooks: false,
        });
        if let Some(p) = prev {
            fx.git(&["tag", "-a", &format!("v{p}"), "-m", &format!("hali v{p}")]);
            fx.git(&["push", "-q", "origin", &format!("refs/tags/v{p}")]);
            fx.opts.version = version;
            fx.set_version(version);
            fx.commit_all(&format!("release: v{version}"));
        }
        fx.merge_sha = fx.head();
        fx.edit("README.md", "fixture readme, after the release PR\n");
        fx.commit_push("docs: after the release PR");
        let list = pr_list_json(version, &fx.merge_sha);
        fx.set_var("FAKE_GH_PR_LIST", &list);
        fx
    }

    /// Everything a refused or dry run must leave alone.
    pub fn snapshot(&self, local_tags: bool) -> String {
        let mut out = String::new();
        let refs = if local_tags {
            self.git(&[
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/heads",
                "refs/tags",
            ])
        } else {
            self.git(&[
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/heads",
            ])
        };
        out.push_str(&format!("refs:\n{refs}\n"));
        let head = {
            let mut c = self.command("git");
            c.args(["symbolic-ref", "-q", "HEAD"]);
            let r = exec(&mut c, "git");
            if r.ok() {
                r.stdout.trim().to_string()
            } else {
                "detached".to_string()
            }
        };
        out.push_str(&format!("HEAD: {head} {}\n", self.head()));
        out.push_str(&format!(
            "status:\n{}\n",
            self.git(&["status", "--porcelain=v1", "--untracked-files=all"])
        ));
        out.push_str(&format!(
            "origin:\n{}\n",
            self.origin_git(&["for-each-ref", "--format=%(refname) %(objectname)"])
        ));
        out.push_str(&format!(
            "tree: {:#?}\n",
            tree_listing(&self.work, &[".git", "target"])
        ));
        out
    }

    pub fn gh_calls(&self) -> Vec<String> {
        fs::read_to_string(&self.gh_log)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// Calls to fake-gh that would change something on GitHub.
    pub fn gh_mutations(&self) -> Vec<String> {
        const VERBS: [&str; 8] = [
            "pr create",
            "pr merge",
            "pr close",
            "pr edit",
            "release create",
            "release delete",
            "release upload",
            "release edit",
        ];
        self.gh_calls()
            .into_iter()
            .filter(|l| {
                let mut w = l.split_whitespace();
                let verb = format!("{} {}", w.next().unwrap_or(""), w.next().unwrap_or(""));
                VERBS.contains(&verb.as_str())
            })
            .collect()
    }

    pub fn gh_called(&self, verb: &str) -> bool {
        self.gh_calls().iter().any(|l| l.starts_with(verb))
    }

    /// A `cargo` wrapper on PATH that answers `cargo set-version --version` with `out` and `exit`,
    /// and passes every other call to the real cargo.
    pub fn shim_cargo_edit(&mut self, out: &str, exit: i32) {
        let dir = self.tmp.path().join("cargo-shim");
        let real = which("cargo");
        write_mode(
            &dir.join("cargo"),
            &format!(
                "#!/bin/sh\nif [ \"$1\" = set-version ] && [ \"${{2:-}}\" = --version ]; then\n  echo '{out}'\n  [ {exit} -eq 0 ] || echo 'error: no such command: `set-version`' >&2\n  exit {exit}\nfi\nexec '{}' \"$@\"\n",
                real.display()
            ),
            0o755,
        );
        self.extra_path.push(dir);
    }
}

pub fn which(program: &str) -> PathBuf {
    for dir in std::env::var("PATH").unwrap_or_default().split(':') {
        let p = Path::new(dir).join(program);
        if p.is_file() {
            return p;
        }
    }
    panic!("{program} is not on PATH");
}

pub fn pr_list_json(version: &str, merge_sha: &str) -> String {
    format!(
        "[{{\"number\":7,\"state\":\"MERGED\",\"headRefName\":\"release/v{version}\",\"baseRefName\":\"main\",\"mergeCommit\":{{\"oid\":\"{merge_sha}\"}}}}]"
    )
}

/// A refused run: non-zero (and not the guard's 99 or a missing command's 127), stderr names the
/// reason, and nothing changed. Returns the last stderr line, for the distinct-messages check.
pub fn assert_refused(
    fx: &Fixture,
    before: &str,
    r: &Run,
    name: &str,
    keywords: &[&str],
) -> String {
    assert!(
        !r.ok(),
        "{name}: expected a refusal but the script exited 0\n{}",
        r.dump()
    );
    assert!(
        r.code != Some(99) && r.code != Some(127),
        "{name}: exit {:?} is the network guard or a missing command, not a refusal\n{}",
        r.code,
        r.dump()
    );
    let err = r.stderr.to_lowercase();
    assert!(
        keywords.iter().any(|k| err.contains(&k.to_lowercase())),
        "{name}: stderr names none of {keywords:?}\n{}",
        r.dump()
    );
    // `before` was taken with or without local tags (a fetch may legitimately import tags).
    let unchanged = before == fx.snapshot(true) || before == fx.snapshot(false);
    assert!(
        unchanged,
        "{name}: a refused run changed the fixture\n{}",
        r.dump()
    );
    assert!(
        fx.gh_mutations().is_empty(),
        "{name}: a refused run reached a mutating gh verb: {:?}",
        fx.gh_mutations()
    );
    r.last_stderr_line()
}

/// Messages of different refusals must differ.
pub fn assert_distinct_messages(messages: &[(&str, String)]) {
    for (i, (ga, ma)) in messages.iter().enumerate() {
        for (gb, mb) in &messages[i + 1..] {
            if ga != gb {
                assert_ne!(
                    ma, mb,
                    "refusals '{ga}' and '{gb}' share the message {ma:?}"
                );
            }
        }
    }
}

/// One entry of `tar --numeric-owner -tvzf`.
#[derive(Debug)]
pub struct TarEntry {
    pub kind: char,
    pub mode: String,
    pub owner: String,
    pub time: String,
    pub name: String,
}

pub fn tar_listing(tarball: &Path) -> Vec<TarEntry> {
    let out = Command::new("tar")
        .env("TZ", "UTC")
        .args(["--numeric-owner", "-tvzf"])
        .arg(tarball)
        .output()
        .expect("tar");
    assert!(
        out.status.success(),
        "tar -t failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            TarEntry {
                kind: f[0].chars().next().unwrap(),
                mode: f[0][1..].to_string(),
                owner: f[1].to_string(),
                time: format!("{} {}", f[3], f[4]),
                name: f[5..].join(" "),
            }
        })
        .collect()
}

pub fn sha256sum_check(dir: &Path) -> Run {
    exec(
        Command::new("sha256sum")
            .current_dir(dir)
            .args(["-c", "SHA256SUMS"]),
        "sha256sum",
    )
}

pub fn extract(tarball: &Path, into: &Path) {
    fs::create_dir_all(into).unwrap();
    let st = Command::new("tar")
        .arg("xzf")
        .arg(tarball)
        .arg("-C")
        .arg(into)
        .status()
        .expect("tar");
    assert!(st.success());
}

/// Copy a script of this repo, failing clearly when it does not exist yet.
pub fn read_script(name: &str) -> String {
    fs::read_to_string(script(name)).unwrap_or_else(|e| panic!("scripts/{name} must exist: {e}"))
}
