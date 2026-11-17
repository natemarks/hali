//! The remembered Scan roots (spec §7): the contents of `roots.json`, text in and text out.
//!
//! The file is `{"version":1,"roots":[<entry>…]}` and a newline, compact, the roots sorted in
//! byte order. An entry holds the flags its last successful `scan <root>` was given.

use serde::Serialize;
use serde_json::{Map, Value};

/// The version of `roots.json` this build reads and writes.
const VERSION: u64 = 1;

/// One remembered Scan root and the scan flags it was given on the command line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RootEntry {
    /// The real, absolute path of the root.
    pub root: String,
    pub include_hidden: bool,
    pub no_ignore: bool,
    /// The `--exclude` globs, in the order given.
    pub exclude: Vec<String>,
    /// `--max-size`, in bytes.
    pub max_size: Option<u64>,
    /// The `--media-type` patterns, lowercase, in the order given.
    pub media_types: Vec<String>,
}

/// Why the text is not a valid `roots.json`.
#[derive(Debug, PartialEq, Eq)]
pub struct Invalid(pub String);

/// Parses the contents of `roots.json`. Anything not of the exact shape, another `version`, or a
/// root that is not an absolute path is `Invalid`.
pub fn parse(text: &str) -> Result<Vec<RootEntry>, Invalid> {
    let bad = |why: &str| Invalid(why.to_string());
    let value: Value = serde_json::from_str(text).map_err(|e| Invalid(e.to_string()))?;
    let object = value.as_object().ok_or_else(|| bad("not a JSON object"))?;
    if object.get("version").and_then(Value::as_u64) != Some(VERSION) {
        return Err(bad("version is not 1"));
    }
    let list = object
        .get("roots")
        .and_then(Value::as_array)
        .ok_or_else(|| bad("roots is not a list"))?;
    let mut roots = Vec::new();
    for item in list {
        let entry = item
            .as_object()
            .ok_or_else(|| bad("a root is not an object"))?;
        roots = remember(roots, parse_entry(entry)?);
    }
    Ok(roots)
}

fn parse_entry(entry: &Map<String, Value>) -> Result<RootEntry, Invalid> {
    let bad = |why: &str| Invalid(why.to_string());
    let field = |key: &str| {
        entry
            .get(key)
            .ok_or_else(|| Invalid(format!("a root has no {key}")))
    };
    let wrong = |key: &str| Invalid(format!("a root has a {key} of the wrong type"));
    let flag = |key: &str| field(key)?.as_bool().ok_or_else(|| wrong(key));
    let strings = |key: &str| -> Result<Vec<String>, Invalid> {
        field(key)?
            .as_array()
            .ok_or_else(|| wrong(key))?
            .iter()
            .map(|v| v.as_str().map(str::to_string).ok_or_else(|| wrong(key)))
            .collect()
    };
    let root = field("root")?.as_str().ok_or_else(|| wrong("root"))?;
    if !root.starts_with('/') || root.contains('\0') {
        return Err(bad("a root is not an absolute path"));
    }
    let max_size = match field("max_size")? {
        Value::Null => None,
        v => Some(v.as_u64().ok_or_else(|| wrong("max_size"))?),
    };
    Ok(RootEntry {
        root: root.to_string(),
        include_hidden: flag("include_hidden")?,
        no_ignore: flag("no_ignore")?,
        exclude: strings("exclude")?,
        max_size,
        media_types: strings("media_types")?,
    })
}

/// The canonical contents of `roots.json`, the roots as given (keep them sorted with
/// `remember`).
pub fn serialise(roots: &[RootEntry]) -> String {
    #[derive(Serialize)]
    struct File<'a> {
        version: u64,
        roots: &'a [RootEntry],
    }
    let text = serde_json::to_string(&File {
        version: VERSION,
        roots,
    })
    .expect("roots serialize");
    format!("{text}\n")
}

/// Adds `entry`, or replaces the one for its root; the result is sorted by root in byte order.
pub fn remember(mut roots: Vec<RootEntry>, entry: RootEntry) -> Vec<RootEntry> {
    match roots.binary_search_by(|r| r.root.as_str().cmp(&entry.root)) {
        Ok(i) => roots[i] = entry,
        Err(i) => roots.insert(i, entry),
    }
    roots
}

/// The other roots and the entry of `root` as it was; `None` when `root` is not remembered.
/// The match is exact.
pub fn forget(mut roots: Vec<RootEntry>, root: &str) -> Option<(Vec<RootEntry>, RootEntry)> {
    let i = roots.iter().position(|r| r.root == root)?;
    let gone = roots.remove(i);
    Some((roots, gone))
}
