//! `query`: read-only SPARQL over the store (spec §9.9).
//!
//! The text is parsed with the §3.4 prefix table predeclared. SPARQL Update is recognised by
//! its grammar and refused; it is never run. A query runs over a per-invocation in-memory copy of
//! the store whose default graph is the merge (set union) of the four named graphs, so nothing
//! in the real store is touched and nothing is saved. The evaluator has no HTTP service handler,
//! so hali never opens a network connection.

use super::Error;
use crate::ontology::PREFIXES;
use crate::storage::Store;
use oxigraph::model::{GraphNameRef, Term, Triple};
use oxigraph::sparql::{CancellationToken, QueryEvaluationError, QueryResults, SparqlEvaluator};
use std::collections::HashSet;
use std::sync::mpsc;
use std::time::Duration;

/// The longest parser message put into a syntax error, in characters.
const MAX_PARSER_MESSAGE: usize = 1_000;

/// How long a query may run and how many solutions or triples it may return.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub timeout: Duration,
    pub max_results: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            timeout: Duration::from_secs(30),
            max_results: 100_000,
        }
    }
}

/// A query that parsed. Opaque: `run` evaluates it.
pub struct Prepared {
    text: String,
}

/// What a query answers.
#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    /// SELECT: the variable names (no `?`) and one cell per variable per row; `None` is unbound.
    Solutions {
        variables: Vec<String>,
        rows: Vec<Vec<Option<Term>>>,
    },
    /// ASK.
    Boolean(bool),
    /// CONSTRUCT and DESCRIBE, without duplicates, in no particular order.
    Triples(Vec<Triple>),
}

/// The one place an evaluator is built: no HTTP service handler, the §3.4 prefixes, no base IRI.
fn evaluator() -> SparqlEvaluator {
    let mut evaluator = SparqlEvaluator::new().without_default_http_service_handler();
    for (name, iri) in PREFIXES {
        evaluator = evaluator
            .with_prefix(name, iri)
            .expect("the prefix table is valid");
    }
    evaluator
}

/// Parses SPARQL Query text. Text that parses as SPARQL Update is `READ_ONLY_QUERY`; text that
/// parses as neither is a usage error.
pub fn parse(text: &str) -> Result<Prepared, Error> {
    let syntax = match evaluator().parse_query(text) {
        Ok(_) => {
            return Ok(Prepared {
                text: text.to_string(),
            });
        }
        Err(e) => e,
    };
    // An update is only parsed to be recognised; it is never evaluated. Text with no operation
    // (empty, comments, a prologue) asks nothing and changes nothing: a usage error.
    if evaluator().parse_update(text).is_ok() && !has_no_operation(text) {
        return Err(Error::read_only_query());
    }
    Err(invalid_query(&syntax.to_string()))
}

/// Whether `text` parses as a SPARQL Update with no operation. Such text names no prefix, so it
/// parses without the prefix table, and prints as its `BASE` line at most (one line per
/// operation otherwise).
#[expect(
    deprecated,
    reason = "oxigraph's Update is the only parser that shows the operations"
)]
fn has_no_operation(text: &str) -> bool {
    oxigraph::sparql::Update::parse(text, None).is_ok_and(|u| {
        u.to_string()
            .lines()
            .all(|l| l.trim().is_empty() || l.starts_with("BASE <"))
    })
}

fn invalid_query(parser_message: &str) -> Error {
    let message: String = parser_message.chars().take(MAX_PARSER_MESSAGE).collect();
    Error::usage(&format!("invalid SPARQL query: {message}"))
}

/// Evaluates `query` over `store`, within `limits`. Reads the store only.
///
/// The evaluation runs on a worker thread and the caller waits at most `limits.timeout` for it,
/// so the limit holds for every phase (joins, sorts), not only for the store scans that check
/// the cancellation token. On timeout the token is cancelled and the worker is left behind, not
/// joined: it works on its own in-memory copy and ends with the process.
pub fn run(store: &Store, query: &Prepared, limits: Limits) -> Result<Answer, Error> {
    let merged = merged(store);
    let token = CancellationToken::new();
    let (sender, receiver) = mpsc::channel();
    let worker = {
        let token = token.clone();
        let text = query.text.clone();
        std::thread::spawn(move || {
            let _ = sender.send(evaluate(&merged, &text, token, limits.max_results));
        })
    };
    match receiver.recv_timeout(limits.timeout) {
        Ok(answer) => answer,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            token.cancel();
            Err(Error::query_failed("timeout", None))
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => match worker.join() {
            Err(panic) => std::panic::resume_unwind(panic),
            Ok(()) => unreachable!("the worker sends its answer before it ends"),
        },
    }
}

/// A copy of the store whose default graph is the union of its named graphs; the named graphs
/// stay, for `GRAPH`, `FROM` and `FROM NAMED`.
fn merged(store: &Store) -> Store {
    let copy = Store::new().expect("creating an in-memory store");
    for quad in store.iter() {
        let quad = quad.expect("reading the in-memory store");
        let triple = Triple::new(
            quad.subject.clone(),
            quad.predicate.clone(),
            quad.object.clone(),
        );
        copy.insert(&quad).expect("writing to the in-memory store");
        copy.insert(triple.as_ref().in_graph(GraphNameRef::DefaultGraph))
            .expect("writing to the in-memory store");
    }
    copy
}

fn evaluate(
    store: &Store,
    text: &str,
    token: CancellationToken,
    max: usize,
) -> Result<Answer, Error> {
    let prepared = evaluator()
        .with_cancellation_token(token)
        .parse_query(text)
        .map_err(|e| invalid_query(&e.to_string()))?;
    match prepared.on_store(store).execute().map_err(failed)? {
        QueryResults::Boolean(b) => Ok(Answer::Boolean(b)),
        QueryResults::Solutions(solutions) => {
            let variables: Vec<_> = solutions.variables().to_vec();
            let mut rows = Vec::new();
            for solution in solutions {
                let solution = solution.map_err(failed)?;
                if rows.len() == max {
                    return Err(Error::query_failed("too_large", None));
                }
                rows.push(variables.iter().map(|v| solution.get(v).cloned()).collect());
            }
            Ok(Answer::Solutions {
                variables: variables.iter().map(|v| v.as_str().to_string()).collect(),
                rows,
            })
        }
        QueryResults::Graph(triples) => {
            let mut seen = HashSet::new();
            let mut out = Vec::new();
            for triple in triples {
                let triple = triple.map_err(failed)?;
                if seen.insert(triple.clone()) {
                    if out.len() == max {
                        return Err(Error::query_failed("too_large", None));
                    }
                    out.push(triple);
                }
            }
            Ok(Answer::Triples(out))
        }
    }
}

/// An evaluation error as a hali error.
fn failed(err: QueryEvaluationError) -> Error {
    match err {
        QueryEvaluationError::Cancelled => Error::query_failed("timeout", None),
        QueryEvaluationError::UnsupportedService(service) => {
            Error::query_failed("service", Some(service.as_str()))
        }
        QueryEvaluationError::InvalidServiceName(term) => {
            Error::query_failed("service", Some(&term.to_string()))
        }
        QueryEvaluationError::UnboundService => Error::query_failed("service", Some("?")),
        // The copy is in memory and the evaluator defines no custom function, so any other
        // error comes from the query text (an unsupported custom function or arity).
        other => invalid_query(&other.to_string()),
    }
}
