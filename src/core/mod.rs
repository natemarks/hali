//! Transport-agnostic operations. `core` works on an in-memory store and never touches clap,
//! stdout, the filesystem or the process environment; time, ids and file facts are injected.

pub mod concept;
pub mod config;
pub mod relation;
pub mod resource;
pub mod schema;
pub mod types;
pub mod validate;

use crate::storage;
use oxigraph::model::{NamedNode, Quad, TermRef, TripleRef};
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
        rfc3339(i64::try_from(secs).unwrap_or(i64::MAX))
    }
}

/// Formats seconds since the Unix epoch as `YYYY-MM-DDTHH:MM:SSZ`.
pub fn rfc3339(secs: i64) -> String {
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil date from days since 1970-01-01 (H. Hinnant's algorithm).
    let z = days + 719_468;
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
    apply(store, quads, &[])
}

/// Removes the `remove` quads the store holds and that `add` does not put back, inserts the
/// `add` quads it does not hold yet, and returns what changed, sorted and without duplicates.
pub(crate) fn apply(store: &storage::Store, add: &[Quad], remove: &[Quad]) -> Vec<Change> {
    let held = |quad: &Quad| store.contains(quad).expect("reading the in-memory store");
    let mut changes = Vec::new();
    for quad in remove {
        if add.contains(quad) || !held(quad) {
            continue;
        }
        store
            .remove(quad)
            .expect("removing from the in-memory store");
        changes.push(change(Op::Remove, quad));
    }
    for quad in add {
        if held(quad) {
            continue;
        }
        store
            .insert(quad)
            .expect("inserting into the in-memory store");
        changes.push(change(Op::Add, quad));
    }
    changes.sort();
    changes.dedup();
    changes
}

fn change(op: Op, quad: &Quad) -> Change {
    Change {
        op,
        triple: format!(
            "{} .",
            TripleRef::new(
                quad.subject.as_ref(),
                quad.predicate.as_ref(),
                quad.object.as_ref()
            )
        ),
    }
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

    /// A Concept cannot be removed: `tagged` subjects have it as `dcterms:subject` and
    /// `narrower` other Concepts are under it, in any graph.
    pub fn in_use(concept: &str, tagged: usize, narrower: usize) -> Error {
        let mut err = Error::new(
            "IN_USE",
            1,
            format!(
                "Concept {concept} is in use: {tagged} tagged, broader of {narrower} Concepts; nothing was written"
            ),
            json!({"concept": concept, "tagged": tagged, "narrower": narrower}),
        )
        .fix(&format!(
            "Untag the Resources (hali edit <ref> --remove-tag {id}) and unlink the narrower Concepts (hali concept edit <C> --remove-broader {id}), then retry.",
            id = shell_quote(concept)
        ));
        err.suggested_command = Some(if tagged > 0 {
            format!("hali find --exact {}", flag_value("--tag", concept))
        } else {
            format!("hali concept show {}", shell_quote(concept))
        });
        err
    }

    /// A Type cannot be removed: `typed` subjects have it as `rdf:type` and `subtypes` other
    /// subjects are `rdfs:subClassOf` it, in any graph.
    pub fn type_in_use(id: &str, typed: usize, subtypes: usize) -> Error {
        let mut err = Error::new(
            "IN_USE",
            1,
            format!(
                "Type {id} is in use: {typed} typed, parent of {subtypes} Types; nothing was written"
            ),
            json!({"type": id, "typed": typed, "subtypes": subtypes}),
        )
        .fix(&format!(
            "Remove the Type from the Resources (hali edit <ref> {}) and remove its subtypes first (hali type remove <S>), then retry.",
            flag_value("--remove-type", id)
        ));
        err.suggested_command = Some(if typed > 0 {
            format!("hali find {}", flag_value("--type", id))
        } else {
            "hali type list".to_string()
        });
        err
    }

    /// A Type name is already a Type's id, or its label in the configured language.
    pub fn type_label_taken(label: &str, language: &str, holder: &str) -> Error {
        let mut err = Error::new(
            "LABEL_TAKEN",
            1,
            format!("label \"{label}\"@{language} is already used by Type {holder}"),
            json!({"label": label, "language": language, "type": holder}),
        )
        .fix("Use the existing Type, or choose a different name.");
        err.suggested_command = Some("hali type list".to_string());
        err
    }

    /// A relation kind cannot be removed: `used` triples in any graph have it as predicate, and
    /// `subproperties` other subjects are `rdfs:subPropertyOf` it.
    pub fn relation_in_use(id: &str, used: usize, subproperties: usize) -> Error {
        let mut err = Error::new(
            "IN_USE",
            1,
            format!(
                "relation kind {id} is in use: {used} relations, parent of {subproperties} relation kinds; nothing was written"
            ),
            json!({"relation": id, "used": used, "subproperties": subproperties}),
        )
        .fix(&format!(
            "Remove the relations (hali unrelate <ref> {} <ref>) and remove its sub-relations first (hali relation remove <S>), then retry.",
            shell_quote(id)
        ));
        err.suggested_command = Some("hali relation list".to_string());
        err
    }

    /// A relation kind name is already a kind's id (its local part) or its label in the
    /// configured language.
    pub fn relation_label_taken(label: &str, language: &str, holder: &str) -> Error {
        let mut err = Error::new(
            "LABEL_TAKEN",
            1,
            format!("label \"{label}\"@{language} is already used by relation kind {holder}"),
            json!({"label": label, "language": language, "relation": holder}),
        )
        .fix("Use the existing relation kind, or choose a different name.");
        err.suggested_command = Some("hali relation list".to_string());
        err
    }

    /// A Resource cannot be forgotten without `--force`: `relations` relation triples from
    /// `from` other Resources point at it.
    pub fn forget_blocked_by_relations(resource: &str, relations: usize, from: usize) -> Error {
        let mut err = Error::new(
            "FORGET_BLOCKED_BY_RELATIONS",
            1,
            format!(
                "Resource {resource} has {relations} inbound relations from {from} other Resources; nothing was written"
            ),
            json!({"resource": resource, "relations": relations, "from": from}),
        )
        .fix(&format!(
            "Remove them (hali unrelate <ref> <relation> {}), or forget with --force to remove them too, then retry.",
            shell_quote(resource)
        ));
        err.suggested_command = Some(format!("hali show {}", shell_quote(resource)));
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
        .fix("Fix the config file; only [user] name and [vocabulary] language are read.")
    }

    /// A destructive command was not confirmed: `declined` is true when the user answered no
    /// at the prompt, false when there was no terminal to ask. `retry` is the command that
    /// confirms it.
    pub fn confirmation_required(command: &str, retry: &str, declined: bool) -> Error {
        let message = if declined {
            format!("{command} was not confirmed; nothing was written")
        } else {
            format!(
                "{command} needs confirmation, but stdin or stderr is not a terminal; nothing was written"
            )
        };
        let mut err = Error::new(
            "CONFIRMATION_REQUIRED",
            1,
            message,
            json!({"command": command, "declined": declined}),
        )
        .fix("Run it again with --yes to confirm, or with --dry-run to see what it would change.");
        err.suggested_command = Some(retry.to_string());
        err
    }

    /// A path names no regular file. `kind` is `missing`, `directory` or `other`.
    pub fn not_a_regular_file(path: &str, kind: &str) -> Error {
        let what = match kind {
            "missing" => "does not exist",
            "directory" => "is a directory",
            _ => "is not a regular file",
        };
        Error::new(
            "NOT_A_REGULAR_FILE",
            1,
            format!("{path} {what}; only regular files can be cataloged"),
            json!({"path": path, "kind": kind}),
        )
        .fix("Give the path of an existing regular file.")
    }

    /// A path is not UTF-8. `path` is its lossy (U+FFFD) form.
    pub fn non_utf8_path(path: &str) -> Error {
        Error::new(
            "NON_UTF8_PATH",
            1,
            format!("{path} is not valid UTF-8; hali catalogs UTF-8 paths only"),
            json!({"path": path}),
        )
        .fix("Rename the file, or the directory on its path, to a valid UTF-8 name.")
    }

    /// A regular file exists but cannot be opened or read.
    pub fn file_unreadable(path: &str) -> Error {
        Error::new(
            "FILE_UNREADABLE",
            1,
            format!("cannot read {path}"),
            json!({"path": path}),
        )
        .fix("Check that you can read the file and the directories above it.")
    }

    /// A Resource reference matches no Resource.
    pub fn unknown_resource(reference: &str) -> Error {
        Error::new(
            "UNKNOWN_RESOURCE",
            1,
            format!("no cataloged Resource matches {reference}"),
            json!({"ref": reference}),
        )
        .fix("Give the path of a cataloged file or the full UUID of a Resource; UUID prefixes are not accepted.")
    }

    /// A Concept reference matches no Concept. `suggestions` are the closest Concepts.
    pub fn unknown_concept(reference: &str, suggestions: &[resource::Tag]) -> Error {
        let mut err = Error::new(
            "UNKNOWN_CONCEPT",
            1,
            format!("no Concept matches {reference}"),
            json!({"concept": reference, "suggestions": suggestions}),
        )
        .fix("Use the slug or a label of an existing Concept, or add the Concept first.");
        err.suggested_command = Some(format!(
            "hali --dry-run concept add {}",
            positional(reference)
        ));
        err
    }

    /// A Type reference matches no Type. `suggestions` are the closest Types.
    pub fn unknown_type(reference: &str, suggestions: &[resource::Tag]) -> Error {
        let mut err = Error::new(
            "UNKNOWN_TYPE",
            1,
            format!("no Type matches {reference}"),
            json!({"type": reference, "suggestions": suggestions}),
        )
        .fix("Use the name of an existing Type, or add the Type first.");
        err.suggested_command = Some(if types::valid_name(reference) {
            format!("hali --dry-run type add {}", shell_quote(reference))
        } else {
            "hali type list".to_string()
        });
        err
    }

    /// A relation reference matches no relation kind. `suggestions` are the closest kinds.
    pub fn unknown_relation(reference: &str, suggestions: &[resource::Tag]) -> Error {
        let mut err = Error::new(
            "UNKNOWN_RELATION",
            1,
            format!("no relation kind matches {reference}"),
            json!({"relation": reference, "suggestions": suggestions}),
        )
        .fix("Use an existing relation kind (hali relation list), or add one first.");
        err.suggested_command = Some(if relation::valid_name(reference) {
            format!(
                "hali --dry-run relation add {} --parent dcterms:relation",
                shell_quote(reference)
            )
        } else {
            "hali relation list".to_string()
        });
        err
    }
}

/// Whether an IRI already appears in any position of any graph.
pub(crate) fn is_used(store: &storage::Store, iri: &NamedNode) -> bool {
    store
        .quads_for_pattern(Some(iri.as_ref().into()), None, None, None)
        .next()
        .is_some()
        || store
            .quads_for_pattern(None, Some(iri.as_ref()), None, None)
            .next()
            .is_some()
        || store
            .quads_for_pattern(None, None, Some(TermRef::from(iri.as_ref())), None)
            .next()
            .is_some()
}

/// `<flag> <value>` as a shell reads it back. A value that starts with `-` would be read as a
/// flag, so it is attached with `=`.
pub fn flag_value(flag: &str, value: &str) -> String {
    let sep = if value.starts_with('-') { '=' } else { ' ' };
    format!("{flag}{sep}{}", shell_quote(value))
}

/// A positional argument as a shell reads it back. One that starts with `-` would be read as a
/// flag, so it follows `--`.
pub fn positional(value: &str) -> String {
    let separator = if value.starts_with('-') { "-- " } else { "" };
    format!("{separator}{}", shell_quote(value))
}

/// A word as a POSIX shell reads it back: as is when it is made of `[A-Za-z0-9._-]` only,
/// otherwise single-quoted, with each `'` written as `'\''`.
pub fn shell_quote(word: &str) -> String {
    let safe = !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if safe {
        word.to_string()
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
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

/// How labels compare for uniqueness: whitespace-normalised and case-insensitive.
pub(crate) fn fold(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// At most this many suggestions go into an unknown-reference error.
const MAX_SUGGESTIONS: usize = 3;

/// A Concept or Type an unknown reference could have meant: its id, every name it answers to
/// (its id among them), and the label it is shown with.
pub(crate) struct Candidate {
    pub id: String,
    pub names: Vec<String>,
    pub label: String,
}

/// The candidates closest to the folded reference `key`: those with a folded name that contains
/// it, or is within edit distance max(2, len/3) of it, by distance, label, then id.
pub(crate) fn suggest(
    candidates: impl IntoIterator<Item = Candidate>,
    key: &str,
) -> Vec<resource::Tag> {
    let limit = (key.chars().count() / 3).max(2);
    let mut found: Vec<(usize, resource::Tag)> = candidates
        .into_iter()
        .filter_map(|c| {
            let names: Vec<String> = c.names.iter().map(|n| fold(n)).collect();
            let distance = names.iter().map(|n| levenshtein(n, key)).min()?;
            let close = distance <= limit || names.iter().any(|n| n.contains(key));
            close.then_some((
                distance,
                resource::Tag {
                    id: c.id,
                    label: c.label,
                },
            ))
        })
        .collect();
    found.sort_by(|(da, a), (db, b)| (da, &a.label, &a.id).cmp(&(db, &b.label, &b.id)));
    found
        .into_iter()
        .take(MAX_SUGGESTIONS)
        .map(|(_, t)| t)
        .collect()
}

/// The edit distance between two strings, in characters.
fn levenshtein(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = if ca == *cb {
                diagonal
            } else {
                1 + diagonal.min(above).min(row[j])
            };
            diagonal = above;
        }
    }
    row[b.len()]
}
