//! Goal tests for the `storage` seam (#3): one named graph per file, deterministic Turtle.
//!
//! Assumed API:
//!   `hali::storage::load(dir: &Path) -> Result<hali::storage::Store, E>` (E: Debug),
//!       where `Store` is `oxigraph::store::Store`; missing files are empty graphs.
//!   `hali::storage::to_turtle(store: &Store, file: hali::storage::File) -> String`,
//!       with `File::{Vocabulary, Resources}` (extensions.ttl is read-only to hali).

use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

const GRAPH_BASE: &str = "https://w3id.org/hali/graph/";

/// The N-Triples-ish `s p o` lines of one named graph, sorted.
fn graph_lines(store: &hali::storage::Store, name: &str) -> Vec<String> {
    let graph = format!("<{GRAPH_BASE}{name}>");
    let mut lines: Vec<String> = store
        .iter()
        .map(|q| q.unwrap())
        .filter(|q| q.graph_name.to_string() == graph)
        .map(|q| format!("{} {} {}", q.subject, q.predicate, q.object))
        .collect();
    lines.sort();
    lines
}

#[test]
fn load_puts_each_file_in_its_named_graph() {
    let store = hali::storage::load(&fixture("store")).unwrap();

    // Nothing lands in the default graph.
    assert!(
        store.iter().map(|q| q.unwrap()).all(|q| q
            .graph_name
            .to_string()
            .starts_with(&format!("<{GRAPH_BASE}"))),
        "every quad must be in a named graph"
    );

    let vocabulary = graph_lines(&store, "vocabulary");
    assert_eq!(vocabulary.len(), 19, "{vocabulary:#?}");
    assert!(vocabulary.contains(
        &"<https://w3id.org/hali#ml> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.w3.org/2004/02/skos/core#Concept>"
            .to_string()
    ));
    assert!(vocabulary.contains(
        &"<https://w3id.org/hali#a-first> <http://www.w3.org/2004/02/skos/core#prefLabel> \"Zulu\"@en"
            .to_string()
    ));

    let resources = graph_lines(&store, "resources");
    assert_eq!(resources.len(), 4, "{resources:#?}");
    assert!(resources.contains(
        &"<urn:uuid:0192f0c4-7b1e-7cc3-9a43-6c1d2e8f4b10> <http://purl.org/dc/terms/title> \"Linked Data Patterns\""
            .to_string()
    ));

    let extensions = graph_lines(&store, "extensions");
    assert_eq!(
        extensions,
        vec![
            "<https://w3id.org/hali#ml> <http://www.w3.org/2004/02/skos/core#exactMatch> <http://example.org/ml>"
                .to_string()
        ]
    );

    // The core ontology is embedded in the binary and sits in its own graph.
    let core = graph_lines(&store, "core");
    assert!(
        !core.is_empty(),
        "the embedded core ontology must be loaded"
    );
    assert!(
        core.iter()
            .any(|l| l.starts_with("<https://w3id.org/hali#Resource> ")),
        "the core graph declares hali:Resource"
    );
    assert!(core.iter().all(|l| !vocabulary.contains(l)));
}

#[test]
fn load_of_an_empty_directory_has_only_the_core_graph() {
    let dir = tempfile::tempdir().unwrap();
    let store = hali::storage::load(dir.path()).unwrap();
    assert!(graph_lines(&store, "vocabulary").is_empty());
    assert!(graph_lines(&store, "resources").is_empty());
    assert!(graph_lines(&store, "extensions").is_empty());
    assert!(!graph_lines(&store, "core").is_empty());
    assert_eq!(
        std::fs::read_dir(dir.path()).unwrap().count(),
        0,
        "load must not create files"
    );
}

#[test]
fn to_turtle_matches_canonical_bytes() {
    // Same graph as fixtures/store/vocabulary.ttl, written by hand in a different
    // order, with other prefix names and no header.
    let store = hali::storage::load(&fixture("noncanonical")).unwrap();
    let got = hali::storage::to_turtle(&store, hali::storage::File::Vocabulary);
    let want = std::fs::read_to_string(fixture("store").join("vocabulary.ttl")).unwrap();
    assert_eq!(got, want);
}

#[test]
fn round_trip_is_byte_identical() {
    let store = hali::storage::load(&fixture("store")).unwrap();
    for (file, name) in [
        (hali::storage::File::Vocabulary, "vocabulary.ttl"),
        (hali::storage::File::Resources, "resources.ttl"),
    ] {
        let want = std::fs::read_to_string(fixture("store").join(name)).unwrap();
        assert_eq!(hali::storage::to_turtle(&store, file), want, "{name}");
    }
}
