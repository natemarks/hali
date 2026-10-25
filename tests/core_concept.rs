//! Goal tests for `core::concept::{add, list}` and `core::slug` (#3).
//!
//! Assumed API:
//!   `hali::core::slug(label: &str) -> String`
//!   `hali::core::Context::new(language: &str, Box<dyn Clock>, Box<dyn IdGen>) -> Context`
//!   `hali::core::concept::add(&Context, &mut Store, label: &str) -> Result<Concept, hali::core::Error>`
//!   `hali::core::concept::list(&Context, &Store, offset: usize, limit: usize) -> Page`
//!   `Concept { id, label: String, alt_labels, broader: Vec<String> }`
//!   `Page { results: Vec<Concept>, total: usize, truncated: bool, next_offset: Option<usize> }`
//!   `hali::core::Error { rule: &str/String, message: String, details: serde_json::Value,
//!                        suggested_fix: Option<String>, suggested_command: Option<String> }`
//!   core never touches the filesystem; it works on an in-memory `Store`.

#[cfg(feature = "goal")]
use hali::core::{Context, FixedClock, SeededIdGen, concept};
#[cfg(feature = "goal")]
use std::path::{Path, PathBuf};

#[cfg(feature = "goal")]
fn ctx(language: &str) -> Context {
    Context::new(
        language,
        Box::new(FixedClock::new("2026-10-24T09:30:00Z")),
        Box::new(SeededIdGen::new(1)),
    )
}

#[cfg(feature = "goal")]
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[cfg(feature = "goal")]
fn empty_store() -> (hali::storage::Store, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    (hali::storage::load(dir.path()).unwrap(), dir)
}

#[cfg(feature = "goal")]
#[test]
fn slug_rules() {
    use hali::core::slug;
    assert_eq!(slug("machine learning"), "machine-learning");
    assert_eq!(slug("Machine Learning"), "machine-learning");
    assert_eq!(slug("  C++ & Rust!!  "), "c-rust");
    assert_eq!(slug("a---b"), "a-b");
    assert_eq!(slug("-- x --"), "x");
    assert_eq!(slug("RDF 1.1"), "rdf-1-1");
    // Unicode alphanumerics are kept.
    assert_eq!(slug("Ünïcode Straße"), "ünïcode-straße");
    assert_eq!(slug("日本語 データ"), "日本語-データ");
}

#[cfg(feature = "goal")]
#[test]
fn add_writes_concept_triples_and_scheme() {
    let (mut store, _dir) = empty_store();
    let c = concept::add(&ctx("en"), &mut store, "machine learning").unwrap();
    assert_eq!(c.id, "machine-learning");
    assert_eq!(c.label, "machine learning");
    assert!(c.alt_labels.is_empty());
    assert!(c.broader.is_empty());

    let got = hali::storage::to_turtle(&store, hali::storage::File::Vocabulary);
    let want = std::fs::read_to_string(fixture("first-add").join("vocabulary.ttl")).unwrap();
    assert_eq!(got, want);
}

#[cfg(feature = "goal")]
#[test]
fn add_normalises_whitespace_in_the_label() {
    let (mut store, _dir) = empty_store();
    let c = concept::add(&ctx("en"), &mut store, "  machine \t  learning ").unwrap();
    assert_eq!(c.label, "machine learning");
    assert_eq!(c.id, "machine-learning");
}

#[cfg(feature = "goal")]
#[test]
fn add_uses_the_configured_language_tag() {
    let (mut store, _dir) = empty_store();
    concept::add(&ctx("de"), &mut store, "Maschinelles Lernen").unwrap();
    let ttl = hali::storage::to_turtle(&store, hali::storage::File::Vocabulary);
    assert!(
        ttl.contains("skos:prefLabel \"Maschinelles Lernen\"@de"),
        "{ttl}"
    );
}

#[cfg(feature = "goal")]
#[test]
fn colliding_slugs_get_numeric_suffixes() {
    let (mut store, _dir) = empty_store();
    let a = concept::add(&ctx("en"), &mut store, "Rust").unwrap();
    let b = concept::add(&ctx("en"), &mut store, "rust!").unwrap();
    let c = concept::add(&ctx("en"), &mut store, "Rust?").unwrap();
    assert_eq!(
        (a.id.as_str(), b.id.as_str(), c.id.as_str()),
        ("rust", "rust-2", "rust-3")
    );
}

#[cfg(feature = "goal")]
#[test]
fn duplicate_label_is_label_taken() {
    let (mut store, _dir) = empty_store();
    concept::add(&ctx("en"), &mut store, "machine learning").unwrap();
    let before = hali::storage::to_turtle(&store, hali::storage::File::Vocabulary);

    // Case-insensitive, whitespace-normalised.
    let err = concept::add(&ctx("en"), &mut store, "Machine   LEARNING").unwrap_err();
    assert_eq!(err.rule, "LABEL_TAKEN");
    assert_eq!(
        err.message,
        "label \"Machine LEARNING\"@en is already used by Concept machine-learning"
    );
    assert_eq!(err.details["label"], "Machine LEARNING");
    assert_eq!(err.details["language"], "en");
    assert_eq!(err.details["concept"], "machine-learning");
    assert_eq!(
        err.suggested_fix.as_deref(),
        Some("Use the existing Concept, or choose a different label.")
    );
    assert_eq!(err.suggested_command.as_deref(), Some("hali concept list"));

    // The store is unchanged.
    assert_eq!(
        hali::storage::to_turtle(&store, hali::storage::File::Vocabulary),
        before
    );
}

#[cfg(feature = "goal")]
#[test]
fn label_taken_also_matches_alt_labels() {
    let mut store = hali::storage::load(&fixture("store")).unwrap();
    let err = concept::add(&ctx("en"), &mut store, "ld").unwrap_err();
    assert_eq!(err.rule, "LABEL_TAKEN");
    assert_eq!(err.details["concept"], "linked-data");
}

#[cfg(feature = "goal")]
#[test]
fn same_label_in_another_language_is_not_taken() {
    let mut store = hali::storage::load(&fixture("store")).unwrap();
    let c = concept::add(&ctx("de"), &mut store, "machine learning").unwrap();
    assert_eq!(c.label, "machine learning");
}

#[cfg(feature = "goal")]
#[test]
fn invalid_labels_are_rejected() {
    let (mut store, _dir) = empty_store();
    for bad in ["", "   ", "!!!", "a\u{7}b", "a\nb"] {
        assert!(
            concept::add(&ctx("en"), &mut store, bad).is_err(),
            "{bad:?} must be rejected"
        );
    }
    let ttl = hali::storage::to_turtle(&store, hali::storage::File::Vocabulary);
    assert!(!ttl.contains("skos:Concept"), "nothing may be added: {ttl}");
}

#[cfg(feature = "goal")]
#[test]
fn list_pages_and_sorts_by_label() {
    let store = hali::storage::load(&fixture("store")).unwrap();
    let ids =
        |p: &concept::Page| -> Vec<String> { p.results.iter().map(|c| c.id.clone()).collect() };

    // Sorted by label in code point order: "Zulu" (uppercase) sorts before "linked data".
    let all = concept::list(&ctx("en"), &store, 0, 50);
    assert_eq!(ids(&all), ["a-first", "linked-data", "ml", "semantic-web"]);
    assert_eq!(all.total, 4);
    assert!(!all.truncated);
    assert_eq!(all.next_offset, None);
    assert_eq!(all.results[0].label, "Zulu");
    let linked = &all.results[1];
    assert_eq!(linked.alt_labels, ["LD", "linkeddata"]);
    assert_eq!(linked.broader, ["semantic-web"]);

    let first = concept::list(&ctx("en"), &store, 0, 2);
    assert_eq!(ids(&first), ["a-first", "linked-data"]);
    assert_eq!(first.total, 4);
    assert!(first.truncated);
    assert_eq!(first.next_offset, Some(2));

    let last = concept::list(&ctx("en"), &store, 2, 2);
    assert_eq!(ids(&last), ["ml", "semantic-web"]);
    assert!(!last.truncated);
    assert_eq!(last.next_offset, None);

    let past_end = concept::list(&ctx("en"), &store, 10, 2);
    assert!(past_end.results.is_empty());
    assert_eq!(past_end.total, 4);
    assert!(!past_end.truncated);
    assert_eq!(past_end.next_offset, None);
}

#[cfg(feature = "goal")]
#[test]
fn list_on_an_empty_store_is_empty() {
    let (store, _dir) = empty_store();
    let page = concept::list(&ctx("en"), &store, 0, 50);
    assert!(page.results.is_empty());
    assert_eq!(page.total, 0);
    assert!(!page.truncated);
    assert_eq!(page.next_offset, None);
}
