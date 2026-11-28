//! Goal tests for `hali skill install` and the `skill` key of `hali describe`, through the real
//! binary (#28, spec §9.1, §9.4, §11.2, §11.3). Locked golden snapshots sit in tests/snapshots/
//! (hand-written from the approved contract, not from a run): `cli_skill__install_json` and
//! `cli_skill__dry_run_json` (stdout) and `cli_skill__error_json_envelope` (stderr). The three
//! `cli_describe__*` snapshots gained the `skill` key the same way.
//!
//! Every run clears the environment and sets HOME, XDG_CONFIG_HOME, XDG_DATA_HOME,
//! XDG_STATE_HOME and HALI_DATA_DIR to dirs of one temp root (a test says so when it removes or
//! changes one), so the real `~/.claude`, `~/.agents` and `~/.config/hali` are never touched.
//! The temp root is canonicalised and shown as `/TMP` in captured output. The binary version is
//! redacted at JSON level: a test asserts the value equals `env!("CARGO_PKG_VERSION")`, then
//! snapshots `"<VERSION>"`.
//!
//! Wording the contract leaves open and these tests fix (change them, with the human, if the
//! wording should differ):
//! - a created directory (`~/.claude`, `skills`, `hali`, `references`) is 0700, a written file
//!   0600, whatever the umask;
//! - `SKILL_DIR_UNUSABLE` `details.path` is the offending path: `$HOME/.claude` (a file) for
//!   `not_a_directory`, the symlink for `symlink`, the directory or file for `not_a_file`, the
//!   too-open directory for `unsafe`, `null` for `unresolved`; for `cannot_create` it is a path
//!   below the unwritable directory; the message of the golden is `cannot install the skill:
//!   <path> is not a directory; nothing was written` and its `suggested_fix` is `Move what is in
//!   the way aside, then run hali skill install again.`;
//! - a dry run reports the same `SKILL_DIR_UNUSABLE` errors as a real run for everything found
//!   while checking (including `cannot_create` when a directory cannot be opened); only a
//!   failure while writing, which a dry run never reaches, is specific to a real run;
//! - the text of `describe` gains `skill_installed` (`-` for none), `skill_binary` and
//!   `skill_stale` (`true` or `false`) lines straight after `relations`, before the lists;
//! - the real-run text is one `created|updated|unchanged skill file <path>` line per file, and
//!   under `--dry-run` `would create|would update skill file <path>` (an unchanged file still
//!   reads `unchanged skill file <path>`), with the shared stderr note and no `+` lines.

use assert_cmd::Command;
use std::path::Path;

/// A guard that lasts: `hali concept add -- --dry-run` adds a Concept labelled `--dry-run`. It is
/// why the skill pre-approves `--dry-run` only as the prefix `hali --dry-run` (spec §11.2). It
/// passes today and must keep passing.
mod guard {
    use super::*;

    #[test]
    fn suffix_dry_run_is_a_real_run() {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        for d in ["home", "cfg"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        let run = |args: &[&str]| {
            let out = Command::cargo_bin("hali")
                .unwrap()
                .env_clear()
                .env("HOME", root.join("home"))
                .env("XDG_CONFIG_HOME", root.join("cfg"))
                .env("HALI_DATA_DIR", Path::new(&root).join("data"))
                .args(args)
                .output()
                .unwrap();
            assert_eq!(
                out.status.code(),
                Some(0),
                "{args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()
        };
        let added = run(&["--json", "concept", "add", "--", "--dry-run"]);
        assert!(
            added.get("dry_run").is_none() && added.get("changes").is_none(),
            "a trailing `--dry-run` was taken for the flag: {added}"
        );
        assert_eq!(added["concept"]["label"], "--dry-run", "{added}");
        let listed = run(&["--json", "concept", "list"]);
        assert_eq!(listed["total"], 1, "{listed}");
        assert_eq!(listed["results"][0]["label"], "--dry-run", "{listed}");
    }
}

mod goal {
    use assert_cmd::Command;
    use serde_json::{Value, json};
    use std::collections::BTreeMap;
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime};
    use tempfile::TempDir;

    const VERSION: &str = env!("CARGO_PKG_VERSION");
    const NOTE: &str = "note: dry run; nothing was written\n";

    struct Out {
        code: Option<i32>,
        stdout: String,
        stderr: String,
    }

    #[derive(Debug, PartialEq)]
    struct Entry {
        kind: char,
        mode: u32,
        mtime: SystemTime,
        bytes: Vec<u8>,
        target: Option<PathBuf>,
    }

    struct Env {
        _tmp: TempDir,
        root: PathBuf,
    }

    impl Env {
        fn new() -> Env {
            let tmp = tempfile::tempdir().unwrap();
            let root = std::fs::canonicalize(tmp.path()).unwrap();
            for d in ["home", "cfg", "share", "state"] {
                std::fs::create_dir_all(root.join(d)).unwrap();
            }
            Env { _tmp: tmp, root }
        }

        fn home(&self) -> PathBuf {
            self.root.join("home")
        }

        /// `~/.claude/skills/hali`
        fn claude(&self) -> PathBuf {
            self.home().join(".claude/skills/hali")
        }

        /// `~/.agents/skills/hali`
        fn agents(&self) -> PathBuf {
            self.home().join(".agents/skills/hali")
        }

        fn data(&self) -> PathBuf {
            self.root.join("data")
        }

        fn cmd(&self, remove: &[&str], set: &[(&str, &OsStr)]) -> Command {
            let mut cmd = Command::cargo_bin("hali").unwrap();
            cmd.env_clear()
                .env("HOME", self.home())
                .env("XDG_CONFIG_HOME", self.root.join("cfg"))
                .env("XDG_DATA_HOME", self.root.join("share"))
                .env("XDG_STATE_HOME", self.root.join("state"))
                .env("HALI_DATA_DIR", self.data())
                .timeout(Duration::from_secs(30));
            for name in remove {
                cmd.env_remove(name);
            }
            for (name, value) in set {
                cmd.env(name, value);
            }
            cmd
        }

        fn clean(&self, bytes: Vec<u8>) -> String {
            String::from_utf8(bytes)
                .unwrap()
                .replace(self.root.to_str().unwrap(), "/TMP")
        }

        fn run_with<I, S>(&self, remove: &[&str], set: &[(&str, &OsStr)], args: I) -> Out
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            let out = self.cmd(remove, set).args(args).output().unwrap();
            Out {
                code: out.status.code(),
                stdout: self.clean(out.stdout),
                stderr: self.clean(out.stderr),
            }
        }

        fn run<I, S>(&self, args: I) -> Out
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            self.run_with(&[], &[], args)
        }

        /// Everything under the root: kind, mode, mtime, bytes of a regular file, link target.
        fn tree(&self) -> BTreeMap<String, Entry> {
            fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<String, Entry>) {
                for entry in std::fs::read_dir(dir).unwrap() {
                    let path = entry.unwrap().path();
                    let meta = std::fs::symlink_metadata(&path).unwrap();
                    let name = path.strip_prefix(root).unwrap().display().to_string();
                    let ft = meta.file_type();
                    let (kind, bytes, target) = if ft.is_symlink() {
                        ('l', Vec::new(), Some(std::fs::read_link(&path).unwrap()))
                    } else if ft.is_dir() {
                        ('d', Vec::new(), None)
                    } else if ft.is_file() {
                        ('f', std::fs::read(&path).unwrap(), None)
                    } else {
                        ('o', Vec::new(), None)
                    };
                    out.insert(
                        name,
                        Entry {
                            kind,
                            mode: meta.permissions().mode() & 0o7777,
                            mtime: meta.modified().unwrap(),
                            bytes,
                            target,
                        },
                    );
                    if ft.is_dir() {
                        walk(&path, root, out);
                    }
                }
            }
            let mut out = BTreeMap::new();
            walk(&self.root, &self.root, &mut out);
            out
        }
    }

    /// Puts a directory back to 0700 when dropped, so the temp dir can be removed.
    struct Restore(PathBuf);

    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o700));
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

    fn mode(path: &Path) -> u32 {
        std::fs::symlink_metadata(path)
            .unwrap()
            .permissions()
            .mode()
            & 0o7777
    }

    fn mkdir(path: &Path, mode: u32) {
        std::fs::create_dir_all(path).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    fn mkfifo(path: &Path) {
        let c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        // SAFETY: `c` is a valid NUL-terminated path for the duration of the call.
        let r = unsafe { libc::mkfifo(c.as_ptr(), 0o600) };
        assert_eq!(r, 0, "mkfifo failed");
    }

    fn is_root() -> bool {
        // SAFETY: geteuid has no preconditions.
        unsafe { libc::geteuid() == 0 }
    }

    /// The text of `skill/SKILL.md` with the version filled in, from the file itself.
    fn expected_skill() -> String {
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("skill/SKILL.md"))
            .expect("skill/SKILL.md exists")
            .replace("{{HALI_VERSION}}", VERSION)
    }

    fn guide() -> Vec<u8> {
        std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/agent-usage.md")).unwrap()
    }

    fn print(env: &Env) -> String {
        let out = env.run(["skill", "install", "--print"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        out.stdout
    }

    /// The JSON of an install with the binary version redacted (after checking it).
    fn redact_version(out: &Out) -> Out {
        let mut v = json(&out.stdout);
        assert_eq!(v["version"], VERSION, "{v}");
        v["version"] = json!("<VERSION>");
        Out {
            code: out.code,
            stdout: format!("{}\n", serde_json::to_string(&v).unwrap()),
            stderr: out.stderr.clone(),
        }
    }

    fn snapshot(name: &str, out: &Out, stream: &str) {
        let (shown, other) = match stream {
            "stdout" => (&out.stdout, &out.stderr),
            _ => (&out.stderr, &out.stdout),
        };
        assert_eq!(other, "", "nothing on the other stream");
        // The locked snapshots are named without the `goal__` module segment.
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!(name, shown);
        });
    }

    fn files_of(v: &Value) -> Vec<(String, String)> {
        v["files"]
            .as_array()
            .unwrap_or_else(|| panic!("no files in {v}"))
            .iter()
            .map(|f| {
                (
                    f["path"].as_str().unwrap().to_string(),
                    f["action"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    }

    fn expect_files(v: &Value, expected: &[(&str, &str)]) {
        let want: Vec<(String, String)> = expected
            .iter()
            .map(|(p, a)| (p.to_string(), a.to_string()))
            .collect();
        assert_eq!(files_of(v), want, "{v}");
    }

    const CLAUDE_SKILL: &str = "/TMP/home/.claude/skills/hali/SKILL.md";
    const CLAUDE_USAGE: &str = "/TMP/home/.claude/skills/hali/references/usage.md";
    const AGENTS_SKILL: &str = "/TMP/home/.agents/skills/hali/SKILL.md";
    const AGENTS_USAGE: &str = "/TMP/home/.agents/skills/hali/references/usage.md";

    // ---- criterion: install writes the claude location -------------------------------------

    #[test]
    fn install_claude_location() {
        let env = Env::new();
        let out = env.run(["skill", "install", "--json"]);
        let v = ok(&out);
        assert_eq!(v["schema_version"], 1);
        expect_files(&v, &[(CLAUDE_SKILL, "created"), (CLAUDE_USAGE, "created")]);
        let skill = std::fs::read_to_string(env.claude().join("SKILL.md")).unwrap();
        assert_eq!(skill, print(&env), "SKILL.md is what --print shows");
        assert_eq!(skill, expected_skill());
        assert_eq!(
            std::fs::read(env.claude().join("references/usage.md")).unwrap(),
            guide(),
            "references/usage.md is docs/agent-usage.md byte for byte"
        );
        for dir in [
            env.home().join(".claude"),
            env.home().join(".claude/skills"),
            env.claude(),
            env.claude().join("references"),
        ] {
            assert_eq!(mode(&dir), 0o700, "{}", dir.display());
        }
        for file in ["SKILL.md", "references/usage.md"] {
            assert_eq!(mode(&env.claude().join(file)), 0o600, "{file}");
        }
        assert!(
            !env.home().join(".agents").exists(),
            "~/.agents is written only with --agents"
        );
        // Nothing but the two directories' files: no temp file is left behind.
        let names: Vec<String> = env
            .tree()
            .into_keys()
            .filter(|k| k.starts_with("home/.claude/skills/hali"))
            .collect();
        assert_eq!(
            names,
            [
                "home/.claude/skills/hali",
                "home/.claude/skills/hali/SKILL.md",
                "home/.claude/skills/hali/references",
                "home/.claude/skills/hali/references/usage.md"
            ]
        );
        // hali's own dirs are not touched: no store, no lock, no state.
        assert!(!env.data().exists(), "install created the data dir");
        assert_eq!(
            std::fs::read_dir(env.root.join("state")).unwrap().count(),
            0,
            "install wrote state"
        );
    }

    #[test]
    fn install_json() {
        let env = Env::new();
        let out = env.run(["skill", "install", "--json"]);
        ok(&out);
        snapshot("cli_skill__install_json", &redact_version(&out), "stdout");
    }

    #[test]
    fn install_text() {
        let env = Env::new();
        let out = env.run(["skill", "install"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        assert_eq!(
            out.stdout,
            format!("created skill file {CLAUDE_SKILL}\ncreated skill file {CLAUDE_USAGE}\n")
        );
        let again = env.run(["skill", "install"]);
        assert_eq!(
            again.stdout,
            format!("unchanged skill file {CLAUDE_SKILL}\nunchanged skill file {CLAUDE_USAGE}\n")
        );
    }

    #[test]
    fn symlinked_claude_is_followed() {
        // A symlinked ~/.claude (or ~/.agents, or skills) is the user's choice: it is followed.
        let env = Env::new();
        let real = env.root.join("dotfiles-claude");
        std::fs::create_dir_all(&real).unwrap();
        symlink(&real, env.home().join(".claude")).unwrap();
        let v = ok(&env.run(["skill", "install", "--json"]));
        expect_files(&v, &[(CLAUDE_SKILL, "created"), (CLAUDE_USAGE, "created")]);
        assert!(real.join("skills/hali/SKILL.md").is_file());
        assert!(real.join("skills/hali/references/usage.md").is_file());
    }

    // ---- criterion: --agents -----------------------------------------------------------------

    #[test]
    fn agents_location() {
        let env = Env::new();
        let v = ok(&env.run(["skill", "install", "--agents", "--json"]));
        expect_files(
            &v,
            &[
                (CLAUDE_SKILL, "created"),
                (CLAUDE_USAGE, "created"),
                (AGENTS_SKILL, "created"),
                (AGENTS_USAGE, "created"),
            ],
        );
        for base in [env.claude(), env.agents()] {
            assert_eq!(
                std::fs::read_to_string(base.join("SKILL.md")).unwrap(),
                expected_skill(),
                "{}",
                base.display()
            );
            assert_eq!(
                std::fs::read(base.join("references/usage.md")).unwrap(),
                guide()
            );
            assert_eq!(mode(&base), 0o700);
            assert_eq!(mode(&base.join("references")), 0o700);
            assert_eq!(mode(&base.join("SKILL.md")), 0o600);
            assert_eq!(mode(&base.join("references/usage.md")), 0o600);
        }
        assert_eq!(mode(&env.home().join(".agents")), 0o700);
    }

    // ---- criterion: --print ------------------------------------------------------------------

    #[test]
    fn print_writes_nothing() {
        let env = Env::new();
        let before = env.tree();
        let text = print(&env);
        assert_eq!(
            text,
            expected_skill(),
            "--print is skill/SKILL.md, rendered"
        );
        assert!(text.starts_with("---\n"));
        // `--json` and `--dry-run` change nothing: the text is the same.
        for args in [
            &["skill", "install", "--print", "--json"][..],
            &["--json", "skill", "install", "--print"][..],
            &["--dry-run", "skill", "install", "--print"][..],
        ] {
            let out = env.run(args);
            assert_eq!(out.code, Some(0), "{args:?}: {}", out.stderr);
            assert_eq!(out.stdout, text, "{args:?}");
            assert_eq!(out.stderr, "", "{args:?}");
        }
        // It resolves no directory, so it needs no HOME.
        let out = env.run_with(&["HOME"], &[], ["skill", "install", "--print"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stdout, text);
        assert_eq!(env.tree(), before, "--print wrote");
    }

    #[test]
    fn print_conflicts_with_agents() {
        let env = Env::new();
        let before = env.tree();
        for args in [
            &["skill", "install", "--print", "--agents"][..],
            &["skill", "install", "--agents", "--print"][..],
        ] {
            let out = env.run(args);
            assert_eq!(out.code, Some(2), "{args:?}: {}", out.stderr);
            assert_eq!(out.stdout, "", "{args:?}");
            assert!(!out.stderr.is_empty(), "{args:?}");
        }
        assert_eq!(env.tree(), before);
    }

    // ---- criterion: idempotence, modification, foreign files ---------------------------------

    #[test]
    fn rerun_unchanged() {
        let env = Env::new();
        ok(&env.run(["skill", "install", "--agents", "--json"]));
        let before = env.tree();
        std::thread::sleep(Duration::from_millis(30));
        let v = ok(&env.run(["skill", "install", "--agents", "--json"]));
        expect_files(
            &v,
            &[
                (CLAUDE_SKILL, "unchanged"),
                (CLAUDE_USAGE, "unchanged"),
                (AGENTS_SKILL, "unchanged"),
                (AGENTS_USAGE, "unchanged"),
            ],
        );
        assert_eq!(env.tree(), before, "a rerun changed a file or its mtime");
    }

    #[test]
    fn modified_updated() {
        let env = Env::new();
        ok(&env.run(["skill", "install", "--json"]));
        let skill = env.claude().join("SKILL.md");
        let usage = env.claude().join("references/usage.md");
        std::fs::write(&skill, "my own edits\n").unwrap();
        std::fs::set_permissions(&skill, std::fs::Permissions::from_mode(0o644)).unwrap();
        let usage_before = std::fs::metadata(&usage).unwrap().modified().unwrap();
        std::thread::sleep(Duration::from_millis(30));

        let v = ok(&env.run(["skill", "install", "--json"]));
        expect_files(
            &v,
            &[(CLAUDE_SKILL, "updated"), (CLAUDE_USAGE, "unchanged")],
        );
        assert_eq!(std::fs::read_to_string(&skill).unwrap(), expected_skill());
        assert_eq!(mode(&skill), 0o600, "an updated file is 0600");
        assert_eq!(
            std::fs::metadata(&usage).unwrap().modified().unwrap(),
            usage_before,
            "the unchanged file was rewritten"
        );

        // The guide too: a truncated copy is updated.
        std::fs::write(&usage, "x").unwrap();
        let v = ok(&env.run(["skill", "install", "--json"]));
        expect_files(
            &v,
            &[(CLAUDE_SKILL, "unchanged"), (CLAUDE_USAGE, "updated")],
        );
        assert_eq!(std::fs::read(&usage).unwrap(), guide());
    }

    #[test]
    fn foreign_file_untouched() {
        let env = Env::new();
        ok(&env.run(["skill", "install", "--json"]));
        let mine = env.claude().join("my-notes.md");
        let mine_ref = env.claude().join("references/mine.md");
        std::fs::write(&mine, "keep me\n").unwrap();
        std::fs::write(&mine_ref, "keep me too\n").unwrap();
        let before = env.tree();
        std::thread::sleep(Duration::from_millis(30));
        ok(&env.run(["skill", "install", "--json"]));
        assert_eq!(std::fs::read_to_string(&mine).unwrap(), "keep me\n");
        assert_eq!(std::fs::read_to_string(&mine_ref).unwrap(), "keep me too\n");
        assert_eq!(env.tree(), before, "install touched a file it does not own");
    }

    // ---- criterion: --dry-run ----------------------------------------------------------------

    #[test]
    fn dry_run_json() {
        let env = Env::new();
        let before = env.tree();
        let out = env.run(["--dry-run", "skill", "install", "--json"]);
        let v = ok(&out);
        assert_eq!(v["dry_run"], true);
        assert_eq!(v["changes"], json!([]));
        assert_eq!(env.tree(), before, "a dry run wrote");
        snapshot("cli_skill__dry_run_json", &redact_version(&out), "stdout");
        // Its payload is the real run's, plus the two keys.
        let real = ok(&env.run(["skill", "install", "--json"]));
        let mut dry = v.clone();
        let keys = dry.as_object_mut().unwrap();
        keys.remove("dry_run");
        keys.remove("changes");
        assert_eq!(dry, real);
    }

    #[test]
    fn dry_run_text() {
        let env = Env::new();
        let before = env.tree();
        let out = env.run(["--dry-run", "skill", "install", "--agents"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(
            out.stdout,
            format!(
                "would create skill file {CLAUDE_SKILL}\nwould create skill file {CLAUDE_USAGE}\n\
                 would create skill file {AGENTS_SKILL}\nwould create skill file {AGENTS_USAGE}\n"
            )
        );
        assert_eq!(out.stderr, NOTE);
        assert_eq!(env.tree(), before, "a dry run wrote");
        // After an install with one edited file, a dry run says update, and still writes nothing.
        ok(&env.run(["skill", "install", "--json"]));
        std::fs::write(env.claude().join("SKILL.md"), "edited\n").unwrap();
        let before = env.tree();
        let out = env.run(["skill", "install", "--dry-run"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(
            out.stdout,
            format!(
                "would update skill file {CLAUDE_SKILL}\nunchanged skill file {CLAUDE_USAGE}\n"
            )
        );
        assert_eq!(out.stderr, NOTE);
        assert_eq!(env.tree(), before, "a dry run wrote");
    }

    // ---- criterion: SKILL_DIR_UNUSABLE -------------------------------------------------------

    /// A failed run: exit 3, nothing on stdout, the `SKILL_DIR_UNUSABLE` envelope on stderr.
    fn unusable(out: &Out, reason: &str) -> Value {
        assert_eq!(out.code, Some(3), "stderr: {}", out.stderr);
        assert_eq!(out.stdout, "");
        let v = json(&out.stderr);
        assert_eq!(v["error"]["rule"], "SKILL_DIR_UNUSABLE", "{}", out.stderr);
        assert_eq!(v["error"]["details"]["reason"], reason, "{}", out.stderr);
        assert!(
            v["error"]["details"].get("path").is_some(),
            "no path: {}",
            out.stderr
        );
        assert!(!v["error"]["message"].as_str().unwrap().is_empty());
        assert!(!v["error"]["suggested_fix"].as_str().unwrap().is_empty());
        v["error"].clone()
    }

    /// Runs `skill install` for real and as a dry run on the prepared env; both must give the
    /// same `reason` and `path`, exit 3, identical stderr, and write nothing.
    fn assert_unusable(env: &Env, reason: &str, path: Option<&str>, extra: &[&str]) {
        let before = env.tree();
        let mut real_args = vec!["--json", "skill", "install"];
        real_args.extend_from_slice(extra);
        let real = env.run(&real_args);
        let e = unusable(&real, reason);
        match path {
            Some(p) => assert_eq!(e["details"]["path"], p, "{}", real.stderr),
            None => assert_eq!(e["details"]["path"], Value::Null, "{}", real.stderr),
        }
        assert_eq!(env.tree(), before, "{reason}: a failed install wrote");
        let mut dry_args = vec!["--dry-run", "--json", "skill", "install"];
        dry_args.extend_from_slice(extra);
        let dry = env.run(&dry_args);
        assert_eq!(dry.code, real.code, "{reason}: dry run exit");
        assert_eq!(dry.stdout, "", "{reason}: dry run stdout");
        assert_eq!(dry.stderr, real.stderr, "{reason}: dry run stderr");
        assert_eq!(env.tree(), before, "{reason}: a dry run wrote");
    }

    #[test]
    fn error_unresolved() {
        for (what, remove, set) in [
            ("no HOME", vec!["HOME"], None),
            ("empty HOME", vec![], Some("")),
            ("relative HOME", vec![], Some("home")),
        ] {
            let env = Env::new();
            let before = env.tree();
            let set_vars: Vec<(&str, &OsStr)> =
                set.map(|v| ("HOME", OsStr::new(v))).into_iter().collect();
            let real = env.run_with(&remove, &set_vars, ["--json", "skill", "install"]);
            let e = unusable(&real, "unresolved");
            assert_eq!(e["details"]["path"], Value::Null, "{what}: {}", real.stderr);
            let dry = env.run_with(
                &remove,
                &set_vars,
                ["--dry-run", "--json", "skill", "install"],
            );
            assert_eq!(dry.code, real.code, "{what}");
            assert_eq!(dry.stderr, real.stderr, "{what}");
            assert_eq!(env.tree(), before, "{what}: wrote");
            // --agents resolves the same way.
            let agents = env.run_with(
                &remove,
                &set_vars,
                ["--json", "skill", "install", "--agents"],
            );
            unusable(&agents, "unresolved");
        }
    }

    #[test]
    fn error_not_a_directory() {
        // ~/.claude is a file.
        let env = Env::new();
        std::fs::write(env.home().join(".claude"), "a file\n").unwrap();
        assert_unusable(&env, "not_a_directory", Some("/TMP/home/.claude"), &[]);

        // skills is a file.
        let env = Env::new();
        std::fs::create_dir_all(env.home().join(".claude")).unwrap();
        std::fs::write(env.home().join(".claude/skills"), "a file\n").unwrap();
        assert_unusable(
            &env,
            "not_a_directory",
            Some("/TMP/home/.claude/skills"),
            &[],
        );

        // ~/.agents is a file: with --agents nothing is written, not even the claude location.
        let env = Env::new();
        std::fs::write(env.home().join(".agents"), "a file\n").unwrap();
        assert_unusable(
            &env,
            "not_a_directory",
            Some("/TMP/home/.agents"),
            &["--agents"],
        );
        assert!(
            !env.home().join(".claude").exists(),
            "the claude location was written before the agents location was checked"
        );
    }

    #[test]
    fn error_symlink() {
        // hali/ is a symlink to a directory elsewhere.
        let env = Env::new();
        let elsewhere = env.root.join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::create_dir_all(env.home().join(".claude/skills")).unwrap();
        symlink(&elsewhere, env.claude()).unwrap();
        assert_unusable(&env, "symlink", Some("/TMP/home/.claude/skills/hali"), &[]);
        assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0);

        // references/ is a symlink.
        let env = Env::new();
        let elsewhere = env.root.join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        mkdir(&env.claude(), 0o700);
        symlink(&elsewhere, env.claude().join("references")).unwrap();
        assert_unusable(
            &env,
            "symlink",
            Some("/TMP/home/.claude/skills/hali/references"),
            &[],
        );
        assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0);
        assert!(!env.claude().join("SKILL.md").exists());

        // SKILL.md is a symlink, to a file that stays as it is; so is a dangling one.
        for (what, target_exists) in [("a file", true), ("nothing", false)] {
            let env = Env::new();
            mkdir(&env.claude(), 0o700);
            let target = env.root.join("target.md");
            if target_exists {
                std::fs::write(&target, "do not touch\n").unwrap();
            }
            symlink(&target, env.claude().join("SKILL.md")).unwrap();
            assert_unusable(
                &env,
                "symlink",
                Some("/TMP/home/.claude/skills/hali/SKILL.md"),
                &[],
            );
            if target_exists {
                assert_eq!(
                    std::fs::read_to_string(&target).unwrap(),
                    "do not touch\n",
                    "a symlink to {what} was written through"
                );
            } else {
                assert!(!target.exists(), "a dangling symlink was written through");
            }
        }

        // references/usage.md is a symlink: SKILL.md is not written first.
        let env = Env::new();
        mkdir(&env.claude().join("references"), 0o700);
        let target = env.root.join("target.md");
        std::fs::write(&target, "do not touch\n").unwrap();
        symlink(&target, env.claude().join("references/usage.md")).unwrap();
        assert_unusable(
            &env,
            "symlink",
            Some("/TMP/home/.claude/skills/hali/references/usage.md"),
            &[],
        );
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "do not touch\n");
        assert!(
            !env.claude().join("SKILL.md").exists(),
            "SKILL.md was written before usage.md was checked"
        );
    }

    #[test]
    fn error_not_a_file() {
        // A directory where SKILL.md goes.
        let env = Env::new();
        mkdir(&env.claude().join("SKILL.md"), 0o700);
        assert_unusable(
            &env,
            "not_a_file",
            Some("/TMP/home/.claude/skills/hali/SKILL.md"),
            &[],
        );

        // A directory where usage.md goes.
        let env = Env::new();
        mkdir(&env.claude().join("references/usage.md"), 0o700);
        assert_unusable(
            &env,
            "not_a_file",
            Some("/TMP/home/.claude/skills/hali/references/usage.md"),
            &[],
        );
        assert!(!env.claude().join("SKILL.md").exists());

        // A FIFO where SKILL.md goes: it is refused, not opened for reading.
        let env = Env::new();
        mkdir(&env.claude(), 0o700);
        mkfifo(&env.claude().join("SKILL.md"));
        assert_unusable(
            &env,
            "not_a_file",
            Some("/TMP/home/.claude/skills/hali/SKILL.md"),
            &[],
        );
    }

    #[test]
    fn error_unsafe() {
        for (what, dir_mode) in [("group-writable", 0o770), ("world-writable", 0o707)] {
            let env = Env::new();
            mkdir(&env.claude(), dir_mode);
            assert_unusable(&env, "unsafe", Some("/TMP/home/.claude/skills/hali"), &[]);
            assert!(
                !env.claude().join("SKILL.md").exists(),
                "{what}: a file was written into an unsafe directory"
            );
        }
        // references/ is written into too.
        let env = Env::new();
        mkdir(&env.claude(), 0o700);
        mkdir(&env.claude().join("references"), 0o770);
        assert_unusable(
            &env,
            "unsafe",
            Some("/TMP/home/.claude/skills/hali/references"),
            &[],
        );
        assert!(!env.claude().join("SKILL.md").exists());
    }

    #[test]
    fn error_cannot_create() {
        if is_root() {
            eprintln!("skipping error_cannot_create: running as root, 0500 does not stop root");
            return;
        }
        let env = Env::new();
        let skills = env.home().join(".claude/skills");
        mkdir(&skills, 0o700);
        let _restore = Restore(skills.clone());
        std::fs::set_permissions(&skills, std::fs::Permissions::from_mode(0o500)).unwrap();
        let before = env.tree();
        let out = env.run(["--json", "skill", "install"]);
        let e = unusable(&out, "cannot_create");
        let path = e["details"]["path"].as_str().expect("a path string");
        assert!(
            path.starts_with("/TMP/home/.claude/skills"),
            "path {path:?} is not below the unwritable directory"
        );
        assert_eq!(env.tree(), before, "a failed install wrote");
    }

    #[test]
    fn error_json_envelope() {
        let env = Env::new();
        std::fs::write(env.home().join(".claude"), "a file\n").unwrap();
        let out = env.run(["--json", "skill", "install"]);
        unusable(&out, "not_a_directory");
        snapshot("cli_skill__error_json_envelope", &out, "stderr");
    }

    #[test]
    fn error_text() {
        let env = Env::new();
        std::fs::write(env.home().join(".claude"), "a file\n").unwrap();
        let out = env.run(["skill", "install"]);
        assert_eq!(out.code, Some(3), "{}", out.stderr);
        assert_eq!(out.stdout, "");
        let lines: Vec<&str> = out.stderr.lines().collect();
        assert_eq!(lines.len(), 2, "{:?}", out.stderr);
        assert!(lines[0].starts_with("error: "), "{}", lines[0]);
        assert!(lines[0].contains("/TMP/home/.claude"), "{}", lines[0]);
        assert!(lines[1].starts_with("hint: "), "{}", lines[1]);
    }

    // ---- criterion: identity and config ------------------------------------------------------

    #[test]
    fn agent_checked() {
        let env = Env::new();
        let before = env.tree();
        for args in [
            &["skill", "install"][..],
            &["skill", "install", "--print"][..],
            &["--dry-run", "skill", "install"][..],
        ] {
            let out = env.run_with(&[], &[("HALI_AGENT", OsStr::new("bad\u{1}name"))], args);
            assert_eq!(out.code, Some(2), "{args:?}: {}", out.stderr);
            assert_eq!(out.stdout, "", "{args:?}");
            assert!(!out.stderr.is_empty(), "{args:?}");
        }
        let out = env.run(["--agent", "", "skill", "install"]);
        assert_eq!(out.code, Some(2), "{}", out.stderr);
        let out = env.run(["--agent", "bad\u{1}name", "skill", "install"]);
        assert_eq!(out.code, Some(2), "{}", out.stderr);
        assert_eq!(env.tree(), before, "a usage error wrote");
        // A valid name is accepted and records nothing.
        let out = env.run_with(
            &[],
            &[("HALI_AGENT", OsStr::new("claude"))],
            ["skill", "install"],
        );
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert!(!env.data().exists(), "an agent was minted");
    }

    #[test]
    fn config_not_read() {
        let env = Env::new();
        let cfg = env.root.join("cfg/hali");
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(cfg.join("config.toml"), "= = =\n").unwrap();
        // A broken store too: install reads neither.
        std::fs::create_dir_all(env.data()).unwrap();
        std::fs::write(env.data().join("vocabulary.ttl"), "this is not turtle\n").unwrap();
        let v = ok(&env.run(["skill", "install", "--json"]));
        expect_files(&v, &[(CLAUDE_SKILL, "created"), (CLAUDE_USAGE, "created")]);
        assert_eq!(
            std::fs::read_to_string(cfg.join("config.toml")).unwrap(),
            "= = =\n"
        );
        assert_eq!(
            std::fs::read_dir(env.data()).unwrap().count(),
            1,
            "install touched the data dir (a lock file?)"
        );
    }

    // ---- criterion: describe -----------------------------------------------------------------

    const FULL_KEYS: [&str; 8] = [
        "schema_version",
        "core_version",
        "store",
        "counts",
        "concepts",
        "types",
        "relation_kinds",
        "skill",
    ];
    const BRIEF_KEYS: [&str; 5] = ["schema_version", "core_version", "store", "counts", "skill"];

    fn keys(v: &Value) -> Vec<&str> {
        v.as_object().unwrap().keys().map(String::as_str).collect()
    }

    /// `describe` in both forms and both outputs, against the expected `skill` key.
    fn assert_describe(env: &Env, remove: &[&str], installed: Option<&str>, stale: bool) {
        let skill = json!({"installed": installed, "binary": VERSION, "stale": stale});
        for (args, expected_keys) in [
            (&["describe", "--json"][..], &FULL_KEYS[..]),
            (&["describe", "--brief", "--json"][..], &BRIEF_KEYS[..]),
        ] {
            let v = ok(&env.run_with(remove, &[], args));
            assert_eq!(keys(&v), expected_keys, "{args:?}");
            assert_eq!(v["skill"], skill, "{args:?}: {v}");
            assert_eq!(v["schema_version"], 1);
        }
        let shown = installed.unwrap_or("-");
        let brief = format!(
            "core_version\t0.1.0\nstore\t/TMP/data\nresources\t0\nconcepts\t0\ntypes\t0\n\
             relation_kinds\t4\nrelations\t0\nskill_installed\t{shown}\nskill_binary\t{VERSION}\n\
             skill_stale\t{stale}\n"
        );
        let out = env.run_with(remove, &[], ["describe", "--brief"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        assert_eq!(out.stdout, brief);
        let out = env.run_with(remove, &[], ["describe"]);
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        let rest = out.stdout.strip_prefix(&brief).unwrap_or_else(|| {
            panic!(
                "the full text does not start with the brief text: {:?}",
                out.stdout
            )
        });
        assert!(
            rest.lines().all(|l| l.starts_with("relation_kind\t")),
            "the lists follow the skill lines: {rest:?}"
        );
    }

    #[test]
    fn describe_skill_absent() {
        let env = Env::new();
        let before = env.tree();
        assert_describe(&env, &[], None, false);
        assert_eq!(env.tree(), before, "describe wrote");
        assert!(!env.home().join(".claude").exists());
    }

    #[test]
    fn describe_skill_current() {
        let env = Env::new();
        ok(&env.run(["skill", "install", "--json"]));
        let before = env.tree();
        assert_describe(&env, &[], Some(VERSION), false);
        assert_eq!(env.tree(), before, "describe wrote");
    }

    #[test]
    fn describe_skill_stale() {
        let env = Env::new();
        mkdir(&env.claude(), 0o700);
        std::fs::write(
            env.claude().join("SKILL.md"),
            "---\nname: hali\ndescription: old\nmetadata:\n  hali-version: \"9.9.9\"\n---\nold\n",
        )
        .unwrap();
        assert_describe(&env, &[], Some("9.9.9"), true);
        // A file with no readable version: installed null, stale true.
        std::fs::write(env.claude().join("SKILL.md"), "no frontmatter\n").unwrap();
        let v = ok(&env.run(["describe", "--brief", "--json"]));
        assert_eq!(
            v["skill"],
            json!({"installed": null, "binary": VERSION, "stale": true})
        );
        // Only the claude location is reported: an agents-only install is "not installed".
        let env = Env::new();
        mkdir(&env.agents(), 0o700);
        std::fs::write(
            env.agents().join("SKILL.md"),
            "---\nmetadata:\n  hali-version: \"9.9.9\"\n---\n",
        )
        .unwrap();
        assert_describe(&env, &[], None, false);
    }

    #[test]
    fn describe_skill_unresolved() {
        // No HOME, but a data dir: describe works, and the skill is "not installed".
        let env = Env::new();
        assert_describe(&env, &["HOME"], None, false);
    }

    #[test]
    fn describe_skill_text() {
        // The three lines sit straight after `relations`; `-` stands for no installed version.
        let env = Env::new();
        let out = env.run(["describe", "--brief"]);
        assert!(
            out.stdout.ends_with(&format!(
                "relations\t0\nskill_installed\t-\nskill_binary\t{VERSION}\nskill_stale\tfalse\n"
            )),
            "{:?}",
            out.stdout
        );
    }

    #[test]
    fn describe_skill_is_not_read_through_a_symlink() {
        let env = Env::new();
        mkdir(&env.claude(), 0o700);
        let target = env.root.join("elsewhere.md");
        std::fs::write(
            &target,
            "---\nname: hali\nmetadata:\n  hali-version: \"1.0.0\"\n---\n",
        )
        .unwrap();
        symlink(&target, env.claude().join("SKILL.md")).unwrap();
        let v = ok(&env.run(["describe", "--brief", "--json"]));
        assert_eq!(v["skill"]["installed"], Value::Null, "{v}");
        assert_eq!(v["skill"]["binary"], VERSION);
        assert_eq!(
            v["skill"]["stale"], true,
            "no readable version is stale: {v}"
        );
    }

    #[test]
    fn describe_skill_is_not_read_through_a_symlinked_hali_dir() {
        let env = Env::new();
        mkdir(&env.home().join(".claude/skills"), 0o700);
        let elsewhere = env.root.join("elsewhere");
        mkdir(&elsewhere, 0o700);
        std::fs::write(
            elsewhere.join("SKILL.md"),
            "---\nname: hali\nmetadata:\n  hali-version: \"1.0.0\"\n---\n",
        )
        .unwrap();
        symlink(&elsewhere, env.claude()).unwrap();
        let v = ok(&env.run(["describe", "--brief", "--json"]));
        assert_eq!(v["skill"]["installed"], Value::Null, "{v}");
        assert_eq!(v["skill"]["stale"], true, "{v}");
    }

    #[test]
    fn describe_skill_fifo_does_not_block() {
        let env = Env::new();
        mkdir(&env.claude(), 0o700);
        mkfifo(&env.claude().join("SKILL.md"));
        // `run` has a 30 second timeout: a blocking open would fail the test.
        let v = ok(&env.run(["describe", "--brief", "--json"]));
        assert_eq!(v["skill"]["installed"], Value::Null, "{v}");
        assert_eq!(v["skill"]["stale"], true, "{v}");
    }

    // ---- criterion: schema and help ----------------------------------------------------------

    #[test]
    fn schema_skill_install() {
        let env = Env::new();
        let text = env.run(["schema", "skill", "install"]);
        let json_out = env.run(["schema", "--json", "skill", "install"]);
        for out in [&text, &json_out] {
            assert_eq!(out.code, Some(0), "{}", out.stderr);
            assert_eq!(out.stderr, "");
        }
        assert_eq!(text.stdout, json_out.stdout);
        let doc = json(&text.stdout);
        assert_eq!(
            doc["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
        assert!(doc.get("$id").is_none());
        let listed = ok(&env.run(["schema", "--json"]));
        assert!(
            listed["schemas"]
                .as_array()
                .unwrap()
                .iter()
                .any(|n| n == "skill install"),
            "{listed}"
        );
    }

    #[test]
    fn skill_help_ok() {
        // scripts/install.sh probes `hali skill --help`.
        let env = Env::new();
        for args in [
            &["skill", "--help"][..],
            &["skill", "-h"][..],
            &["skill", "install", "--help"][..],
            &["help", "skill"][..],
        ] {
            let out = env.run(args);
            assert_eq!(out.code, Some(0), "{args:?}: {}", out.stderr);
            assert!(out.stdout.starts_with("Examples:\n  hali "), "{args:?}");
            assert!(out.stdout.contains("skill"), "{args:?}");
        }
        let root = env.run(["--help"]);
        assert!(
            root.stdout
                .lines()
                .any(|l| l.trim_start().starts_with("skill ")),
            "`hali --help` does not list skill"
        );
        let install = env.run(["skill", "install", "--help"]);
        for flag in ["--agents", "--print"] {
            assert!(install.stdout.contains(flag), "install --help lacks {flag}");
        }
    }

    // ---- review round 1: ownership and modes of every directory written into ---------------

    fn skip_root(what: &str) -> bool {
        if is_root() {
            eprintln!("skipping {what}: running as root, permission bits do not stop root");
            return true;
        }
        false
    }

    /// Every directory the install writes into must be owned by the euid and not group- or
    /// world-writable: `~/.claude`, `skills`, `hali`, `references` (HOME's mode is not checked).
    #[test]
    fn error_unsafe_every_written_directory() {
        // `~/.claude/skills` at 0777.
        let env = Env::new();
        mkdir(&env.home().join(".claude"), 0o700);
        mkdir(&env.home().join(".claude/skills"), 0o777);
        assert_unusable(&env, "unsafe", Some("/TMP/home/.claude/skills"), &[]);

        // `~/.claude` at 0775.
        let env = Env::new();
        mkdir(&env.home().join(".claude"), 0o775);
        assert_unusable(&env, "unsafe", Some("/TMP/home/.claude"), &[]);
        assert!(!env.home().join(".claude/skills").exists());
    }

    #[test]
    fn error_unsafe_agents_location_writes_nothing_anywhere() {
        let env = Env::new();
        mkdir(&env.home().join(".claude"), 0o700);
        mkdir(&env.home().join(".agents"), 0o700);
        mkdir(&env.home().join(".agents/skills"), 0o777);
        assert_unusable(
            &env,
            "unsafe",
            Some("/TMP/home/.agents/skills"),
            &["--agents"],
        );
        assert!(
            !env.home().join(".claude/skills").exists(),
            "the claude location was written before the agents location was checked"
        );
    }

    #[test]
    fn error_unsafe_home_not_owned() {
        if skip_root("error_unsafe_home_not_owned") {
            return;
        }
        if Path::new("/.claude").exists() {
            eprintln!("skipping error_unsafe_home_not_owned: /.claude exists on this machine");
            return;
        }
        // `/` is owned by root, not the euid. A dry run only looks, so nothing can be written.
        let env = Env::new();
        let out = env.run_with(
            &[],
            &[("HOME", OsStr::new("/"))],
            ["--dry-run", "--json", "skill", "install"],
        );
        let e = unusable(&out, "unsafe");
        assert_eq!(e["details"]["path"], "/", "{}", out.stderr);
    }

    /// A guard that lasts: ordinary 0755 directories owned by the user are accepted, and HOME's
    /// own mode is not checked.
    #[test]
    fn unsafe_check_accepts_user_owned_0755() {
        let env = Env::new();
        for d in [
            ".claude",
            ".claude/skills",
            ".claude/skills/hali",
            ".claude/skills/hali/references",
        ] {
            mkdir(&env.home().join(d), 0o755);
        }
        std::fs::set_permissions(env.home(), std::fs::Permissions::from_mode(0o777)).unwrap();
        let v = ok(&env.run(["skill", "install", "--json"]));
        expect_files(&v, &[(CLAUDE_SKILL, "created"), (CLAUDE_USAGE, "created")]);
    }

    // ---- review round 1: a file with the right bytes but the wrong mode ---------------------

    #[test]
    fn right_bytes_wrong_mode_is_updated() {
        let env = Env::new();
        ok(&env.run(["skill", "install", "--json"]));
        let files = [
            env.claude().join("SKILL.md"),
            env.claude().join("references/usage.md"),
        ];
        for f in &files {
            std::fs::set_permissions(f, std::fs::Permissions::from_mode(0o666)).unwrap();
        }
        let v = ok(&env.run(["skill", "install", "--json"]));
        expect_files(&v, &[(CLAUDE_SKILL, "updated"), (CLAUDE_USAGE, "updated")]);
        for f in &files {
            assert_eq!(mode(f), 0o600, "{}", f.display());
        }
        let v = ok(&env.run(["skill", "install", "--json"]));
        expect_files(
            &v,
            &[(CLAUDE_SKILL, "unchanged"), (CLAUDE_USAGE, "unchanged")],
        );
    }

    // ---- review round 1: both locations are the same directory ------------------------------

    fn assert_aliased(env: &Env) {
        let v = ok(&env.run(["skill", "install", "--agents", "--json"]));
        expect_files(
            &v,
            &[
                (CLAUDE_SKILL, "created"),
                (CLAUDE_USAGE, "created"),
                (AGENTS_SKILL, "unchanged"),
                (AGENTS_USAGE, "unchanged"),
            ],
        );
        for loc in [env.claude(), env.agents()] {
            assert_eq!(
                std::fs::read_to_string(loc.join("SKILL.md")).unwrap(),
                expected_skill()
            );
            assert_eq!(
                std::fs::read(loc.join("references/usage.md")).unwrap(),
                guide()
            );
        }
    }

    #[test]
    fn agents_symlinked_to_claude() {
        let env = Env::new();
        mkdir(&env.home().join(".claude"), 0o700);
        symlink(env.home().join(".claude"), env.home().join(".agents")).unwrap();
        assert_aliased(&env);
    }

    #[test]
    fn agents_skills_symlinked_to_claude_skills() {
        let env = Env::new();
        mkdir(&env.home().join(".claude/skills"), 0o700);
        mkdir(&env.home().join(".agents"), 0o700);
        symlink(
            env.home().join(".claude/skills"),
            env.home().join(".agents/skills"),
        )
        .unwrap();
        assert_aliased(&env);
    }

    // ---- review round 1: what the error message says about writes ---------------------------

    #[test]
    fn cannot_create_found_before_writing_says_nothing_was_written() {
        if skip_root("cannot_create_found_before_writing_says_nothing_was_written") {
            return;
        }
        let env = Env::new();
        let claude = env.home().join(".claude");
        mkdir(&claude, 0o700);
        let _restore = Restore(claude.clone());
        std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o000)).unwrap();
        for args in [
            &["--dry-run", "--json", "skill", "install"][..],
            &["--json", "skill", "install"][..],
        ] {
            let out = env.run(args);
            let e = unusable(&out, "cannot_create");
            let message = e["message"].as_str().unwrap();
            assert!(
                message.ends_with("nothing was written"),
                "{args:?}: message {message:?} does not end with \"nothing was written\""
            );
        }
    }

    #[test]
    fn cannot_create_after_a_write_began_does_not_say_nothing_was_written() {
        if skip_root("cannot_create_after_a_write_began") {
            return;
        }
        let env = Env::new();
        let agents = env.home().join(".agents");
        mkdir(&agents, 0o700);
        let _restore = Restore(agents.clone());
        std::fs::set_permissions(&agents, std::fs::Permissions::from_mode(0o500)).unwrap();
        let out = env.run(["--json", "skill", "install", "--agents"]);
        let e = unusable(&out, "cannot_create");
        let message = e["message"].as_str().unwrap();
        assert!(
            !message.contains("nothing was written"),
            "the claude files were written, yet the message says: {message:?}"
        );
        assert_eq!(
            std::fs::read_to_string(env.claude().join("SKILL.md")).unwrap(),
            expected_skill(),
            "the claude location is written before the agents location fails"
        );
        assert_eq!(
            std::fs::read(env.claude().join("references/usage.md")).unwrap(),
            guide()
        );
    }
}
