//! Goal tests for managing Concepts in `core` (#17, spec §2.3, §2.7, §9.1, §9.2, §9.4, §9.5,
//! §9.7, §14.1).
//!
//! Assumed API (invented here; the implementer follows it, and callers in tests that are not
//! goal-owned must too). Every operation takes Concept *references* (§9.2: a slug, or a label
//! in the configured language), resolved as `concept::resolve` does:
//!   `concept::show(&Context, &Store, reference: &str) -> Result<concept::Shown, Error>`
//!       read-only. `Shown` has the public fields `id: String`, `label: String`,
//!       `alt_labels: Vec<String>`, `broader: Vec<String>`, `narrower: Vec<String>` (the sorted
//!       ids of the Concepts with a direct `skos:broader <C>` in any graph, `extensions.ttl`
//!       included) and `tagged: usize` (the distinct subjects with `dcterms:subject <C>` in any
//!       graph). Labels are those of the configured language. `Shown` implements `Serialize`.
//!   `concept::rename(&Context, &mut Store, reference: &str, new_label: &str)
//!        -> Result<Mutation<Concept>, Error>`
//!   `concept::ConceptEdit { add_alt, remove_alt, add_broader, remove_broader: Vec<String> }`
//!       (`Clone`, `Debug`, `Default`), and `concept::edit(&Context, &mut Store, reference: &str, &ConceptEdit)
//!        -> Result<Mutation<Concept>, Error>`; labels are normalised as in `concept::add`.
//!   `concept::remove(&Context, &mut Store, reference: &str) -> Result<Mutation<Concept>, Error>`
//!       The `Mutation::value` is the Concept after the change (as it was, for `remove`), in the
//!       configured language, and every change is on the vocabulary graph. `Mutation::changes`
//!       are sorted: adds before removes, then the whole triple string in byte order.
//!   `Error::in_use(concept: &str, tagged: usize, narrower: usize) -> Error`
//!       rule `IN_USE`, exit code 1, message `Concept <id> is in use: <n> tagged, broader of <m>
//!       Concepts; nothing was written`, details `{"concept","tagged","narrower"}`.
//!
//! The tests read tests/fixtures/concepts: animal; mammal (broader animal); dog ("dog"@en,
//! "chien"@fr, altLabels "hound"@en and "toutou"@fr; broader mammal and pet); pet; cat ("cat"@en,
//! "chat"@fr, altLabel "kitty"@en; broader mammal; `pet skos:related cat`); fish; plant; bird.
//! resources.ttl: one Resource tagged dog and fish. extensions.ttl: parrot (a Concept, broader
//! bird) and a `dcterms:subject plant` on the Resource.

mod goal {
    use hali::core::concept::{self, Concept, ConceptEdit};
    use hali::core::{Context, Error, FixedClock, Mutation, Op, SeededIdGen};
    use hali::storage::{EXTENSIONS_GRAPH, File, Store};
    use serde_json::json;
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    const H: &str = "https://w3id.org/hali#";
    const SKOS: &str = "http://www.w3.org/2004/02/skos/core#";
    const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
    const R1: &str = "urn:uuid:0192f0c4-0000-7000-8000-000000000301";

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

    fn fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/concepts")
    }

    fn load() -> Store {
        hali::storage::load(&fixture()).unwrap()
    }

    /// A store made from a vocabulary.ttl text alone.
    fn store_from(vocabulary: &str) -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("vocabulary.ttl"), vocabulary).unwrap();
        (hali::storage::load(dir.path()).unwrap(), dir)
    }

    fn strs(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    /// Every quad in the store, graph included, as text.
    fn all_quads(store: &Store) -> BTreeSet<String> {
        store.iter().map(|q| q.unwrap().to_string()).collect()
    }

    fn vocabulary_triples(store: &Store) -> BTreeSet<String> {
        let text = hali::storage::to_turtle(store, File::Vocabulary);
        oxttl::TurtleParser::new()
            .for_slice(text.as_bytes())
            .map(|t| format!("{} .", t.unwrap()))
            .collect()
    }

    fn changes<T>(m: &Mutation<T>) -> Vec<(Op, String)> {
        m.changes.iter().map(|c| (c.op, c.triple.clone())).collect()
    }

    fn pref(c: &str, label: &str, lang: &str) -> String {
        format!("<{H}{c}> <{SKOS}prefLabel> \"{label}\"@{lang} .")
    }

    fn alt(c: &str, label: &str, lang: &str) -> String {
        format!("<{H}{c}> <{SKOS}altLabel> \"{label}\"@{lang} .")
    }

    fn broader(c: &str, parent: &str) -> String {
        format!("<{H}{c}> <{SKOS}broader> <{H}{parent}> .")
    }

    fn concept(id: &str, label: &str, alts: &[&str], parents: &[&str]) -> Concept {
        Concept {
            id: id.to_string(),
            label: label.to_string(),
            alt_labels: strs(alts),
            broader: strs(parents),
        }
    }

    /// The error of a failed call; the store must be as it was.
    fn refused<T: std::fmt::Debug>(
        store: &Store,
        before: &BTreeSet<String>,
        result: Result<T, Error>,
    ) -> Error {
        let err = result.expect_err("the call should be refused");
        assert_eq!(
            &all_quads(store),
            before,
            "a refused call changed the store"
        );
        err
    }

    fn edit_with(
        add_alt: &[&str],
        remove_alt: &[&str],
        add_broader: &[&str],
        remove_broader: &[&str],
    ) -> ConceptEdit {
        ConceptEdit {
            add_alt: strs(add_alt),
            remove_alt: strs(remove_alt),
            add_broader: strs(add_broader),
            remove_broader: strs(remove_broader),
        }
    }

    // ---- criterion 1: several broader parents; narrower derived, never stored -----------------

    #[test]
    fn edit_add_broader_multiple_parents() {
        let mut store = load();
        // fish gets two parents, one by slug and one by label.
        let m = concept::edit(
            &ctx(),
            &mut store,
            "fish",
            &edit_with(&[], &[], &["animal", "Pet"], &[]),
        )
        .unwrap();
        assert_eq!(m.value, concept("fish", "fish", &[], &["animal", "pet"]));
        assert_eq!(
            changes(&m),
            [
                (Op::Add, broader("fish", "animal")),
                (Op::Add, broader("fish", "pet")),
            ]
        );
        let vocabulary = vocabulary_triples(&store);
        assert!(vocabulary.contains(&broader("fish", "animal")));
        assert!(vocabulary.contains(&broader("fish", "pet")));
    }

    #[test]
    fn edit_remove_broader() {
        let mut store = load();
        let m = concept::edit(
            &ctx(),
            &mut store,
            "dog",
            &edit_with(&[], &[], &[], &["pet"]),
        )
        .unwrap();
        assert_eq!(m.value, concept("dog", "dog", &["hound"], &["mammal"]));
        assert_eq!(changes(&m), [(Op::Remove, broader("dog", "pet"))]);
        assert!(!vocabulary_triples(&store).contains(&broader("dog", "pet")));
        assert!(vocabulary_triples(&store).contains(&broader("dog", "mammal")));
    }

    #[test]
    fn edit_never_stores_narrower() {
        let mut store = load();
        let m = concept::edit(
            &ctx(),
            &mut store,
            "fish",
            &edit_with(&[], &[], &["animal"], &[]),
        )
        .unwrap();
        for c in &m.changes {
            assert!(!c.triple.contains("narrower"), "{}", c.triple);
        }
        for quad in all_quads(&store) {
            assert!(!quad.contains("core#narrower>"), "{quad}");
        }
        // `animal` now has two narrower Concepts, derived from the broader links.
        let shown = concept::show(&ctx(), &store, "animal").unwrap();
        assert_eq!(shown.narrower, ["fish", "mammal"]);
    }

    #[test]
    fn show_narrower_is_derived_including_extensions() {
        let store = load();
        // bird has one narrower Concept, and the link lives only in extensions.ttl.
        let bird = concept::show(&ctx(), &store, "bird").unwrap();
        assert_eq!(bird.narrower, ["parrot"]);
        let mammal = concept::show(&ctx(), &store, "mammal").unwrap();
        assert_eq!(mammal.narrower, ["cat", "dog"]);
        let animal = concept::show(&ctx(), &store, "animal").unwrap();
        assert_eq!(animal.narrower, ["mammal"]);
        let pet = concept::show(&ctx(), &store, "pet").unwrap();
        assert_eq!(pet.narrower, ["dog"]);
        // `skos:related` is not a hierarchy link.
        let cat = concept::show(&ctx(), &store, "cat").unwrap();
        assert!(cat.narrower.is_empty());
        let parrot = concept::show(&ctx(), &store, "parrot").unwrap();
        assert_eq!(parrot.broader, ["bird"]);
    }

    #[test]
    fn edit_broader_cycle_is_allowed() {
        // animal under dog closes the loop dog -> mammal -> animal -> dog; `find` copes (§9.7).
        let mut store = load();
        let m = concept::edit(
            &ctx(),
            &mut store,
            "animal",
            &edit_with(&[], &[], &["dog"], &[]),
        )
        .unwrap();
        assert_eq!(m.value.broader, ["dog"]);
        assert_eq!(changes(&m), [(Op::Add, broader("animal", "dog"))]);
    }

    #[test]
    fn edit_self_broader_is_usage() {
        for reference in ["cat", "Cat", "KITTY"] {
            let mut store = load();
            let before = all_quads(&store);
            let result = concept::edit(
                &ctx(),
                &mut store,
                "cat",
                &edit_with(&[], &[], &[reference], &[]),
            );
            let err = refused(&store, &before, result);
            assert_eq!(err.rule, "USAGE", "{reference}");
            assert_eq!(err.exit_code(), 2, "{reference}");
        }
    }

    #[test]
    fn edit_broader_in_both_is_usage() {
        let mut store = load();
        let before = all_quads(&store);
        // The same Concept however it is referred to.
        let result = concept::edit(
            &ctx(),
            &mut store,
            "cat",
            &edit_with(&[], &[], &["pet"], &["Pet"]),
        );
        let err = refused(&store, &before, result);
        assert_eq!(err.rule, "USAGE");
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn edit_remove_broader_absent_is_noop() {
        let mut store = load();
        let before = all_quads(&store);
        // cat is not under pet.
        let m = concept::edit(
            &ctx(),
            &mut store,
            "cat",
            &edit_with(&[], &[], &[], &["pet"]),
        )
        .unwrap();
        assert!(m.changes.is_empty(), "{:?}", m.changes);
        assert_eq!(m.value, concept("cat", "cat", &["kitty"], &["mammal"]));
        // parrot's link to bird lives only in extensions.ttl: nothing to remove from the
        // vocabulary, and the link stays in the value.
        let m = concept::edit(
            &ctx(),
            &mut store,
            "parrot",
            &edit_with(&[], &[], &[], &["bird"]),
        )
        .unwrap();
        assert!(m.changes.is_empty(), "{:?}", m.changes);
        assert_eq!(m.value.broader, ["bird"]);
        // Adding a link that is already there is a no-op too.
        let m = concept::edit(
            &ctx(),
            &mut store,
            "cat",
            &edit_with(&[], &[], &["mammal"], &[]),
        )
        .unwrap();
        assert!(m.changes.is_empty(), "{:?}", m.changes);
        assert_eq!(all_quads(&store), before);
    }

    // ---- criterion 2: show --------------------------------------------------------------------

    #[test]
    fn show_lists_labels_broader_narrower_and_tagged() {
        let store = load();
        let dog = concept::show(&ctx(), &store, "dog").unwrap();
        assert_eq!(dog.id, "dog");
        assert_eq!(dog.label, "dog");
        assert_eq!(dog.alt_labels, ["hound"]);
        assert_eq!(dog.broader, ["mammal", "pet"]);
        assert!(dog.narrower.is_empty());
        assert_eq!(dog.tagged, 1);

        // Tagged from resources.ttl (fish), from extensions.ttl only (plant), and untagged.
        for (id, tagged) in [("fish", 1), ("plant", 1), ("mammal", 0), ("cat", 0)] {
            let shown = concept::show(&ctx(), &store, id).unwrap();
            assert_eq!(shown.tagged, tagged, "{id}");
        }
        // The value is a plain record in --json.
        let v = serde_json::to_value(&dog).unwrap();
        assert_eq!(
            v,
            json!({"id": "dog", "label": "dog", "alt_labels": ["hound"],
                "broader": ["mammal", "pet"], "narrower": [], "tagged": 1})
        );
    }

    #[test]
    fn show_tagged_counts_distinct_subjects_over_all_graphs() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["vocabulary.ttl", "resources.ttl"] {
            std::fs::copy(fixture().join(name), dir.path().join(name)).unwrap();
        }
        // The Resource tags fish in resources.ttl; here extensions.ttl repeats that tag (one
        // subject, two graphs) and adds a second subject.
        std::fs::write(
            dir.path().join("extensions.ttl"),
            format!(
                "@prefix dcterms: <http://purl.org/dc/terms/> .\n@prefix hali: <{H}> .\n\
                 <{R1}> dcterms:subject hali:fish .\n\
                 <urn:uuid:0192f0c4-0000-7000-8000-000000000302> dcterms:subject hali:fish .\n"
            ),
        )
        .unwrap();
        let store = hali::storage::load(dir.path()).unwrap();
        let fish = concept::show(&ctx(), &store, "fish").unwrap();
        assert_eq!(fish.tagged, 2);
        // Only direct tags count: mammal has none, though dog is under it.
        assert_eq!(concept::show(&ctx(), &store, "mammal").unwrap().tagged, 0);
    }

    #[test]
    fn show_uses_configured_language() {
        let store = load();
        let dog = concept::show(&ctx_in("fr"), &store, "dog").unwrap();
        assert_eq!(dog.id, "dog");
        assert_eq!(dog.label, "chien");
        assert_eq!(dog.alt_labels, ["toutou"]);
        assert_eq!(dog.broader, ["mammal", "pet"]);
        assert_eq!(dog.tagged, 1);
        // A French label resolves in French and is unknown in English.
        assert_eq!(
            concept::show(&ctx_in("fr"), &store, "Chien").unwrap().id,
            "dog"
        );
        let err = concept::show(&ctx(), &store, "chien").unwrap_err();
        assert_eq!(err.rule, "UNKNOWN_CONCEPT");
    }

    #[test]
    fn show_resolves_by_label_and_alt() {
        let store = load();
        assert_eq!(concept::show(&ctx(), &store, "Hound").unwrap().id, "dog");
        assert_eq!(concept::show(&ctx(), &store, "  KITTY ").unwrap().id, "cat");
        assert_eq!(concept::show(&ctx(), &store, "dog").unwrap().id, "dog");
        assert_eq!(
            concept::show(&ctx(), &store, "Mammal").unwrap().id,
            "mammal"
        );
    }

    #[test]
    fn show_unknown_is_unknown_concept() {
        let store = load();
        let err = concept::show(&ctx(), &store, "doge").unwrap_err();
        assert_eq!(err.rule, "UNKNOWN_CONCEPT");
        assert_eq!(err.exit_code(), 1);
        assert_eq!(err.details["concept"], "doge");
        assert_eq!(err.details["suggestions"][0]["id"], "dog");
    }

    // ---- criterion 3: rename ------------------------------------------------------------------

    #[test]
    fn rename_keeps_old_pref_label_as_alt() {
        let mut store = load();
        let m = concept::rename(&ctx(), &mut store, "cat", "feline").unwrap();
        assert_eq!(
            m.value,
            concept("cat", "feline", &["cat", "kitty"], &["mammal"])
        );
        assert_eq!(
            changes(&m),
            [
                (Op::Add, alt("cat", "cat", "en")),
                (Op::Add, pref("cat", "feline", "en")),
                (Op::Remove, pref("cat", "cat", "en")),
            ]
        );
        let vocabulary = vocabulary_triples(&store);
        assert!(vocabulary.contains(&pref("cat", "feline", "en")));
        assert!(vocabulary.contains(&alt("cat", "cat", "en")));
        assert!(!vocabulary.contains(&pref("cat", "cat", "en")));
        // The label is normalised as `concept add` does.
        let m = concept::rename(&ctx(), &mut store, "cat", "  big \t cat ").unwrap();
        assert_eq!(m.value.label, "big cat");
    }

    #[test]
    fn rename_keeps_iri() {
        let mut store = load();
        concept::rename(&ctx(), &mut store, "dog", "Hound Dog Of Wales").unwrap();
        // The slug is minted once, from the first prefLabel: still `dog`, never `hound-dog-of-wales`.
        let vocabulary = vocabulary_triples(&store);
        assert!(vocabulary.contains(&pref("dog", "Hound Dog Of Wales", "en")));
        assert!(
            !vocabulary
                .iter()
                .any(|t| t.contains("#hound-dog-of-wales>")),
            "{vocabulary:?}"
        );
        // The Resource's tag still points at it, and by its new label it resolves.
        let shown = concept::show(&ctx(), &store, "hound dog of wales").unwrap();
        assert_eq!(shown.id, "dog");
        assert_eq!(shown.tagged, 1);
    }

    #[test]
    fn rename_to_own_alt_swaps() {
        let mut store = load();
        let m = concept::rename(&ctx(), &mut store, "dog", "hound").unwrap();
        assert_eq!(
            m.value,
            concept("dog", "hound", &["dog"], &["mammal", "pet"])
        );
        assert_eq!(
            changes(&m),
            [
                (Op::Add, alt("dog", "dog", "en")),
                (Op::Add, pref("dog", "hound", "en")),
                (Op::Remove, alt("dog", "hound", "en")),
                (Op::Remove, pref("dog", "dog", "en")),
            ]
        );
    }

    #[test]
    fn rename_case_only_replaces_without_alt() {
        let mut store = load();
        let m = concept::rename(&ctx(), &mut store, "cat", "Cat").unwrap();
        assert_eq!(m.value, concept("cat", "Cat", &["kitty"], &["mammal"]));
        assert_eq!(
            changes(&m),
            [
                (Op::Add, pref("cat", "Cat", "en")),
                (Op::Remove, pref("cat", "cat", "en")),
            ]
        );
    }

    #[test]
    fn rename_same_label_is_noop() {
        let mut store = load();
        let before = all_quads(&store);
        for label in ["cat", "  cat  "] {
            let m = concept::rename(&ctx(), &mut store, "cat", label).unwrap();
            assert!(m.changes.is_empty(), "{label:?}: {:?}", m.changes);
            assert_eq!(m.value, concept("cat", "cat", &["kitty"], &["mammal"]));
        }
        assert_eq!(all_quads(&store), before);
    }

    #[test]
    fn rename_leaves_other_languages() {
        let mut store = load();
        let m = concept::rename(&ctx(), &mut store, "dog", "pooch").unwrap();
        assert_eq!(
            changes(&m),
            [
                (Op::Add, alt("dog", "dog", "en")),
                (Op::Add, pref("dog", "pooch", "en")),
                (Op::Remove, pref("dog", "dog", "en")),
            ]
        );
        let vocabulary = vocabulary_triples(&store);
        for kept in [pref("dog", "chien", "fr"), alt("dog", "toutou", "fr")] {
            assert!(vocabulary.contains(&kept), "{kept}");
        }

        // In French the French prefLabel is the one that is replaced.
        let m = concept::rename(&ctx_in("fr"), &mut store, "cat", "minou").unwrap();
        assert_eq!(
            changes(&m),
            [
                (Op::Add, alt("cat", "chat", "fr")),
                (Op::Add, pref("cat", "minou", "fr")),
                (Op::Remove, pref("cat", "chat", "fr")),
            ]
        );
        assert!(vocabulary_triples(&store).contains(&pref("cat", "cat", "en")));
    }

    #[test]
    fn rename_unknown_is_unknown_concept() {
        let mut store = load();
        let before = all_quads(&store);
        let result = concept::rename(&ctx(), &mut store, "dogg", "x");
        let err = refused(&store, &before, result);
        assert_eq!(err.rule, "UNKNOWN_CONCEPT");
        assert_eq!(err.exit_code(), 1);
        assert_eq!(err.details["concept"], "dogg");
    }

    // ---- criterion 4: labels are unique per language ------------------------------------------

    #[test]
    fn rename_to_taken_label_is_label_taken() {
        // Another Concept's prefLabel, folded; and its altLabel.
        for (label, holder) in [("Dog", "dog"), ("  MAMMAL ", "mammal"), ("hound", "dog")] {
            let mut store = load();
            let before = all_quads(&store);
            let result = concept::rename(&ctx(), &mut store, "cat", label);
            let err = refused(&store, &before, result);
            assert_eq!(err.rule, "LABEL_TAKEN", "{label}");
            assert_eq!(err.exit_code(), 1);
            assert_eq!(err.details["language"], "en");
            assert_eq!(err.details["concept"], holder, "{label}");
        }
        // A label another Concept holds in another language is free.
        let mut store = load();
        concept::rename(&ctx(), &mut store, "cat", "chien").unwrap();
    }

    #[test]
    fn edit_add_alt_taken_is_label_taken() {
        let mut store = load();
        let before = all_quads(&store);
        // The first taken label in argument order is reported; "fine" is free.
        let result = concept::edit(
            &ctx(),
            &mut store,
            "cat",
            &edit_with(&["fine", "Hound", "dog"], &[], &[], &[]),
        );
        let err = refused(&store, &before, result);
        assert_eq!(err.rule, "LABEL_TAKEN");
        assert_eq!(err.details["label"], "Hound");
        assert_eq!(err.details["language"], "en");
        assert_eq!(err.details["concept"], "dog");

        // The configured language decides: "toutou" is dog's French altLabel.
        let result = concept::edit(
            &ctx_in("fr"),
            &mut store,
            "cat",
            &edit_with(&["toutou"], &[], &[], &[]),
        );
        let err = refused(&store, &before, result);
        assert_eq!(err.rule, "LABEL_TAKEN");
        assert_eq!(err.details["language"], "fr");
        assert_eq!(err.details["concept"], "dog");
    }

    #[test]
    fn edit_add_alt_same_label_other_language_ok() {
        let mut store = load();
        // "chien" is dog's French prefLabel, free in English.
        let m = concept::edit(
            &ctx(),
            &mut store,
            "cat",
            &edit_with(&["chien"], &[], &[], &[]),
        )
        .unwrap();
        assert_eq!(changes(&m), [(Op::Add, alt("cat", "chien", "en"))]);
        assert_eq!(m.value.alt_labels, ["chien", "kitty"]);
    }

    #[test]
    fn edit_add_alt_own_label_is_noop() {
        let mut store = load();
        let before = all_quads(&store);
        // Its own prefLabel and its own altLabel, folded.
        let m = concept::edit(
            &ctx(),
            &mut store,
            "cat",
            &edit_with(&["CAT", "  Kitty "], &[], &[], &[]),
        )
        .unwrap();
        assert!(m.changes.is_empty(), "{:?}", m.changes);
        assert_eq!(m.value, concept("cat", "cat", &["kitty"], &["mammal"]));
        assert_eq!(all_quads(&store), before);
    }

    // ---- criterion 5: edit's alt flags and the order of its checks ----------------------------

    #[test]
    fn edit_add_and_remove_alt() {
        let mut store = load();
        let m = concept::edit(
            &ctx(),
            &mut store,
            "cat",
            &edit_with(&["Moggy", "puss"], &["KITTY"], &[], &[]),
        )
        .unwrap();
        assert_eq!(
            m.value,
            concept("cat", "cat", &["Moggy", "puss"], &["mammal"])
        );
        assert_eq!(
            changes(&m),
            [
                (Op::Add, alt("cat", "Moggy", "en")),
                (Op::Add, alt("cat", "puss", "en")),
                (Op::Remove, alt("cat", "kitty", "en")),
            ]
        );
        // Flags can combine alt and broader changes in one mutation.
        let m = concept::edit(
            &ctx(),
            &mut store,
            "cat",
            &edit_with(&["tom"], &[], &["pet"], &["mammal"]),
        )
        .unwrap();
        assert_eq!(
            changes(&m),
            [
                (Op::Add, alt("cat", "tom", "en")),
                (Op::Add, broader("cat", "pet")),
                (Op::Remove, broader("cat", "mammal")),
            ]
        );
    }

    #[test]
    fn edit_remove_absent_alt_is_noop() {
        let mut store = load();
        let m = concept::edit(
            &ctx(),
            &mut store,
            "cat",
            &edit_with(&[], &["nonesuch"], &[], &[]),
        )
        .unwrap();
        assert!(m.changes.is_empty(), "{:?}", m.changes);
        // An altLabel another Concept holds is not this Concept's to remove.
        let m = concept::edit(
            &ctx(),
            &mut store,
            "cat",
            &edit_with(&[], &["hound"], &[], &[]),
        )
        .unwrap();
        assert!(m.changes.is_empty(), "{:?}", m.changes);
        assert!(vocabulary_triples(&store).contains(&alt("dog", "hound", "en")));
    }

    #[test]
    fn edit_remove_alt_never_touches_pref_label() {
        let mut store = load();
        let before = all_quads(&store);
        // Naming the prefLabel removes nothing: only `rename` changes it. Nor does a label
        // in another language ("chat" is cat's French prefLabel).
        for label in ["cat", "CAT", "chat"] {
            let m = concept::edit(
                &ctx(),
                &mut store,
                "cat",
                &edit_with(&[], &[label], &[], &[]),
            )
            .unwrap();
            assert!(m.changes.is_empty(), "{label}: {:?}", m.changes);
            assert_eq!(m.value.label, "cat");
        }
        assert_eq!(all_quads(&store), before);
    }

    #[test]
    fn edit_alt_in_both_is_usage() {
        let mut store = load();
        let before = all_quads(&store);
        let result = concept::edit(
            &ctx(),
            &mut store,
            "cat",
            &edit_with(&["Foo"], &["  foo "], &[], &[]),
        );
        let err = refused(&store, &before, result);
        assert_eq!(err.rule, "USAGE");
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn edit_no_change_is_usage() {
        let mut store = load();
        let before = all_quads(&store);
        let result = concept::edit(&ctx(), &mut store, "cat", &ConceptEdit::default());
        let err = refused(&store, &before, result);
        assert_eq!(err.rule, "USAGE");
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn edit_unknown_order() {
        let mut store = load();
        let before = all_quads(&store);
        let mut unknown = |reference: &str, edit: ConceptEdit| -> serde_json::Value {
            let result = concept::edit(&ctx(), &mut store, reference, &edit);
            let err = result.expect_err("should be refused");
            assert_eq!(err.rule, "UNKNOWN_CONCEPT", "{reference} {edit:?}");
            assert_eq!(err.exit_code(), 1);
            err.details["concept"].clone()
        };
        // (1) the usage error for one label in both lists is checked before anything is
        // resolved, so the unknown Concept is not reported.
        // (2) <C> is resolved first...
        assert_eq!(
            unknown("ghost", edit_with(&[], &[], &["ghost-a"], &[])),
            "ghost"
        );
        // (3) ...then --add-broader, then --remove-broader, each in argument order.
        assert_eq!(
            unknown(
                "cat",
                edit_with(&[], &[], &["pet", "ghost-b", "ghost-a"], &["ghost-c"])
            ),
            "ghost-b"
        );
        assert_eq!(
            unknown(
                "cat",
                edit_with(&[], &[], &["pet"], &["ghost-c", "ghost-d"])
            ),
            "ghost-c"
        );
        // Unknown references come before the usage errors of step (4).
        assert_eq!(
            unknown("cat", edit_with(&[], &[], &["cat", "ghost-e"], &[])),
            "ghost-e"
        );
        assert_eq!(
            unknown("cat", edit_with(&[], &[], &["pet", "ghost-f"], &["pet"])),
            "ghost-f"
        );
        assert_eq!(all_quads(&store), before);

        // The first usage error (1) beats an unknown Concept; (4) beats LABEL_TAKEN (5).
        let err = concept::edit(
            &ctx(),
            &mut store,
            "ghost",
            &edit_with(&["x"], &["X"], &[], &[]),
        )
        .unwrap_err();
        assert_eq!(err.rule, "USAGE");
        let err = concept::edit(
            &ctx(),
            &mut store,
            "cat",
            &edit_with(&["dog"], &[], &["cat"], &[]),
        )
        .unwrap_err();
        assert_eq!(err.rule, "USAGE");
        // And (5) is reported with the valid parts of the edit unapplied.
        let err = concept::edit(
            &ctx(),
            &mut store,
            "cat",
            &edit_with(&["dog"], &[], &["pet"], &[]),
        )
        .unwrap_err();
        assert_eq!(err.rule, "LABEL_TAKEN");
        assert_eq!(all_quads(&store), before);
    }

    // ---- criterion 6: IN_USE ------------------------------------------------------------------

    #[test]
    fn remove_tagged_is_in_use() {
        let mut store = load();
        let before = all_quads(&store);
        // fish: tagged by the Resource in resources.ttl, narrower of nothing.
        let result = concept::remove(&ctx(), &mut store, "fish");
        let err = refused(&store, &before, result);
        assert_eq!(err.rule, "IN_USE");
        assert_eq!(err.exit_code(), 1);
        assert_eq!(
            *err.details,
            json!({"concept": "fish", "tagged": 1, "narrower": 0})
        );
        // dog too, by its label.
        let result = concept::remove(&ctx(), &mut store, "Dog");
        let err = refused(&store, &before, result);
        assert_eq!(
            *err.details,
            json!({"concept": "dog", "tagged": 1, "narrower": 0})
        );
    }

    #[test]
    fn remove_tagged_in_extensions_is_in_use() {
        let mut store = load();
        let before = all_quads(&store);
        // plant is tagged only from extensions.ttl.
        let result = concept::remove(&ctx(), &mut store, "plant");
        let err = refused(&store, &before, result);
        assert_eq!(err.rule, "IN_USE");
        assert_eq!(
            *err.details,
            json!({"concept": "plant", "tagged": 1, "narrower": 0})
        );
    }

    #[test]
    fn remove_broader_parent_is_in_use() {
        let mut store = load();
        let before = all_quads(&store);
        // mammal: broader of cat and dog; bird: broader of parrot, a link only extensions.ttl
        // holds; animal and pet: one each.
        for (id, narrower) in [("mammal", 2), ("bird", 1), ("animal", 1), ("pet", 1)] {
            let result = concept::remove(&ctx(), &mut store, id);
            let err = refused(&store, &before, result);
            assert_eq!(err.rule, "IN_USE", "{id}");
            assert_eq!(
                *err.details,
                json!({"concept": id, "tagged": 0, "narrower": narrower}),
                "{id}"
            );
        }
    }

    #[test]
    fn remove_in_use_before_shacl() {
        // Removing fish would leave the Resource's tag pointing at no Concept, a SHACL
        // violation; IN_USE is what is reported, as it is checked first.
        let mut store = load();
        let err = concept::remove(&ctx(), &mut store, "fish").unwrap_err();
        assert_eq!(err.rule, "IN_USE");
        assert_ne!(err.rule, "SHACL_VIOLATION");
    }

    #[test]
    fn in_use_error() {
        let err = Error::in_use("fish", 1, 0);
        assert_eq!(err.rule, "IN_USE");
        assert_eq!(err.exit_code(), 1);
        assert_eq!(
            err.message,
            "Concept fish is in use: 1 tagged, broader of 0 Concepts; nothing was written"
        );
        assert_eq!(
            *err.details,
            json!({"concept": "fish", "tagged": 1, "narrower": 0})
        );
        assert_eq!(
            err.suggested_fix.as_deref(),
            Some(
                "Untag the Resources (hali edit <ref> --remove-tag fish) and unlink the narrower \
                 Concepts (hali concept edit <C> --remove-broader fish), then retry."
            )
        );
        // Tagged: find the Resources. Only a parent: look at the Concept.
        assert_eq!(
            err.suggested_command.as_deref(),
            Some("hali find --exact --tag fish")
        );
        let err = Error::in_use("mammal", 0, 2);
        assert_eq!(
            err.message,
            "Concept mammal is in use: 0 tagged, broader of 2 Concepts; nothing was written"
        );
        assert_eq!(
            err.suggested_command.as_deref(),
            Some("hali concept show mammal")
        );
        let err = Error::in_use("pet", 3, 1);
        assert_eq!(
            err.suggested_command.as_deref(),
            Some("hali find --exact --tag pet")
        );
    }

    // ---- criterion 7: remove removes the right triples and nothing else -----------------------

    /// Everything a removal of cat takes, in the order `changes` lists it: the subject's
    /// triples (`rdf:type` before the `skos:` ones, labels by predicate then value), then the
    /// one triple that points at it.
    fn cat_removals() -> Vec<String> {
        vec![
            format!("<{H}cat> <{RDF_TYPE}> <{SKOS}Concept> ."),
            alt("cat", "kitty", "en"),
            broader("cat", "mammal"),
            format!("<{H}cat> <{SKOS}inScheme> <{H}topics> ."),
            pref("cat", "cat", "en"),
            pref("cat", "chat", "fr"),
            format!("<{H}pet> <{SKOS}related> <{H}cat> ."),
        ]
    }

    #[test]
    fn remove_takes_concept_triples_from_vocabulary_only() {
        let mut store = load();
        let quads_before = all_quads(&store);
        let before = vocabulary_triples(&store);
        let m = concept::remove(&ctx(), &mut store, "cat").unwrap();
        assert_eq!(m.value, concept("cat", "cat", &["kitty"], &["mammal"]));

        // Every language, the outgoing broader link and the inbound `pet skos:related cat`.
        let after = vocabulary_triples(&store);
        let removed: BTreeSet<String> = before.difference(&after).cloned().collect();
        assert_eq!(removed, cat_removals().into_iter().collect::<BTreeSet<_>>());
        assert!(after.is_subset(&before), "remove added a triple");
        assert!(
            !after.iter().any(|t| t.contains("#cat>")),
            "something still mentions cat"
        );
        // The scheme and the other Concepts are intact.
        assert!(after.contains(&format!("<{H}topics> <{RDF_TYPE}> <{SKOS}ConceptScheme> .")));
        for kept in ["animal", "bird", "dog", "fish", "mammal", "pet", "plant"] {
            assert!(
                after.contains(&format!("<{H}{kept}> <{RDF_TYPE}> <{SKOS}Concept> .")),
                "{kept}"
            );
        }
        assert!(after.contains(&broader("dog", "mammal")));

        // Only quads of the vocabulary graph went; resources and extensions keep every quad
        // (parrot, plant's tag, the Resource).
        let quads_after = all_quads(&store);
        for gone in quads_before.difference(&quads_after) {
            assert!(gone.contains("graph/vocabulary"), "{gone}");
        }
        assert!(quads_after.is_subset(&quads_before));
        assert!(
            quads_after
                .iter()
                .any(|q| q.contains("#parrot>") && q.contains(EXTENSIONS_GRAPH))
        );
        assert!(!quads_after.iter().any(|q| q.contains("#cat>")));

        // Where it was resolved by a French label, the same triples go.
        let mut store = load();
        let m = concept::remove(&ctx_in("fr"), &mut store, "chat").unwrap();
        assert_eq!(m.value.id, "cat");
        assert_eq!(m.value.label, "chat");
        assert_eq!(m.value.alt_labels, Vec::<String>::new());
        assert_eq!(
            m.changes
                .iter()
                .map(|c| c.triple.clone())
                .collect::<Vec<_>>(),
            cat_removals()
        );
    }

    #[test]
    fn remove_last_concept_keeps_scheme() {
        let (mut store, _dir) = store_from(&format!(
            "@prefix hali: <{H}> .\n@prefix skos: <{SKOS}> .\n\
             hali:topics a skos:ConceptScheme .\n\
             hali:solo a skos:Concept ; skos:inScheme hali:topics ; skos:prefLabel \"solo\"@en .\n"
        ));
        let m = concept::remove(&ctx(), &mut store, "solo").unwrap();
        assert_eq!(m.value, concept("solo", "solo", &[], &[]));
        assert_eq!(
            changes(&m),
            [
                (
                    Op::Remove,
                    format!("<{H}solo> <{RDF_TYPE}> <{SKOS}Concept> .")
                ),
                (
                    Op::Remove,
                    format!("<{H}solo> <{SKOS}inScheme> <{H}topics> .")
                ),
                (Op::Remove, pref("solo", "solo", "en")),
            ]
        );
        // The scheme node stays: `concept add` would otherwise have to write it again.
        let vocabulary = vocabulary_triples(&store);
        assert_eq!(
            vocabulary.into_iter().collect::<Vec<_>>(),
            [format!("<{H}topics> <{RDF_TYPE}> <{SKOS}ConceptScheme> .")]
        );
    }

    #[test]
    fn remove_changes_are_sorted_removes() {
        let mut store = load();
        let m = concept::remove(&ctx(), &mut store, "cat").unwrap();
        assert!(m.changes.iter().all(|c| c.op == Op::Remove));
        let listed: Vec<&str> = m.changes.iter().map(|c| c.triple.as_str()).collect();
        assert_eq!(listed, cat_removals());
        let mut sorted = listed.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(listed, sorted, "sorted, and no triple listed twice");
    }

    #[test]
    fn remove_extensions_only_concept_changes_nothing() {
        // parrot exists only in extensions.ttl: there is nothing to remove from the
        // vocabulary, and hali never writes extensions.ttl.
        let mut store = load();
        let before = all_quads(&store);
        let m = concept::remove(&ctx(), &mut store, "parrot").unwrap();
        assert!(m.changes.is_empty(), "{:?}", m.changes);
        assert_eq!(m.value, concept("parrot", "parrot", &[], &["bird"]));
        assert_eq!(all_quads(&store), before);
    }

    #[test]
    fn remove_unknown_is_unknown_concept() {
        let mut store = load();
        let before = all_quads(&store);
        let result = concept::remove(&ctx(), &mut store, "dogg");
        let err = refused(&store, &before, result);
        assert_eq!(err.rule, "UNKNOWN_CONCEPT");
        assert_eq!(err.exit_code(), 1);
        assert_eq!(err.details["concept"], "dogg");
    }
}
