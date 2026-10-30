//! Goal tests for `hali describe` in `core` (#19, spec §9.1 **Describe**, §9.5, §10.4, §11.2).
//!
//! Assumed API (invented here; the implementer follows it). Module `hali::core::describe`:
//!   `describe(&Context, &Store, store_path: &str, brief: bool) -> Description`
//!       `store_path` is what `cli` hands over, shown as given (core never resolves, escapes or
//!       canonicalises it, and never touches the filesystem). It reads the store only.
//!   `Description { core_version: String, store: String, counts: Counts,
//!       concepts: Option<Vec<concept::Concept>>, types: Option<Vec<types::Type>>,
//!       relation_kinds: Option<Vec<relation::RelationKind>> }`
//!       `Clone + Debug + PartialEq + Serialize`. The three lists are `Some` unless `brief`
//!       (then `None`, and omitted from the serialization). The fields are serialised in this
//!       order, with no `schema_version` (the CLI wraps the value). Each list equals the
//!       `results` of `concept::list`, `types::list` and `relation::list` with `(0, usize::MAX)`.
//!   `Counts { resources: usize, concepts: usize, types: usize, relation_kinds: usize,
//!       relations: usize }` `Clone + Debug + PartialEq + Serialize`.
//!       `resources`: the named subjects with `rdf:type hali:Resource` in `resources.ttl`;
//!       `concepts`, `types`, `relation_kinds`: the `total` of the three list functions;
//!       `relations`: the distinct (s, p, o) triples, in any graph, whose predicate is a
//!       relation kind (dangling ones included, a triple in two graphs counted once).
//!   `core_version`: the lexical value of `owl:versionInfo` of `<https://w3id.org/hali>` in the
//!       core graph only.
//!
//! The tests read tests/fixtures/relations (see tests/core_relation.rs): 5 Resources, Concept
//! `ml`, Type `Note`, 10 relation kinds (2 only in extensions.ttl) and 6 relation triples (1
//! only in extensions.ttl). No operation here mints an id.

#[cfg(feature = "goal")]
mod goal {
    use hali::core::describe::{Counts, Description, describe};
    use hali::core::relation::{self, RelationKind};
    use hali::core::{Context, FixedClock, SeededIdGen, concept, types};
    use hali::storage::Store;
    use std::path::{Path, PathBuf};

    const PREFIXES: &str = "@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .\n\
        @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .\n\
        @prefix owl: <http://www.w3.org/2002/07/owl#> .\n\
        @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .\n\
        @prefix skos: <http://www.w3.org/2004/02/skos/core#> .\n\
        @prefix dcterms: <http://purl.org/dc/terms/> .\n\
        @prefix prov: <http://www.w3.org/ns/prov#> .\n\
        @prefix hali: <https://w3id.org/hali#> .\n";
    const R1: &str = "0192f0c4-0000-7000-8000-000000000701";
    const R3: &str = "0192f0c4-0000-7000-8000-000000000703";
    /// A well-formed UUID that no Resource has.
    const GONE: &str = "0192f0c4-0000-7000-8000-000000000799";
    const STORE: &str = "/data/hali";

    fn ctx_in(language: &str) -> Context {
        Context::new(
            language,
            Box::new(FixedClock::new("2026-10-24T09:30:00Z")),
            Box::new(SeededIdGen::new(1)),
        )
    }

    fn ctx() -> Context {
        ctx_in("en")
    }

    fn fixture_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/relations")
    }

    fn load() -> Store {
        hali::storage::load(&fixture_dir()).unwrap()
    }

    /// The fixture with Turtle text appended to some of its files (`vocabulary.ttl`,
    /// `resources.ttl`, `extensions.ttl`); the prefixes above are put in front of the text.
    fn load_with(extra: &[(&str, &str)]) -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        for name in ["vocabulary.ttl", "resources.ttl", "extensions.ttl"] {
            let mut text = std::fs::read_to_string(fixture_dir().join(name)).unwrap();
            for (file, more) in extra {
                if *file == name {
                    text.push_str(&format!("\n{PREFIXES}{more}\n"));
                }
            }
            std::fs::write(dir.path().join(name), text).unwrap();
        }
        (hali::storage::load(dir.path()).unwrap(), dir)
    }

    fn counts(resources: usize, concepts: usize, types: usize, kinds: usize, rel: usize) -> Counts {
        Counts {
            resources,
            concepts,
            types,
            relation_kinds: kinds,
            relations: rel,
        }
    }

    fn full(store: &Store) -> Description {
        describe(&ctx(), store, STORE, false)
    }

    fn kind<'a>(d: &'a Description, id: &str) -> &'a RelationKind {
        d.relation_kinds
            .as_ref()
            .unwrap()
            .iter()
            .find(|k| k.id == id)
            .unwrap_or_else(|| panic!("no relation kind {id}"))
    }

    /// The `owl:versionInfo` literal of the ontology in the embedded core.ttl, read here from
    /// the source text and not through the code under test.
    fn core_ttl_version() -> String {
        let text = hali::ontology::CORE_TTL;
        let rest = text
            .split_once("owl:versionInfo \"")
            .expect("core.ttl carries owl:versionInfo")
            .1;
        rest.split_once('"').unwrap().0.to_string()
    }

    // ---- criterion 4: counts ----------------------------------------------------------------

    #[test]
    fn counts_relations_fixture() {
        let d = full(&load());
        assert_eq!(d.counts, counts(5, 1, 1, 10, 6));
        assert_eq!(d.store, STORE);
    }

    #[test]
    fn lists_equal_the_list_commands() {
        let store = load();
        let d = full(&store);
        let c = ctx();
        assert_eq!(
            d.concepts.unwrap(),
            concept::list(&c, &store, 0, usize::MAX).results
        );
        assert_eq!(
            d.types.unwrap(),
            types::list(&c, &store, 0, usize::MAX).results
        );
        assert_eq!(
            d.relation_kinds.unwrap(),
            relation::list(&c, &store, 0, usize::MAX).results
        );
        // And the counts are the totals of the same lists.
        let d = full(&store);
        assert_eq!(d.counts.concepts, d.concepts.as_ref().unwrap().len());
        assert_eq!(d.counts.types, d.types.as_ref().unwrap().len());
        assert_eq!(
            d.counts.relation_kinds,
            d.relation_kinds.as_ref().unwrap().len()
        );
    }

    #[test]
    fn brief_has_no_lists() {
        let store = load();
        let brief = describe(&ctx(), &store, STORE, true);
        assert_eq!(brief.concepts, None);
        assert_eq!(brief.types, None);
        assert_eq!(brief.relation_kinds, None);
        let full = full(&store);
        assert_eq!(brief.core_version, full.core_version);
        assert_eq!(brief.store, full.store);
        assert_eq!(brief.counts, full.counts);
        assert_eq!(brief.counts, counts(5, 1, 1, 10, 6));

        // The serialisation: the three lists are omitted, the other keys stay in order.
        let text = serde_json::to_string(&brief).unwrap();
        assert_eq!(
            text,
            "{\"core_version\":\"0.1.0\",\"store\":\"/data/hali\",\"counts\":{\"resources\":5,\
             \"concepts\":1,\"types\":1,\"relation_kinds\":10,\"relations\":6}}"
        );
        let text = serde_json::to_string(&full).unwrap();
        let keys: Vec<usize> = [
            "\"core_version\"",
            "\"store\"",
            "\"counts\"",
            "\"concepts\"",
            "\"types\"",
            "\"relation_kinds\":[",
        ]
        .iter()
        .map(|k| text.find(k).unwrap_or_else(|| panic!("no {k} in {text}")))
        .collect();
        assert!(keys.windows(2).all(|w| w[0] < w[1]), "{text}");
    }

    #[test]
    fn empty_store_counts() {
        let dir = tempfile::tempdir().unwrap();
        let empty = hali::storage::load(dir.path()).unwrap();
        let absent = hali::storage::load(&dir.path().join("nope")).unwrap();
        for store in [&empty, &absent] {
            let d = full(store);
            assert_eq!(d.counts, counts(0, 0, 0, 4, 0));
            assert_eq!(d.concepts, Some(vec![]));
            assert_eq!(d.types, Some(vec![]));
            let ids: Vec<&str> = d
                .relation_kinds
                .as_ref()
                .unwrap()
                .iter()
                .map(|k| k.id.as_str())
                .collect();
            assert_eq!(
                ids,
                [
                    "dcterms:isPartOf",
                    "dcterms:references",
                    "dcterms:relation",
                    "prov:wasDerivedFrom"
                ]
            );
            assert!(d.relation_kinds.unwrap().iter().all(|k| k.standard));
            assert_eq!(d.core_version, "0.1.0");
        }
        assert_eq!(full(&empty), full(&absent));
    }

    #[test]
    fn relation_in_two_graphs_counts_once() {
        let r = format!("<urn:uuid:{R3}> hali:annotates <urn:uuid:{R1}> .");
        // The same triple, already in resources.ttl, held again by the other two files.
        let (store, _dir) = load_with(&[("extensions.ttl", &r), ("vocabulary.ttl", &r)]);
        assert_eq!(full(&store).counts.relations, 6);
        // A triple only extensions.ttl holds counts (the fixture's `r5 quotes r3` is one).
        let new = format!("<urn:uuid:{R1}> hali:annotates <urn:uuid:{R3}> .");
        let (store, _dir) = load_with(&[("extensions.ttl", &new), ("vocabulary.ttl", &new)]);
        assert_eq!(full(&store).counts.relations, 7);
    }

    #[test]
    fn relations_count_kinds_only_and_dangling_ones() {
        // A dangling target and a standard predicate with a sub-property kind of extensions.
        let (store, _dir) = load_with(&[(
            "resources.ttl",
            &format!(
                "<urn:uuid:{R1}> hali:annotates <urn:uuid:{GONE}> .\n\
                 <urn:uuid:{R1}> dcterms:references <urn:uuid:{R3}> ."
            ),
        )]);
        assert_eq!(full(&store).counts.relations, 8);
        // Predicates that are not relation kinds are not relations.
        let (store, _dir) = load_with(&[(
            "resources.ttl",
            &format!(
                "<urn:uuid:{R1}> dcterms:subject <urn:uuid:{R3}> .\n\
                 <urn:uuid:{R1}> dcterms:title \"x\" .\n\
                 <urn:uuid:{R1}> hali:notAKind <urn:uuid:{R3}> ."
            ),
        )]);
        assert_eq!(full(&store).counts.relations, 6);
    }

    #[test]
    fn resource_only_in_extensions_not_counted() {
        let (store, _dir) = load_with(&[
            (
                "extensions.ttl",
                &format!("<urn:uuid:{GONE}> a hali:Resource ; dcterms:title \"ext\" ."),
            ),
            (
                "vocabulary.ttl",
                "<urn:uuid:0192f0c4-0000-7000-8000-000000000798> a hali:Resource .",
            ),
            // Not typed hali:Resource, and a blank node: neither is a named Resource.
            (
                "resources.ttl",
                "<urn:uuid:0192f0c4-0000-7000-8000-000000000797> a hali:Note .\n\
                 [] a hali:Resource .",
            ),
        ]);
        assert_eq!(full(&store).counts.resources, 5);
        // A Resource of resources.ttl typed twice counts once (r1 is a Resource and a Note).
        let (store, _dir) = load_with(&[(
            "resources.ttl",
            &format!("<urn:uuid:{GONE}> a hali:Resource , hali:Note ."),
        )]);
        assert_eq!(full(&store).counts.resources, 6);
    }

    // ---- criterion 5: the core version ------------------------------------------------------

    #[test]
    fn core_version_is_core_ttl_version_info() {
        let expected = core_ttl_version();
        assert_eq!(expected, "0.1.0");
        assert_eq!(full(&load()).core_version, expected);
        assert_eq!(
            describe(&ctx(), &load(), STORE, true).core_version,
            expected
        );
    }

    #[test]
    fn extensions_cannot_override_core_version() {
        let claim = "<https://w3id.org/hali> owl:versionInfo \"9.9.9\" .";
        let (store, _dir) = load_with(&[
            ("extensions.ttl", claim),
            ("vocabulary.ttl", claim),
            ("resources.ttl", claim),
        ]);
        assert_eq!(full(&store).core_version, core_ttl_version());
        assert_eq!(full(&store).core_version, "0.1.0");
    }

    // ---- criterion 6 (core part): the store path is passed through --------------------------

    #[test]
    fn store_path_is_shown_as_given() {
        let weird = "relative/\u{1b}[31mdata";
        let d = describe(&ctx(), &load(), weird, false);
        assert_eq!(d.store, weird);
    }

    // ---- criterion 8: labels in the configured language -------------------------------------

    #[test]
    fn labels_use_the_configured_language() {
        let store = load();
        let d = describe(&ctx_in("fr"), &store, STORE, false);
        // `annotates` has an @fr label; the others fall back to their ids.
        assert_eq!(kind(&d, "annotates").label, "annote");
        assert_eq!(kind(&d, "summarises").label, "summarises");
        assert_eq!(kind(&d, "dcterms:isPartOf").label, "dcterms:isPartOf");
        // A Concept and a Type with no @fr label: `concept list` gives the Concept an empty
        // label and `describe` inherits that; the Type falls back to its id.
        assert_eq!(d.concepts.as_ref().unwrap()[0].id, "ml");
        assert_eq!(d.concepts.as_ref().unwrap()[0].label, "");
        assert_eq!(d.types.as_ref().unwrap()[0].label, "Note");
        let c = ctx_in("fr");
        assert_eq!(
            d.relation_kinds.unwrap(),
            relation::list(&c, &store, 0, usize::MAX).results
        );
        assert_eq!(d.concepts.unwrap(), {
            concept::list(&c, &store, 0, usize::MAX).results
        });
        // The counts do not depend on the language.
        assert_eq!(
            describe(&ctx_in("fr"), &store, STORE, true).counts,
            counts(5, 1, 1, 10, 6)
        );
    }

    // ---- criterion 9 (core part): nothing is written ----------------------------------------

    #[test]
    fn describe_reads_only() {
        let store = load();
        let before: std::collections::BTreeSet<String> =
            store.iter().map(|q| q.unwrap().to_string()).collect();
        let _ = full(&store);
        let _ = describe(&ctx(), &store, STORE, true);
        let after: std::collections::BTreeSet<String> =
            store.iter().map(|q| q.unwrap().to_string()).collect();
        assert_eq!(before, after);
    }
}
