//! Shared helper of the #27 goal tests (agent usage guide, `hali help agents`, `--help`
//! examples): the example world, the example-line runner, the clap-tree walker and the guide
//! reader. Goal-owned and locked: every example in `--help` and every `sh` line of
//! `docs/agent-usage.md` must fit this world, not the other way round.
//!
//! # The example world
//!
//! Built once per `World::new()` through the real binary (`CARGO_BIN_EXE_hali`), under one temp
//! dir `root`. Every run clears the environment, then sets HOME=`root/home`, XDG_CONFIG_HOME,
//! XDG_DATA_HOME, XDG_STATE_HOME, HALI_DATA_DIR=`root/data`, GIT_CONFIG_GLOBAL and
//! GIT_CONFIG_SYSTEM (temp paths), HALI_AGENT only when the example line sets it; the cwd is
//! HOME and stdin is null. There is no config file, so `files.dir` is the default
//! `~/Documents/hali`. The clock and the ids are real (no HALI_TEST_* variable).
//!
//! Files (below HOME; `~/` below means HOME):
//! - `~/papers/ldp.pdf` (a PDF header line), `~/notes/ideas.md` (`# Ideas`) and
//!   `~/notes/todo.md` (`# Todo`);
//! - `~/archive/old.pdf`: the file of a Resource that was cataloged as `~/notes/old.pdf` and
//!   then moved, so the Resource is Missing and `relink` and `scan --prune` have work.
//!
//! Store, in the order it is built:
//! - Concepts `computing` and `ml` (`ml` is narrower than `computing`);
//! - Types `Paper` and `Note`;
//! - the relation kind `annotates`, a sub-property of `dcterms:relation`;
//! - the unused Concept `drafts` and the unused relation kind `supersedes` (a sub-property of
//!   `dcterms:relation`), so `concept remove` and `relation remove` examples can run;
//! - `~/papers/ldp.pdf` cataloged by `hali add --type Paper --tag ml`;
//! - `hali scan ~/notes`: registers `ideas.md`, `todo.md` and `old.pdf` (untyped, untagged) and
//!   remembers `~/notes` as a Scan root;
//! - `~/notes/ideas.md` `annotates` `~/papers/ldp.pdf`;
//! - `~/notes/old.pdf` moved to `~/archive/old.pdf` (now Missing).
//!
//! So: 4 Resources, 1 Missing, 0 Stale, 1 remembered Scan root, no Agent minted. Examples refer
//! to Resources by path (`~/papers/ldp.pdf`), never by UUID.
//!
//! # The example-line runner
//!
//! One line is `[HALI_AGENT=<name>] hali <args…> [# comment]`. A small quote-aware word
//! splitter, no shell: words split on whitespace; single quotes and double quotes group (no
//! escapes inside, no nesting); an unquoted `#` at the start of a word begins a comment that
//! runs to the end of the line; an unquoted word starting with `~/` has that `~` replaced by
//! HOME. Anything else (pipes, `&&`, `$VAR`, redirects, another leading variable) is just text
//! and is passed on as arguments. In a guide `sh` block the comment `# exit N` is the expected
//! exit code (default 0).
//!
//! # What counts as an example block
//!
//! A command's `before_help` (and the first lines of its `--help` and `-h`) is the text
//! `Examples:\n` followed by the example lines, each `  hali …` (two spaces, then the line as
//! above), with nothing else in the block; a blank line or the end of the text ends it.

#![allow(dead_code)]

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const BIN: &str = env!("CARGO_BIN_EXE_hali");

/// The guide, relative to the manifest dir.
pub const GUIDE: &str = "docs/agent-usage.md";
pub const SNIPPET_START: &str = "<!-- hali:snippet:start -->";
pub const SNIPPET_END: &str = "<!-- hali:snippet:end -->";

pub fn guide_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(GUIDE)
}

/// The guide's bytes; a failure names the missing file.
pub fn guide_bytes() -> Vec<u8> {
    std::fs::read(guide_path()).unwrap_or_else(|e| panic!("{GUIDE} cannot be read: {e}"))
}

pub fn guide_text() -> String {
    String::from_utf8(guide_bytes()).unwrap_or_else(|e| panic!("{GUIDE} is not UTF-8: {e}"))
}

/// The marked section of `text`, found independently of the code under test: the lines strictly
/// between the line equal to the start marker and the line equal to the end marker, joined by
/// LF, with no LF after the last one. `None` when a marker line is missing or out of order.
pub fn snippet_section(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.split('\n').collect();
    let start = lines.iter().position(|l| *l == SNIPPET_START)?;
    let end = lines.iter().position(|l| *l == SNIPPET_END)?;
    if end <= start {
        return None;
    }
    Some(lines[start + 1..end].join("\n"))
}

/// Whether spec §9.3 escapes `c` in text output (written out here, independent of the code).
pub fn is_escaped(c: char) -> bool {
    let u = u32::from(c);
    c.is_control()
        || matches!(u,
            0x2028 | 0x2029
            | 0x061C | 0x200E | 0x200F
            | 0x202A..=0x202E | 0x2066..=0x2069
            | 0x00AD | 0x180E | 0x200B
            | 0x2060..=0x2064 | 0x206A..=0x206F
            | 0xFEFF | 0xFFF9..=0xFFFB
            | 0xE0000..=0xE007F)
}

pub struct Out {
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl Out {
    pub fn out(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }
    pub fn err(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }
}

fn finish(mut cmd: Command) -> Out {
    let out = cmd.output().expect("run the hali binary");
    Out {
        code: out.status.code(),
        stdout: out.stdout,
        stderr: out.stderr,
    }
}

// ----- the word splitter -----------------------------------------------------------------

/// One parsed example line.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// The value of a leading `HALI_AGENT=<name>`.
    pub agent: Option<String>,
    /// The words after `hali`.
    pub args: Vec<String>,
    /// The trailing `# …` comment, without the `#` and trimmed.
    pub comment: Option<String>,
}

/// Splits `line` per the module header; `home` replaces a leading `~` of an unquoted `~/` word.
pub fn parse_line(line: &str, home: &str) -> Result<Line, String> {
    // (text, began outside quotes)
    let mut words: Vec<(String, bool)> = Vec::new();
    let mut comment = None;
    let mut cur = String::new();
    let mut cur_unquoted = true;
    let mut in_word = false;
    let mut quote: Option<char> = None;
    for (i, c) in line.char_indices() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            } else {
                cur.push(c);
            }
        } else if c.is_whitespace() {
            if in_word {
                words.push((std::mem::take(&mut cur), cur_unquoted));
                in_word = false;
            }
        } else if c == '#' && !in_word {
            comment = Some(line[i + 1..].trim().to_string());
            break;
        } else if c == '\'' || c == '"' {
            if !in_word {
                in_word = true;
                cur_unquoted = false;
            }
            quote = Some(c);
        } else {
            if !in_word {
                in_word = true;
                cur_unquoted = true;
            }
            cur.push(c);
        }
    }
    if quote.is_some() {
        return Err(format!("unterminated quote in {line:?}"));
    }
    if in_word {
        words.push((cur, cur_unquoted));
    }
    let mut iter = words.into_iter().map(|(w, unquoted)| {
        if unquoted && w.starts_with("~/") {
            format!("{home}{}", &w[1..])
        } else {
            w
        }
    });
    let mut first = iter.next().ok_or_else(|| format!("empty line {line:?}"))?;
    let mut agent = None;
    if let Some(name) = first.strip_prefix("HALI_AGENT=") {
        agent = Some(name.to_string());
        first = iter
            .next()
            .ok_or_else(|| format!("nothing after HALI_AGENT= in {line:?}"))?;
    }
    if first != "hali" {
        return Err(format!("{line:?} does not start with `hali`"));
    }
    Ok(Line {
        agent,
        args: iter.collect(),
        comment,
    })
}

/// The expected exit code a comment carries: `exit N`.
pub fn expected_exit(comment: Option<&str>) -> Result<i32, String> {
    match comment {
        None => Ok(0),
        Some(c) => match c.strip_prefix("exit ") {
            Some(n) => n
                .trim()
                .parse::<i32>()
                .map_err(|_| format!("bad exit annotation {c:?}")),
            None => Ok(0),
        },
    }
}

// ----- example blocks ----------------------------------------------------------------------

/// The example lines (two-space indent removed) of a text starting with `Examples:\n`, and the
/// rest of the text after the block. `Err` when the text does not start with the heading.
pub fn example_block(text: &str) -> Result<(Vec<String>, String), String> {
    let Some(body) = text.strip_prefix("Examples:\n") else {
        let head: String = text.chars().take(60).collect();
        return Err(format!(
            "does not start with `Examples:` and a line feed: {head:?}"
        ));
    };
    let mut examples = Vec::new();
    let mut consumed = 0;
    for line in body.split_inclusive('\n') {
        let bare = line.trim_end_matches('\n');
        if bare.starts_with("  hali ") {
            examples.push(bare[2..].to_string());
            consumed += line.len();
        } else {
            break;
        }
    }
    Ok((examples, body[consumed..].to_string()))
}

// ----- the clap tree ---------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Root,
    Group,
    Mutating,
    ReadOnly,
}

pub struct Node {
    /// The command words below `hali` (empty for the root).
    pub path: Vec<String>,
    pub cmd: clap::Command,
    pub kind: Kind,
}

impl Node {
    pub fn name(&self) -> String {
        if self.path.is_empty() {
            "hali".to_string()
        } else {
            format!("hali {}", self.path.join(" "))
        }
    }

    /// The text of the command's `before_help` (empty when it has none).
    pub fn before_help(&self) -> String {
        self.cmd
            .get_before_help()
            .map(|s| s.to_string())
            .unwrap_or_default()
    }

    /// The example lines of `before_help`; `Err` when it is not an examples block.
    pub fn examples(&self) -> Result<Vec<String>, String> {
        let text = self.before_help();
        let (examples, rest) = example_block(&text)?;
        if !rest.trim().is_empty() {
            return Err(format!("text after the examples block: {rest:?}"));
        }
        Ok(examples)
    }
}

const MUTATING: [&str; 17] = [
    "add",
    "new",
    "edit",
    "forget",
    "refresh",
    "scan",
    "relate",
    "relink",
    "unrelate",
    "concept add",
    "concept rename",
    "concept edit",
    "concept remove",
    "type add",
    "type remove",
    "relation add",
    "relation remove",
];
const READ_ONLY: [&str; 10] = [
    "describe",
    "query",
    "schema",
    "find",
    "show",
    "validate",
    "concept show",
    "concept list",
    "type list",
    "relation list",
];

/// Every visible command of the clap tree, the root first, then depth first in definition
/// order. Hidden commands (`generate`) and every command named `help` (`help`, `help agents`)
/// are left out: they are exempt from the examples rules. A visible command that is in neither
/// list nor a group makes the walk panic, so a new command must be classified here.
pub fn commands() -> Vec<Node> {
    fn walk(cmd: &clap::Command, path: Vec<String>, out: &mut Vec<Node>) {
        let subs: Vec<&clap::Command> = cmd
            .get_subcommands()
            .filter(|c| !c.is_hide_set() && c.get_name() != "help")
            .collect();
        let joined = path.join(" ");
        let kind = if path.is_empty() {
            Kind::Root
        } else if !subs.is_empty() {
            Kind::Group
        } else if MUTATING.contains(&joined.as_str()) {
            Kind::Mutating
        } else if READ_ONLY.contains(&joined.as_str()) {
            Kind::ReadOnly
        } else {
            panic!(
                "tests/example_world: command `hali {joined}` is not classified (MUTATING or READ_ONLY)"
            );
        };
        out.push(Node {
            path: path.clone(),
            cmd: cmd.clone(),
            kind,
        });
        for sub in subs {
            let mut p = path.clone();
            p.push(sub.get_name().to_string());
            walk(sub, p, out);
        }
    }
    let root = hali::cli::command();
    let mut out = Vec::new();
    walk(&root, Vec::new(), &mut out);
    out
}

/// Parses `args` (the words after `hali`) with the clap tree. `Ok(Some(path))` is the chain of
/// subcommand names matched; `Ok(None)` is a help request (`DisplayHelp`), which counts as
/// success; `Err` is any other clap error.
pub fn parse_with_clap(args: &[String]) -> Result<Option<Vec<String>>, String> {
    let argv = std::iter::once("hali".to_string()).chain(args.iter().cloned());
    match hali::cli::command().try_get_matches_from(argv) {
        Ok(matches) => {
            let mut path = Vec::new();
            let mut m = &matches;
            while let Some((name, sub)) = m.subcommand() {
                path.push(name.to_string());
                m = sub;
            }
            Ok(Some(path))
        }
        Err(e) if e.kind() == clap::error::ErrorKind::DisplayHelp => Ok(None),
        Err(e) => Err(format!(
            "{:?}: {}",
            e.kind(),
            e.to_string().lines().next().unwrap_or("")
        )),
    }
}

// ----- the world ---------------------------------------------------------------------------

pub struct World {
    _tmp: tempfile::TempDir,
    pub root: PathBuf,
    snapshot: PathBuf,
}

impl World {
    pub fn home(&self) -> PathBuf {
        self.root.join("home")
    }
    pub fn data(&self) -> PathBuf {
        self.root.join("data")
    }
    pub fn cfg(&self) -> PathBuf {
        self.root.join("cfg")
    }

    /// Scratch dirs only: no files, an empty store, no setup run.
    pub fn empty() -> World {
        let tmp = tempfile::tempdir().expect("temp dir");
        let base = std::fs::canonicalize(tmp.path()).expect("canonical temp dir");
        for dir in base.ancestors().skip(1) {
            for name in [".gitignore", ".ignore", ".haliignore", ".git", ".jj"] {
                assert!(
                    !dir.join(name).exists(),
                    "{} exists: the host's ignore rules would reach the scan root; point TMPDIR elsewhere",
                    dir.join(name).display()
                );
            }
        }
        let root = base.join("world");
        let snapshot = base.join("snapshot");
        for d in ["home", "cfg", "share", "state"] {
            std::fs::create_dir_all(root.join(d)).expect("scratch dir");
        }
        std::fs::create_dir_all(&snapshot).expect("snapshot dir");
        World {
            _tmp: tmp,
            root,
            snapshot,
        }
    }

    /// A command for `program` with the cleared, scratch environment and cwd HOME.
    pub fn command(&self, program: impl AsRef<OsStr>) -> Command {
        let mut cmd = Command::new(program);
        cmd.env_clear()
            .env("HOME", self.home())
            .env("XDG_CONFIG_HOME", self.cfg())
            .env("XDG_DATA_HOME", self.root.join("share"))
            .env("XDG_STATE_HOME", self.root.join("state"))
            .env("HALI_DATA_DIR", self.data())
            .env("GIT_CONFIG_GLOBAL", self.root.join("gitconfig"))
            .env("GIT_CONFIG_SYSTEM", self.root.join("gitconfig-system"))
            .current_dir(self.home())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }

    pub fn run<S: AsRef<OsStr>>(&self, args: &[S]) -> Out {
        let mut cmd = self.command(BIN);
        cmd.args(args);
        finish(cmd)
    }

    fn setup(&self, args: &[&str]) {
        let out = self.run(args);
        assert_eq!(
            out.code,
            Some(0),
            "world setup `hali {}` failed: {}",
            args.join(" "),
            out.err()
        );
    }

    fn write(&self, rel: &str, text: &str) {
        let path = self.home().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// The example world of the module header.
    pub fn new() -> World {
        let w = World::empty();
        w.write("papers/ldp.pdf", "%PDF-1.4\nLinked data platform\n");
        w.write("notes/ideas.md", "# Ideas\n");
        w.write("notes/todo.md", "# Todo\n");
        w.write("notes/old.pdf", "%PDF-1.4\nold paper\n");
        w.setup(&["concept", "add", "computing"]);
        w.setup(&["concept", "add", "ml", "--broader", "computing"]);
        w.setup(&["type", "add", "Paper"]);
        w.setup(&["type", "add", "Note"]);
        w.setup(&[
            "relation",
            "add",
            "annotates",
            "--parent",
            "dcterms:relation",
        ]);
        // Unused, so `concept remove` and `relation remove` examples have something to remove.
        w.setup(&["concept", "add", "drafts"]);
        w.setup(&[
            "relation",
            "add",
            "supersedes",
            "--parent",
            "dcterms:relation",
        ]);
        let ldp = w.home().join("papers/ldp.pdf");
        let ldp = ldp.to_str().unwrap();
        w.setup(&["add", ldp, "--type", "Paper", "--tag", "ml"]);
        let notes = w.home().join("notes");
        w.setup(&["scan", notes.to_str().unwrap()]);
        let ideas = w.home().join("notes/ideas.md");
        w.setup(&["relate", ideas.to_str().unwrap(), "annotates", ldp]);
        std::fs::create_dir_all(w.home().join("archive")).unwrap();
        std::fs::rename(
            w.home().join("notes/old.pdf"),
            w.home().join("archive/old.pdf"),
        )
        .unwrap();
        w.copy(&w.root, &w.snapshot);
        w
    }

    fn copy(&self, from: &Path, to: &Path) {
        let status = Command::new("cp")
            .arg("-a")
            .arg(format!("{}/.", from.display()))
            .arg(to)
            .status()
            .expect("run cp");
        assert!(
            status.success(),
            "cp -a {} {}",
            from.display(),
            to.display()
        );
    }

    /// Puts the world back as `new()` left it (same paths, so the store's absolute paths hold).
    pub fn reset(&self) {
        for entry in std::fs::read_dir(&self.root).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                std::fs::remove_dir_all(&path).unwrap();
            } else {
                std::fs::remove_file(&path).unwrap();
            }
        }
        self.copy(&self.snapshot, &self.root);
    }

    /// Runs one example line in the world (the runner of the module header). `Err` when the line
    /// cannot be split or does not start with `hali`; otherwise the parsed line and the result.
    pub fn run_line(&self, line: &str) -> Result<(Line, Out), String> {
        let home = self.home();
        let parsed = parse_line(line, home.to_str().unwrap())?;
        let mut cmd = self.command(BIN);
        cmd.args(&parsed.args);
        if let Some(agent) = &parsed.agent {
            cmd.env("HALI_AGENT", agent);
        }
        Ok((parsed, finish(cmd)))
    }

    /// Every path under the root with its size (directories 0), sorted: to prove nothing was
    /// created, changed or removed.
    pub fn tree(&self) -> Vec<String> {
        fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
            for e in std::fs::read_dir(dir).unwrap() {
                let p = e.unwrap().path();
                let meta = std::fs::symlink_metadata(&p).unwrap();
                let rel = p.strip_prefix(root).unwrap().display().to_string();
                out.push(format!(
                    "{rel} {}",
                    if meta.is_dir() { 0 } else { meta.len() }
                ));
                if meta.is_dir() {
                    walk(&p, root, out);
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.root, &self.root, &mut out);
        out.sort();
        out
    }
}
