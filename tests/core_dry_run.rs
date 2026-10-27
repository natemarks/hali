//! Goal tests for the change set `core::concept::add` reports (#8).
//!
//! Assumed API:
//!   `hali::core::{Change, Op, Mutation}`
//!   `Op { Add, Remove }`: Debug, PartialEq, PartialOrd (`Op::Add < Op::Remove`), serialises
//!   lowercase (`"add"`, `"remove"`)
//!   `Change { op: Op, triple: String }`: Debug, Serialize; `{"op":"add","triple":"..."}`
//!   `Mutation<T> { value: T, changes: Vec<Change> }`
//!   `hali::core::concept::add(&Context, &mut Store, &str) -> Result<Mutation<Concept>, Error>`
//!   `changes` lists, in order (add before remove, then the whole triple string in byte
//!   order), the canonical N-Triples (`format!("{triple} .")`) of every triple the real run
//!   adds to a tool-managed graph and the graph did not have yet.
//!   core never touches the filesystem; it works on an in-memory `Store`.

use hali::core::{Change, Context, FixedClock, Mutation, Op, SeededIdGen, concept};
use oxigraph::model::{GraphNameRef, NamedNodeRef, Triple};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const VOCABULARY_GRAPH: &str = "https://w3id.org/hali/graph/vocabulary";

fn ctx() -> Context {
    Context::new(
        "en",
        Box::new(FixedClock::new("2026-10-24T09:30:00Z")),
        Box::new(SeededIdGen::new(1)),
    )
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn empty_store() -> (hali::storage::Store, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    (hali::storage::load(dir.path()).unwrap(), dir)
}

fn add(store: &mut hali::storage::Store, label: &str) -> Mutation<concept::Concept> {
    match concept::add(&ctx(), store, label) {
        Ok(m) => m,
        Err(e) => panic!("{label}: {} {}", e.rule, e.message),
    }
}

/// `(op, triple)` of every change, as strings.
fn ops(changes: &[Change]) -> Vec<(String, String)> {
    changes
        .iter()
        .map(|c| {
            let v = serde_json::to_value(c).unwrap();
            (v["op"].as_str().unwrap().to_string(), c.triple.clone())
        })
        .collect()
}

/// Every triple of the vocabulary graph, as canonical N-Triples lines.
fn vocabulary(store: &hali::storage::Store) -> BTreeSet<String> {
    let graph = NamedNodeRef::new_unchecked(VOCABULARY_GRAPH);
    store
        .quads_for_pattern(None, None, None, Some(GraphNameRef::NamedNode(graph)))
        .map(|q| {
            let q = q.unwrap();
            format!("{} .", Triple::new(q.subject, q.predicate, q.object))
        })
        .collect()
}

const SCHEME: &str = "<https://w3id.org/hali#topics> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.w3.org/2004/02/skos/core#ConceptScheme> .";

#[test]
fn add_reports_changes_for_a_first_add() {
    let (mut store, _dir) = empty_store();
    let m = add(&mut store, "machine learning");
    assert_eq!(m.value.id, "machine-learning");
    let expected = [
        "<https://w3id.org/hali#machine-learning> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.w3.org/2004/02/skos/core#Concept> .",
        "<https://w3id.org/hali#machine-learning> <http://www.w3.org/2004/02/skos/core#inScheme> <https://w3id.org/hali#topics> .",
        "<https://w3id.org/hali#machine-learning> <http://www.w3.org/2004/02/skos/core#prefLabel> \"machine learning\"@en .",
        SCHEME,
    ];
    let got = ops(&m.changes);
    assert_eq!(got.len(), 4, "{got:?}");
    for (i, (op, triple)) in got.iter().enumerate() {
        assert_eq!(op, "add", "{i}");
        assert_eq!(triple, expected[i], "{i}");
    }
    assert!(m.changes.iter().all(|c| c.op == Op::Add));
}

#[test]
fn add_omits_triples_already_in_the_file() {
    let mut store = hali::storage::load(&fixture("store")).unwrap();
    let m = add(&mut store, "deep learning");
    let got = ops(&m.changes);
    let expected = [
        "<https://w3id.org/hali#deep-learning> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.w3.org/2004/02/skos/core#Concept> .",
        "<https://w3id.org/hali#deep-learning> <http://www.w3.org/2004/02/skos/core#inScheme> <https://w3id.org/hali#topics> .",
        "<https://w3id.org/hali#deep-learning> <http://www.w3.org/2004/02/skos/core#prefLabel> \"deep learning\"@en .",
    ];
    assert_eq!(got.len(), 3, "the scheme triple is already there: {got:?}");
    for (i, (op, triple)) in got.iter().enumerate() {
        assert_eq!(op, "add");
        assert_eq!(triple, expected[i]);
    }
}

#[test]
fn add_lists_the_scheme_when_only_another_graph_has_it() {
    // The scheme triple sits in extensions.ttl; the real run still writes it to vocabulary.ttl.
    let mut store = hali::storage::load(&fixture("dry-run/scheme-in-extensions")).unwrap();
    let m = add(&mut store, "machine learning");
    let got = ops(&m.changes);
    assert_eq!(got.len(), 4, "{got:?}");
    assert!(
        got.iter().any(|(op, t)| op == "add" && t == SCHEME),
        "{got:?}"
    );
}

#[test]
fn changes_match_the_store_diff() {
    for name in ["store", "noncanonical", "dry-run/scheme-in-extensions"] {
        let mut store = hali::storage::load(&fixture(name)).unwrap();
        let before = vocabulary(&store);
        let m = add(&mut store, "deep learning");
        let after = vocabulary(&store);
        let diff: BTreeSet<String> = after.difference(&before).cloned().collect();
        assert!(before.is_subset(&after), "{name}: an add removes nothing");
        let changed: BTreeSet<String> = m.changes.iter().map(|c| c.triple.clone()).collect();
        assert_eq!(changed, diff, "{name}");
        assert_eq!(changed.len(), m.changes.len(), "{name}: no duplicates");
    }
}

#[test]
fn changes_escape_literals_as_n_triples() {
    let (mut store, _dir) = empty_store();
    let m = add(&mut store, "say \"hi\" \\ now");
    assert_eq!(m.value.id, "say-hi-now");
    let expected = "<https://w3id.org/hali#say-hi-now> <http://www.w3.org/2004/02/skos/core#prefLabel> \"say \\\"hi\\\" \\\\ now\"@en .";
    assert!(
        m.changes.iter().any(|c| c.triple == expected),
        "{:?}",
        ops(&m.changes)
    );
}

#[test]
fn change_serializes_as_op_and_triple() {
    let add = Change {
        op: Op::Add,
        triple: "<a> <b> <c> .".to_string(),
    };
    assert_eq!(
        serde_json::to_string(&add).unwrap(),
        r#"{"op":"add","triple":"<a> <b> <c> ."}"#
    );
    let remove = Change {
        op: Op::Remove,
        triple: "<a> <b> <c> .".to_string(),
    };
    assert_eq!(
        serde_json::to_string(&remove).unwrap(),
        r#"{"op":"remove","triple":"<a> <b> <c> ."}"#
    );
    assert!(Op::Add < Op::Remove);
}

#[test]
fn changes_are_sorted_by_op_then_triple() {
    // "topics" takes the id `topics-2` (the scheme owns `topics`). In byte order
    // `<...#topics-2>` sorts before `<...#topics>`: '-' (0x2D) < '>' (0x3E).
    let (mut store, _dir) = empty_store();
    let m = add(&mut store, "topics");
    assert_eq!(m.value.id, "topics-2");
    let got = ops(&m.changes);
    assert_eq!(got.len(), 4, "{got:?}");
    assert!(got.iter().all(|(op, _)| op == "add"));
    assert!(
        got[0].1.starts_with("<https://w3id.org/hali#topics-2> "),
        "{got:?}"
    );
    assert_eq!(got[3].1, SCHEME);
    let mut sorted = got.clone();
    sorted.sort();
    assert_eq!(got, sorted);
    // rdf:type is not forced first: the order is the subject's byte order, then the rest.
    assert!(got[0].1.contains("rdf-syntax-ns#type"));
    assert!(got[1].1.contains("skos/core#inScheme"));
    assert!(got[2].1.contains("skos/core#prefLabel"));
}
