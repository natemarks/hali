//! `hali validate`: the SHACL report on the store (spec §2.7), and the check every mutation runs.

use super::Error;
use crate::ontology::shacl;
use crate::storage::Store;
use oxigraph::model::Quad;
use serde::Serialize;

pub use crate::ontology::shacl::Violation;

/// One page of the SHACL report.
#[derive(Debug, Serialize)]
pub struct Report {
    pub conforms: bool,
    pub results: Vec<Violation>,
    pub total: usize,
    pub truncated: bool,
    pub next_offset: Option<usize>,
    /// The first results of the whole report, for the `SHACL_VIOLATION` error.
    #[serde(skip)]
    head: Vec<Violation>,
}

impl Report {
    /// The `SHACL_VIOLATION` error that goes with a non-conforming report.
    pub fn error(&self) -> Option<Error> {
        (!self.conforms).then(|| Error::shacl_violation(&self.head, self.total, false))
    }
}

/// Validates the store against the core and extension shapes, sorted and paged.
pub fn validate(store: &Store, offset: usize, limit: usize) -> Result<Report, Error> {
    let all = check(store, &[])?;
    let total = all.len();
    let head = all.iter().take(Error::MAX_VIOLATIONS).cloned().collect();
    let results: Vec<Violation> = all.into_iter().skip(offset).take(limit).collect();
    let end = offset.saturating_add(results.len());
    let truncated = end < total;
    Ok(Report {
        conforms: total == 0,
        results,
        total,
        truncated,
        next_offset: truncated.then_some(end),
        head,
    })
}

/// Refuses a mutation whose quads `extra` would leave the store not conforming.
pub(crate) fn guard(store: &Store, extra: &[Quad]) -> Result<(), Error> {
    let all = check(store, extra)?;
    if all.is_empty() {
        Ok(())
    } else {
        Err(Error::shacl_violation(&all, all.len(), true))
    }
}

/// Every result for the store plus `extra`, sorted by (focus node, shape, path, constraint,
/// value, message), a missing member sorting as "".
fn check(store: &Store, extra: &[Quad]) -> Result<Vec<Violation>, Error> {
    let mut all = shacl::validate(store, extra).map_err(|_| Error::shapes_invalid())?;
    all.sort_by(|a, b| key(a).cmp(&key(b)));
    Ok(all)
}

fn key(v: &Violation) -> [&str; 6] {
    [
        &v.focus_node,
        v.shape.as_deref().unwrap_or(""),
        v.path.as_deref().unwrap_or(""),
        &v.constraint,
        v.value.as_deref().unwrap_or(""),
        &v.message,
    ]
}
