//! Goal tests for the Resource SHACL shapes (#10, spec §2.7): exactly one downloadURL (an IRI
//! matching `^file:///`), one media type (`dcat:mediaType` or `dcterms:format`), one checksum
//! and one title per Resource.
//!
//! Fixtures sit in tests/fixtures/shacl/resource-*/resources.ttl. This file only asserts what
//! the contract says (the Resource is the focus node, and the path where one property is at
//! fault); the shape names are the implementer's, who also adds them to `SHAPE_FIXTURES` in
//! tests/core_shacl.rs, pointing at these fixtures.

mod goal {
    use hali::core::validate::validate;
    use std::path::Path;

    const FOCUS: &str = "<urn:uuid:0192f0c4-7b1e-7cc3-9a43-6c1d2e8f4b10>";
    const TITLE: &str = "http://purl.org/dc/terms/title";
    const URL: &str = "http://www.w3.org/ns/dcat#downloadURL";
    const CHECKSUM: &str = "http://spdx.org/rdf/terms#checksum";

    fn load(name: &str) -> hali::storage::Store {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/shacl")
            .join(name);
        hali::storage::load(&dir).unwrap()
    }

    #[test]
    fn a_complete_resource_conforms() {
        let report = validate(&load("resource-conforming"), 0, 1000).unwrap();
        assert!(
            report.conforms,
            "{:?}",
            report
                .results
                .iter()
                .map(|v| (&v.focus_node, &v.shape, &v.message))
                .collect::<Vec<_>>()
        );
        assert_eq!(report.total, 0);
    }

    #[test]
    fn each_resource_rule_has_a_violating_fixture() {
        let cases: [(&str, Option<&str>); 6] = [
            ("resource-no-title", Some(TITLE)),
            ("resource-two-download-urls", Some(URL)),
            ("resource-non-file-download-url", Some(URL)),
            ("resource-no-media-type", None),
            ("resource-both-media-types", None),
            ("resource-two-checksums", Some(CHECKSUM)),
        ];
        for (fixture, path) in cases {
            let report = validate(&load(fixture), 0, 1000).unwrap();
            assert!(!report.conforms, "{fixture} should not conform");
            let at_fault: Vec<_> = report
                .results
                .iter()
                .filter(|v| v.focus_node == FOCUS)
                .collect();
            assert!(
                !at_fault.is_empty(),
                "{fixture}: no violation points at the Resource: {:?}",
                report
                    .results
                    .iter()
                    .map(|v| &v.focus_node)
                    .collect::<Vec<_>>()
            );
            if let Some(path) = path {
                assert!(
                    at_fault.iter().any(|v| v.path.as_deref() == Some(path)),
                    "{fixture}: no violation on {path}: {:?}",
                    at_fault.iter().map(|v| &v.path).collect::<Vec<_>>()
                );
            }
            for v in &at_fault {
                assert_eq!(v.severity, "Violation", "{fixture}");
            }
        }
    }

    /// #16 (spec §2.7): every `dcterms:subject` value of a Resource is a `skos:Concept` with
    /// `skos:inScheme hali:topics`. The shape is named (the implementer's name); add it to
    /// `SHAPE_FIXTURES` in tests/core_shacl.rs with one of these fixtures.
    #[cfg(feature = "goal")]
    #[test]
    fn subject_must_be_topics_concept() {
        const SUBJECT: &str = "http://purl.org/dc/terms/subject";
        let report = validate(&load("resource-subject-conforming"), 0, 1000).unwrap();
        assert!(
            report.conforms,
            "{:?}",
            report
                .results
                .iter()
                .map(|v| (&v.focus_node, &v.shape, &v.message))
                .collect::<Vec<_>>()
        );
        assert_eq!(report.total, 0);

        // A subject that is no Concept at all; a Concept of another scheme.
        for fixture in [
            "resource-subject-not-concept",
            "resource-subject-other-scheme",
        ] {
            let report = validate(&load(fixture), 0, 1000).unwrap();
            assert!(!report.conforms, "{fixture} should not conform");
            let at_fault: Vec<_> = report
                .results
                .iter()
                .filter(|v| v.focus_node == FOCUS)
                .collect();
            assert!(
                !at_fault.is_empty(),
                "{fixture}: no violation points at the Resource: {:?}",
                report
                    .results
                    .iter()
                    .map(|v| &v.focus_node)
                    .collect::<Vec<_>>()
            );
            for v in &at_fault {
                assert_eq!(v.severity, "Violation", "{fixture}");
                assert!(
                    v.path.as_deref().is_none_or(|p| p == SUBJECT),
                    "{fixture}: {:?}",
                    v.path
                );
                assert!(
                    v.shape
                        .as_deref()
                        .is_some_and(|s| s.starts_with("https://w3id.org/hali#")),
                    "{fixture}: a named hali shape, not a blank node: {:?}",
                    v.shape
                );
            }
        }
    }
}
