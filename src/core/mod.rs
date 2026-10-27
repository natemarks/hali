//! Transport-agnostic operations. `core` works on an in-memory store and never touches clap,
//! stdout, the filesystem or the process environment; time and ids are injected.

pub mod concept;
pub mod config;
pub mod schema;
pub mod validate;

use crate::storage;
use oxigraph::model::{Quad, TripleRef};
use serde::Serialize;
use serde_json::{Value, json};
use std::cell::Cell;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// The current time, as RFC 3339 UTC (`YYYY-MM-DDTHH:MM:SSZ`).
pub trait Clock {
    fn now(&self) -> String;
}

/// Mints identifiers: hyphenated, lowercase UUIDv7s.
pub trait IdGen {
    fn next_id(&self) -> String;
}

/// A clock that always returns the same instant.
pub struct FixedClock(String);

impl FixedClock {
    pub fn new(instant: &str) -> FixedClock {
        FixedClock(instant.to_string())
    }
}

impl Clock for FixedClock {
    fn now(&self) -> String {
        self.0.clone()
    }
}

/// The system clock.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> String {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        rfc3339(secs)
    }
}

/// Formats seconds since the Unix epoch as `YYYY-MM-DDTHH:MM:SSZ`.
fn rfc3339(secs: u64) -> String {
    let (days, rem) = (secs / 86_400, secs % 86_400);
    // Civil date from days since 1970-01-01 (H. Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem / 60 % 60,
        rem % 60
    )
}

/// Random UUIDv7s from the system clock and RNG.
pub struct RandomIdGen;

impl IdGen for RandomIdGen {
    fn next_id(&self) -> String {
        uuid::Uuid::now_v7().hyphenated().to_string()
    }
}

/// Deterministic UUIDv7s: the same seed gives the same sequence.
pub struct SeededIdGen {
    state: Cell<u64>,
    count: Cell<u64>,
}

impl SeededIdGen {
    pub fn new(seed: u64) -> SeededIdGen {
        SeededIdGen {
            state: Cell::new(seed),
            count: Cell::new(0),
        }
    }

    /// splitmix64.
    fn next_u64(&self) -> u64 {
        let state = self.state.get().wrapping_add(0x9e37_79b9_7f4a_7c15);
        self.state.set(state);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
}

impl IdGen for SeededIdGen {
    fn next_id(&self) -> String {
        // Fixed epoch plus a counter keeps the ids ordered, as real UUIDv7s are.
        let count = self.count.get() + 1;
        self.count.set(count);
        let mut random = [0u8; 10];
        random[..8].copy_from_slice(&self.next_u64().to_be_bytes());
        random[8..].copy_from_slice(&self.next_u64().to_be_bytes()[..2]);
        uuid::Builder::from_unix_timestamp_millis(1_700_000_000_000 + count, &random)
            .into_uuid()
            .hyphenated()
            .to_string()
    }
}

/// What every operation runs with: the configured language and the injected clock and ids.
pub struct Context {
    language: String,
    clock: Box<dyn Clock>,
    ids: Box<dyn IdGen>,
}

impl Context {
    pub fn new(language: &str, clock: Box<dyn Clock>, ids: Box<dyn IdGen>) -> Context {
        Context {
            language: language.to_ascii_lowercase(),
            clock,
            ids,
        }
    }

    /// The language tag of new labels, lowercase.
    pub fn language(&self) -> &str {
        &self.language
    }

    pub fn now(&self) -> String {
        self.clock.now()
    }

    pub fn next_id(&self) -> String {
        self.ids.next_id()
    }
}

/// Whether a change adds a triple to a tool-managed file or removes one from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Op {
    Add,
    Remove,
}

/// One triple a mutation adds or removes, as canonical N-Triples (spec §9.5). The field order
/// is the sort order.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Change {
    pub op: Op,
    pub triple: String,
}

/// The result of a mutating operation: what it produced, and the triples it changed.
#[derive(Clone, Debug, PartialEq)]
pub struct Mutation<T> {
    pub value: T,
    pub changes: Vec<Change>,
}

/// Inserts the quads the store does not hold yet (in their own graph) and returns them as
/// `Op::Add` changes, sorted and without duplicates.
pub(crate) fn insert_all(store: &storage::Store, quads: &[Quad]) -> Vec<Change> {
    let mut changes = Vec::new();
    for quad in quads {
        if store.contains(quad).expect("reading the in-memory store") {
            continue;
        }
        store
            .insert(quad)
            .expect("inserting into the in-memory store");
        changes.push(Change {
            op: Op::Add,
            triple: format!(
                "{} .",
                TripleRef::new(
                    quad.subject.as_ref(),
                    quad.predicate.as_ref(),
                    quad.object.as_ref()
                )
            ),
        });
    }
    changes.sort();
    changes.dedup();
    changes
}

/// A structured error (spec §9.4). It serializes as the `error` member of the envelope.
#[derive(Debug, Serialize)]
pub struct Error {
    pub rule: &'static str,
    pub message: String,
    pub details: Box<Value>,
    pub suggested_fix: Option<String>,
    pub suggested_command: Option<String>,
    #[serde(skip)]
    exit_code: u8,
}

impl Error {
    /// The process exit code: 1 rule/domain error, 2 usage error, 3 store or environment.
    pub fn exit_code(&self) -> u8 {
        self.exit_code
    }

    fn new(rule: &'static str, exit_code: u8, message: String, details: Value) -> Error {
        Error {
            rule,
            message,
            details: Box::new(details),
            suggested_fix: None,
            suggested_command: None,
            exit_code,
        }
    }

    fn fix(mut self, fix: &str) -> Error {
        self.suggested_fix = Some(fix.to_string());
        self
    }

    /// A bad argument. Usage errors are not rules, so the rule is plain `USAGE`.
    pub fn usage(message: &str) -> Error {
        Error::new("USAGE", 2, message.to_string(), json!({}))
    }

    /// At most this many violation records go into a `SHACL_VIOLATION` error.
    const MAX_VIOLATIONS: usize = 10;

    /// The store (with a mutation's change, if `mutation`) does not conform to the shapes.
    /// `violations` are sorted; the first few are listed.
    pub fn shacl_violation(
        violations: &[validate::Violation],
        total: usize,
        mutation: bool,
    ) -> Error {
        let count = match total {
            1 => "1 SHACL violation".to_string(),
            n => format!("{n} SHACL violations"),
        };
        let message = if mutation {
            format!("the change would leave {count} in the store; nothing was written")
        } else {
            format!("the store has {count}")
        };
        let listed: Vec<&validate::Violation> =
            violations.iter().take(Error::MAX_VIOLATIONS).collect();
        let truncated = listed.len() < total;
        let mut err = Error::new(
            "SHACL_VIOLATION",
            1,
            message,
            json!({"violations": listed, "total": total, "truncated": truncated}),
        )
        .fix("Fix the data or the extensions.ttl shapes the violations point at, then retry.");
        if mutation {
            err.suggested_command = Some("hali validate".to_string());
        }
        err
    }

    /// The shapes cannot be used. The message gives no reason: rudof's text is never echoed.
    pub fn shapes_invalid() -> Error {
        Error::new(
            "SHAPES_INVALID",
            3,
            "extensions.ttl has SHACL shapes that hali cannot use".to_string(),
            json!({"file": "extensions.ttl"}),
        )
        .fix("Fix or remove the shapes in extensions.ttl; hali applies SHACL Core shapes from it.")
    }

    pub fn label_taken(label: &str, language: &str, concept: &str) -> Error {
        let mut err = Error::new(
            "LABEL_TAKEN",
            1,
            format!("label \"{label}\"@{language} is already used by Concept {concept}"),
            json!({"label": label, "language": language, "concept": concept}),
        )
        .fix("Use the existing Concept, or choose a different label.");
        err.suggested_command = Some("hali concept list".to_string());
        err
    }

    /// The data dir cannot be resolved, read or created; `path` is `None` when unresolvable.
    pub fn store_io(path: Option<&Path>) -> Error {
        let message = match path {
            Some(p) => format!("cannot read or write {}", p.display()),
            None => {
                "cannot locate the data dir: set HALI_DATA_DIR, XDG_DATA_HOME or HOME".to_string()
            }
        };
        Error::new(
            "STORE_IO",
            3,
            message,
            json!({"path": path.map(|p| p.display().to_string())}),
        )
        .fix("Check that the data dir is a directory you can read and write.")
    }

    /// A store file is not valid Turtle. The message never quotes the file.
    pub fn store_unparseable(file: &str, line: u64, column: u64) -> Error {
        Error::new(
            "STORE_UNPARSEABLE",
            3,
            format!("{file} is not valid Turtle (line {line}, column {column})"),
            json!({"file": file, "line": line, "column": column}),
        )
        .fix(&format!(
            "Fix the syntax of {file} at that position, or restore it from a backup."
        ))
    }

    pub fn config_invalid(path: &Path, reason: &str) -> Error {
        Error::new(
            "CONFIG_INVALID",
            3,
            format!("invalid config file {}: {reason}", path.display()),
            json!({"path": path.display().to_string()}),
        )
        .fix("Fix the config file; only [vocabulary] language is read.")
    }
}

impl From<storage::Error> for Error {
    fn from(err: storage::Error) -> Error {
        match err {
            storage::Error::Io { path } => Error::store_io(Some(&path)),
            storage::Error::Unparseable { file, line, column } => {
                Error::store_unparseable(&file, line, column)
            }
        }
    }
}

/// The slug of a label: lowercase, runs of anything but Unicode alphanumerics become `-`,
/// with no leading or trailing `-`.
pub fn slug(label: &str) -> String {
    let mut out = String::new();
    let mut gap = false;
    for c in label.chars() {
        if c.is_alphanumeric() {
            if gap && !out.is_empty() {
                out.push('-');
            }
            gap = false;
            out.extend(c.to_lowercase());
        } else {
            gap = true;
        }
    }
    out
}

/// Trims a label and collapses its whitespace runs to one space. A label must have a letter
/// or digit and no control characters (a tab counts as whitespace).
pub fn normalise_label(label: &str) -> Result<String, Error> {
    if label.chars().any(|c| c.is_control() && c != '\t') {
        return Err(Error::usage("a label must not contain control characters"));
    }
    let label = label.split_whitespace().collect::<Vec<_>>().join(" ");
    if label.is_empty() {
        return Err(Error::usage("a label must not be empty"));
    }
    if !label.chars().any(char::is_alphanumeric) {
        return Err(Error::usage("a label needs at least one letter or digit"));
    }
    Ok(label)
}
