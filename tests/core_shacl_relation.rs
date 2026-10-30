//! Goal tests for the relation SHACL shapes (#18, spec §2.4, §2.7): every relation target is a
//! `hali:Resource`, and every custom relation property has a standard parent.
//!
//! The contract names two shapes, both named nodes (no blank nodes):
//!   `hali:RelationTargetShape` (`sh:targetClass hali:Resource`): the object of any triple whose
//!       predicate reaches a standard relation (`dcterms:relation`, `dcterms:isPartOf`,
//!       `dcterms:references`, `prov:wasDerivedFrom`) through `rdfs:subPropertyOf*` is a
//!       `hali:Resource`. The focus node is the Resource that holds the relation.
//!   `hali:RelationParentShape` (`sh:targetClass owl:ObjectProperty`): a property in the `hali:`
//!       namespace reaches a standard relation through `rdfs:subPropertyOf+`. The focus node is
//!       the property. Alignment properties outside `hali:` are not constrained.
//! Both are added to `SHAPE_FIXTURES` in tests/core_shacl.rs, which that test file's
//! `every_core_shape_has_a_violating_fixture` requires (the implementer updates it, pointing at
//! `shacl/relation-target-not-resource` and `shacl/relation-kind-no-parent`).
//!
//! Fixtures sit in tests/fixtures/shacl/relation-*. X is `urn:uuid:…0a01` and Y `…0a02`, both
//! complete Resources; the node `…0a99` is no Resource at all.

mod goal {
    use hali::core::validate::validate;
    use std::path::Path;

    const X: &str = "<urn:uuid:0192f0c4-0000-7000-8000-000000000a01>";
    const ORPHAN: &str = "<https://w3id.org/hali#orphan>";
    const STRAY: &str = "<https://w3id.org/hali#stray>";
    const TARGET_SHAPE: &str = "https://w3id.org/hali#RelationTargetShape";
    const PARENT_SHAPE: &str = "https://w3id.org/hali#RelationParentShape";

    fn load(name: &str) -> hali::storage::Store {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/shacl")
            .join(name);
        hali::storage::load(&dir).unwrap()
    }

    #[test]
    fn relations_to_resources_conform() {
        // A standard relation, a custom kind under it and a custom kind under that, all to a
        // Resource, and an alignment property outside hali: with no standard parent.
        let report = validate(&load("relation-conforming"), 0, 1000).unwrap();
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
    fn each_target_rule_has_a_violating_fixture() {
        // Directly by a standard relation, and through a custom-under-custom chain.
        for fixture in [
            "relation-target-not-resource",
            "relation-subproperty-target-not-resource",
        ] {
            let report = validate(&load(fixture), 0, 1000).unwrap();
            assert!(!report.conforms, "{fixture} should not conform");
            assert!(report.total >= 1, "{fixture}");
            // Every result is about X (the shape may report one result per matching path).
            for v in &report.results {
                assert_eq!(
                    v.focus_node, X,
                    "{fixture}: the Resource that holds the relation"
                );
                assert_eq!(v.shape.as_deref(), Some(TARGET_SHAPE), "{fixture}");
                assert_eq!(v.severity, "Violation", "{fixture}");
                assert!(
                    v.value
                        .as_deref()
                        .is_none_or(|n| n == "<urn:uuid:0192f0c4-0000-7000-8000-000000000a99>"),
                    "{fixture}: the value is the target: {:?}",
                    v.value
                );
            }
        }
    }

    #[test]
    fn each_parent_rule_has_a_violating_fixture() {
        // No parent at all, and a parent that is no standard relation (an alignment property).
        for (fixture, focus) in [
            ("relation-kind-no-parent", ORPHAN),
            ("relation-kind-nonstandard-parent", STRAY),
        ] {
            let report = validate(&load(fixture), 0, 1000).unwrap();
            assert!(!report.conforms, "{fixture} should not conform");
            assert!(report.total >= 1, "{fixture}");
            // Every result is about the property at fault.
            for v in &report.results {
                assert_eq!(v.focus_node, focus, "{fixture}");
                assert_eq!(v.shape.as_deref(), Some(PARENT_SHAPE), "{fixture}");
                assert_eq!(v.severity, "Violation", "{fixture}");
            }
        }
    }

    #[test]
    fn the_shapes_are_named_nodes() {
        // A violation names its shape by IRI: a blank-node shape would show `None`.
        for fixture in [
            "relation-target-not-resource",
            "relation-kind-no-parent",
            "relation-kind-nonstandard-parent",
        ] {
            let report = validate(&load(fixture), 0, 1000).unwrap();
            assert!(!report.results.is_empty(), "{fixture} should not conform");
            assert!(
                report.results.iter().all(|v| v
                    .shape
                    .as_deref()
                    .is_some_and(|s| s.starts_with("https://w3id.org/hali#"))),
                "{fixture}"
            );
        }
    }
}
