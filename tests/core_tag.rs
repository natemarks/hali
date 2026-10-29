//! Goal tests for tagging Resources with Concepts in `core` (#16, spec §2.3, §2.7, §9.2, §9.7).
//!
//! Assumed API (invented here; every call that changes a signature or adds one is listed, and
//! callers in tests that are not goal-owned must follow):
//!   `concept::resolve(&Context, &Store, reference: &str) -> Result<String, Error>`
//!       the id (slug) of the Concept a reference names: first by exact slug, then by any
//!       prefLabel or altLabel in the configured language, whitespace-normalised and
//!       case-insensitive; otherwise `UNKNOWN_CONCEPT`.
//!   `concept::add(&Context, &mut Store, label: &str, broader: &[String])
//!        -> Result<Mutation<Concept>, Error>`
//!       `broader` are Concept references, resolved as above; `Concept::broader` lists the
//!       sorted ids; an unknown reference is `UNKNOWN_CONCEPT` and the store is unchanged.
//!   `resource::add_all(&Context, &mut Store, &[FileFacts], title: Option<&str>,
//!        types: &[String], tags: &[String], user_name: &str) -> Result<Mutation<AddedAll>, Error>`
//!       `tags` apply to newly cataloged Resources only; every reference is resolved before
//!       anything changes.
//!   `resource::Edit { title: Option<String>, add_tags: Vec<String>, remove_tags: Vec<String> }`
//!       (`Default`), and `resource::edit(&Context, &mut Store, id: &str, &Edit)
//!        -> Result<Mutation<Record>, Error>`: one mutation, one SHACL guard, and
//!       `dcterms:modified` bumped only on change. No change requested, or the same Concept in
//!       add and remove (however referred to), is a usage error (`exit_code() == 2`).
//!   `resource::Query { tags: Vec<String>, exact: bool, .. }` and
//!       `resource::find(&Context, &Store, &Query, stat) -> Result<Found, Error>`
//!       `tags` are Concept references (AND); an unknown one is `UNKNOWN_CONCEPT`.
//!   `Error::exit_code() -> u8`, `Error { rule, details: Box<Value>, suggested_command, .. }`.
//!
//! The tests that read an existing store use tests/fixtures/store (Concepts `ml`,
//! `linked-data` (altLabels "LD", "linkeddata"; broader `semantic-web`), `semantic-web`,
//! `a-first` (label "Zulu"); one Resource tagged `linked-data` and `ml`) and
//! tests/fixtures/tags (see the comment in its vocabulary.ttl).

mod goal {
    use hali::core::resource::{self, Edit, FileFacts, Query};
    use hali::core::{Context, FixedClock, Op, SeededIdGen, concept};
    use hali::storage::{File, Store};
    use serde_json::json;
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    const EXISTING: &str = "0192f0c4-7b1e-7cc3-9a43-6c1d2e8f4b10";
    const EXISTING_PATH: &str = "/home/nmarks/papers/linked-data-patterns.pdf";
    const CATALOG_RECORD: &str = "urn:uuid:0192f0c4-7b20-7a01-8d2e-1f0b9c3a5e77";
    const NOW: &str = "2026-10-24T09:30:00Z";
    const SHA: &str = "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03";
    const SUBJECT: &str = "http://purl.org/dc/terms/subject";
    const MODIFIED: &str = "http://purl.org/dc/terms/modified";
    const BROADER: &str = "http://www.w3.org/2004/02/skos/core#broader";
    const HALI: &str = "https://w3id.org/hali#";

    fn ctx_in(language: &str) -> Context {
        Context::new(
            language,
            Box::new(FixedClock::new(NOW)),
            Box::new(SeededIdGen::new(1)),
        )
    }

    fn ctx() -> Context {
        ctx_in("en")
    }

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    fn load(name: &str) -> Store {
        hali::storage::load(&fixture(name)).unwrap()
    }

    /// A store made from a vocabulary.ttl text alone.
    fn store_from(vocabulary: &str) -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("vocabulary.ttl"), vocabulary).unwrap();
        (hali::storage::load(dir.path()).unwrap(), dir)
    }

    fn facts(path: &str) -> FileFacts {
        FileFacts {
            path: path.to_string(),
            byte_size: 6,
            modified: "2026-10-20T14:02:11Z".to_string(),
            sha256: SHA.to_string(),
            media_type: "text/plain".to_string(),
        }
    }

    fn strings(refs: &[&str]) -> Vec<String> {
        refs.iter().map(|r| r.to_string()).collect()
    }

    fn resources_ttl(store: &Store) -> String {
        hali::storage::to_turtle(store, File::Resources)
    }

    fn vocabulary_ttl(store: &Store) -> String {
        hali::storage::to_turtle(store, File::Vocabulary)
    }

    fn tag_ids(record: &resource::Record) -> Vec<&str> {
        record.tags.iter().map(|t| t.id.as_str()).collect()
    }

    fn triple(s: &str, p: &str, o: &str) -> String {
        format!("<{s}> <{p}> <{o}> .")
    }

    fn change_set(changes: &[hali::core::Change]) -> BTreeSet<(Op, String)> {
        changes.iter().map(|c| (c.op, c.triple.clone())).collect()
    }

    fn unknown(err: &hali::core::Error) -> &serde_json::Value {
        assert_eq!(err.rule, "UNKNOWN_CONCEPT");
        assert_eq!(err.exit_code(), 1);
        &err.details
    }

    // ---- criterion 1: tags stored as dcterms:subject --------------------------------------

    #[test]
    fn add_with_tags_writes_dcterms_subject() {
        let mut store = load("store");
        let added = resource::add_all(
            &ctx(),
            &mut store,
            &[facts("/a/new.txt")],
            Some("new"),
            &[],
            &strings(&["ml", "Linked  DATA"]),
            "nate",
        )
        .unwrap();
        let record = &added.value.added[0];
        assert_eq!(tag_ids(record), ["linked-data", "ml"]);
        assert_eq!(record.tags[0].label, "linked data");
        assert_eq!(record.tags[1].label, "machine learning");

        let subject = format!("urn:uuid:{}", record.id);
        for concept in ["ml", "linked-data"] {
            let want = triple(&subject, SUBJECT, &format!("{HALI}{concept}"));
            assert!(
                added
                    .changes
                    .iter()
                    .any(|c| c.op == Op::Add && c.triple == want),
                "no add change {want}: {:?}",
                added.changes
            );
            assert!(
                resources_ttl(&store).contains(&subject),
                "the Resource is stored in resources.ttl"
            );
        }
    }

    #[test]
    fn add_tags_are_ignored_for_an_already_cataloged_path() {
        let mut store = load("store");
        let before = resources_ttl(&store);
        let added = resource::add_all(
            &ctx(),
            &mut store,
            &[facts(EXISTING_PATH)],
            None,
            &[],
            &strings(&["semantic-web"]),
            "nate",
        )
        .unwrap();
        assert!(added.value.added.is_empty());
        assert_eq!(added.value.existing.len(), 1);
        assert_eq!(tag_ids(&added.value.existing[0]), ["linked-data", "ml"]);
        assert!(added.changes.is_empty());
        assert_eq!(resources_ttl(&store), before);
    }

    #[test]
    fn add_resolves_every_tag_before_it_changes_anything() {
        let mut store = load("store");
        let before = resources_ttl(&store);
        let err = resource::add_all(
            &ctx(),
            &mut store,
            &[facts("/a/one.txt"), facts("/a/two.txt")],
            None,
            &[],
            &strings(&["ml", "nope"]),
            "nate",
        )
        .unwrap_err();
        assert_eq!(unknown(&err)["concept"], "nope");
        assert_eq!(resources_ttl(&store), before, "nothing was cataloged");
    }

    #[test]
    fn edit_adds_and_removes_tags() {
        let mut store = load("store");
        let edit = Edit {
            add_tags: strings(&["Semantic Web"]),
            remove_tags: strings(&["ml"]),
            ..Edit::default()
        };
        let edited = resource::edit(&ctx(), &mut store, EXISTING, &edit).unwrap();
        assert_eq!(tag_ids(&edited.value), ["linked-data", "semantic-web"]);

        let subject = format!("urn:uuid:{EXISTING}");
        let want: BTreeSet<(Op, String)> = [
            (
                Op::Add,
                triple(&subject, SUBJECT, &format!("{HALI}semantic-web")),
            ),
            (
                Op::Add,
                format!(
                    "<{CATALOG_RECORD}> <{MODIFIED}> \"{NOW}\"^^<http://www.w3.org/2001/XMLSchema#dateTime> ."
                ),
            ),
            (Op::Remove, triple(&subject, SUBJECT, &format!("{HALI}ml"))),
            (
                Op::Remove,
                format!(
                    "<{CATALOG_RECORD}> <{MODIFIED}> \"2026-10-21T08:00:00Z\"^^<http://www.w3.org/2001/XMLSchema#dateTime> ."
                ),
            ),
        ]
        .into_iter()
        .collect();
        assert_eq!(change_set(&edited.changes), want);
        assert_eq!(edited.changes.len(), 4, "no duplicates");
    }

    #[test]
    fn edit_title_and_tags_are_one_mutation() {
        let mut store = load("store");
        let edit = Edit {
            title: Some("Renamed".to_string()),
            add_tags: strings(&["semantic-web"]),
            ..Edit::default()
        };
        let edited = resource::edit(&ctx(), &mut store, EXISTING, &edit).unwrap();
        assert_eq!(edited.value.title, "Renamed");
        assert_eq!(
            tag_ids(&edited.value),
            ["linked-data", "ml", "semantic-web"]
        );
        // title out and in, subject in, modified out and in: one bump.
        assert_eq!(edited.changes.len(), 5, "{:?}", edited.changes);
        let bumps = edited
            .changes
            .iter()
            .filter(|c| c.op == Op::Add && c.triple.contains(MODIFIED))
            .count();
        assert_eq!(bumps, 1);
    }

    #[test]
    fn edit_tag_noops_have_no_changes() {
        let mut store = load("store");
        let before = resources_ttl(&store);
        // `linked-data` is present (also by altLabel "LD"); `semantic-web` is absent.
        let edit = Edit {
            add_tags: strings(&["linked-data", "LD"]),
            remove_tags: strings(&["semantic-web"]),
            ..Edit::default()
        };
        let edited = resource::edit(&ctx(), &mut store, EXISTING, &edit).unwrap();
        assert!(edited.changes.is_empty(), "{:?}", edited.changes);
        assert_eq!(tag_ids(&edited.value), ["linked-data", "ml"]);
        assert_eq!(resources_ttl(&store), before, "modified is not bumped");

        // The same title with a present tag is a no-op too.
        let edit = Edit {
            title: Some("Linked Data Patterns".to_string()),
            add_tags: strings(&["ml"]),
            ..Edit::default()
        };
        let edited = resource::edit(&ctx(), &mut store, EXISTING, &edit).unwrap();
        assert!(edited.changes.is_empty(), "{:?}", edited.changes);
    }

    #[test]
    fn edit_requires_some_change() {
        let mut store = load("store");
        let before = resources_ttl(&store);
        let err = resource::edit(&ctx(), &mut store, EXISTING, &Edit::default()).unwrap_err();
        assert_eq!(err.exit_code(), 2, "a usage error");
        assert_eq!(resources_ttl(&store), before);

        // The same Concept in add and remove is a usage error, by slug or by label.
        for (add, remove) in [
            ("ml", "ml"),
            ("ml", "Machine Learning"),
            ("LD", "linked-data"),
        ] {
            let edit = Edit {
                add_tags: strings(&[add]),
                remove_tags: strings(&[remove]),
                ..Edit::default()
            };
            let err = resource::edit(&ctx(), &mut store, EXISTING, &edit).unwrap_err();
            assert_eq!(err.exit_code(), 2, "{add} / {remove}");
            assert_eq!(resources_ttl(&store), before, "{add} / {remove}");
        }
    }

    #[test]
    fn edit_with_an_unknown_tag_changes_nothing() {
        let mut store = load("store");
        let before = resources_ttl(&store);
        let edit = Edit {
            title: Some("Renamed".to_string()),
            add_tags: strings(&["semantic-web", "nope-one", "nope-two"]),
            ..Edit::default()
        };
        let err = resource::edit(&ctx(), &mut store, EXISTING, &edit).unwrap_err();
        assert_eq!(unknown(&err)["concept"], "nope-one", "the first, in order");
        assert_eq!(resources_ttl(&store), before);

        let edit = Edit {
            remove_tags: strings(&["nope"]),
            ..Edit::default()
        };
        let err = resource::edit(&ctx(), &mut store, EXISTING, &edit).unwrap_err();
        assert_eq!(unknown(&err)["concept"], "nope");
        assert_eq!(resources_ttl(&store), before);
    }

    // ---- criterion 2: UNKNOWN_CONCEPT and concept references ------------------------------

    #[test]
    fn unknown_concept_suggests_closest() {
        let store = load("store");
        let err = concept::resolve(&ctx(), &store, "machin learning").unwrap_err();
        let d = unknown(&err);
        assert_eq!(d["concept"], "machin learning");
        assert_eq!(
            d["suggestions"],
            json!([{"id": "ml", "label": "machine learning"}])
        );
        assert_eq!(
            err.suggested_command.as_deref(),
            Some("hali --dry-run concept add 'machin learning'")
        );

        // An altLabel containing the reference suggests its Concept, by its prefLabel.
        let err = concept::resolve(&ctx(), &store, "linkeddat").unwrap_err();
        assert_eq!(
            unknown(&err)["suggestions"],
            json!([{"id": "linked-data", "label": "linked data"}])
        );

        // Nothing is close: an empty list, not a missing key.
        let err = concept::resolve(&ctx(), &store, "zzzzzzzz").unwrap_err();
        assert_eq!(unknown(&err)["suggestions"], json!([]));
        assert_eq!(
            err.suggested_command.as_deref(),
            Some("hali --dry-run concept add zzzzzzzz")
        );
    }

    #[test]
    fn unknown_concept_ranks_and_caps_suggestions() {
        let store = load("tags");
        // Within edit distance, the one Concept.
        let err = concept::resolve(&ctx(), &store, "mamal").unwrap_err();
        assert_eq!(
            unknown(&err)["suggestions"],
            json!([{"id": "mammal", "label": "mammal"}])
        );
        // Contained in a label: equally far, so by label, then id.
        let err = concept::resolve(&ctx(), &store, "mal").unwrap_err();
        assert_eq!(
            unknown(&err)["suggestions"],
            json!([{"id": "animal", "label": "animal"}, {"id": "mammal", "label": "mammal"}])
        );
        // The case of the reference does not matter for suggestions.
        let err = concept::resolve(&ctx(), &store, "MAL").unwrap_err();
        assert_eq!(unknown(&err)["suggestions"].as_array().unwrap().len(), 2);
        // At most three, however many contain the reference ("a" is in four labels).
        let err = concept::resolve(&ctx(), &store, "a").unwrap_err();
        let suggestions = unknown(&err)["suggestions"].as_array().unwrap().clone();
        assert_eq!(suggestions.len(), 3, "{suggestions:?}");
        for s in &suggestions {
            let id = s["id"].as_str().unwrap();
            assert!(
                ["animal", "mammal", "plant", "loop-a", "loop-b"].contains(&id),
                "{id}"
            );
        }
    }

    #[test]
    fn unknown_concept_suggested_command_is_quoted() {
        let store = load("store");
        let command = |reference: &str| {
            concept::resolve(&ctx(), &store, reference)
                .unwrap_err()
                .suggested_command
                .unwrap()
        };
        // Safe characters are left alone.
        assert_eq!(command("a.b_c-d9"), "hali --dry-run concept add a.b_c-d9");
        // Anything else is POSIX single-quoted; a quote inside is `'\''`.
        assert_eq!(
            command("deep learning"),
            "hali --dry-run concept add 'deep learning'"
        );
        assert_eq!(command("it's"), "hali --dry-run concept add 'it'\\''s'");
        assert_eq!(
            command("$(rm -rf ~); `x`"),
            "hali --dry-run concept add '$(rm -rf ~); `x`'"
        );
        assert_eq!(command("café"), "hali --dry-run concept add 'café'");
        // The details echo the reference as given.
        let err = concept::resolve(&ctx(), &store, "it's").unwrap_err();
        assert_eq!(unknown(&err)["concept"], "it's");
    }

    #[test]
    fn leading_dash_reference_gets_a_double_dash_in_the_suggested_command() {
        let store = load("store");
        let command = |reference: &str| {
            concept::resolve(&ctx(), &store, reference)
                .unwrap_err()
                .suggested_command
                .unwrap()
        };
        assert_eq!(command("-x"), "hali --dry-run concept add -- -x");
        assert_eq!(
            command("-1 rated"),
            "hali --dry-run concept add -- '-1 rated'"
        );
        assert_eq!(command("a-x"), "hali --dry-run concept add a-x");
    }

    #[test]
    fn empty_or_blank_reference_is_a_usage_error() {
        let store = load("store");
        for reference in ["", "  ", "\t"] {
            let err = concept::resolve(&ctx(), &store, reference).unwrap_err();
            assert_eq!(err.rule, "USAGE", "{reference:?}");
            assert_eq!(err.exit_code(), 2, "{reference:?}");
            assert!(err.message.contains("empty"), "{}", err.message);
        }
    }

    #[test]
    fn ref_slug_wins_over_label() {
        // `rust` is the slug of one Concept and the label of another.
        let (store, _dir) = store_from(
            "@prefix hali: <https://w3id.org/hali#> .\n\
             @prefix skos: <http://www.w3.org/2004/02/skos/core#> .\n\
             hali:topics a skos:ConceptScheme .\n\
             hali:rust a skos:Concept ; skos:inScheme hali:topics ; skos:prefLabel \"Language\"@en .\n\
             hali:lang a skos:Concept ; skos:inScheme hali:topics ; skos:prefLabel \"rust\"@en .\n",
        );
        assert_eq!(concept::resolve(&ctx(), &store, "rust").unwrap(), "rust");
        assert_eq!(
            concept::resolve(&ctx(), &store, "Language").unwrap(),
            "rust"
        );
        // A slug match is exact: another case falls through to the label.
        assert_eq!(concept::resolve(&ctx(), &store, "RUST").unwrap(), "lang");
        assert_eq!(concept::resolve(&ctx(), &store, "lang").unwrap(), "lang");
    }

    #[test]
    fn ref_label_case_and_whitespace_insensitive() {
        let store = load("store");
        let resolve = |r: &str| concept::resolve(&ctx(), &store, r).unwrap();
        assert_eq!(resolve("ml"), "ml");
        assert_eq!(resolve("  Machine   LEARNING "), "ml");
        assert_eq!(resolve("machine\tlearning"), "ml");
        assert_eq!(resolve("ld"), "linked-data", "an altLabel");
        assert_eq!(resolve("LINKEDDATA"), "linked-data", "an altLabel");
        assert_eq!(resolve("linked data"), "linked-data");
        assert_eq!(resolve("ZULU"), "a-first");
        assert_eq!(resolve("a-first"), "a-first");
    }

    #[test]
    fn ref_other_language_label_not_matched() {
        let (store, _dir) = store_from(
            "@prefix hali: <https://w3id.org/hali#> .\n\
             @prefix skos: <http://www.w3.org/2004/02/skos/core#> .\n\
             hali:topics a skos:ConceptScheme .\n\
             hali:ml a skos:Concept ; skos:inScheme hali:topics ;\n\
             \tskos:prefLabel \"machine learning\"@en , \"maschinelles Lernen\"@de .\n",
        );
        assert_eq!(
            concept::resolve(&ctx_in("en"), &store, "machine learning").unwrap(),
            "ml"
        );
        let err = concept::resolve(&ctx_in("en"), &store, "Maschinelles Lernen").unwrap_err();
        assert_eq!(unknown(&err)["concept"], "Maschinelles Lernen");
        assert_eq!(
            concept::resolve(&ctx_in("de"), &store, "Maschinelles  Lernen").unwrap(),
            "ml"
        );
        assert!(concept::resolve(&ctx_in("de"), &store, "machine learning").is_err());
    }

    // ---- criterion 3: concept add --broader, broader-path find -----------------------------

    #[test]
    fn add_with_broader() {
        let mut store = load("store");
        let added = concept::add(
            &ctx(),
            &mut store,
            "deep learning",
            &strings(&["ml", "Linked Data", "machine learning"]),
        )
        .unwrap();
        assert_eq!(added.value.id, "deep-learning");
        assert_eq!(
            added.value.broader,
            ["linked-data", "ml"],
            "sorted, no repeats"
        );
        for parent in ["ml", "linked-data"] {
            let want = triple(
                &format!("{HALI}deep-learning"),
                BROADER,
                &format!("{HALI}{parent}"),
            );
            assert!(
                added
                    .changes
                    .iter()
                    .any(|c| c.op == Op::Add && c.triple == want),
                "no add change {want}: {:?}",
                added.changes
            );
        }
        assert!(
            vocabulary_ttl(&store).contains("skos:broader hali:linked-data , hali:ml"),
            "{}",
            vocabulary_ttl(&store)
        );
        // `list` reports them.
        let page = concept::list(&ctx(), &store, 0, 50);
        let listed = page
            .results
            .iter()
            .find(|c| c.id == "deep-learning")
            .unwrap();
        assert_eq!(listed.broader, ["linked-data", "ml"]);
    }

    #[test]
    fn add_with_unknown_broader_changes_nothing() {
        let mut store = load("store");
        let before = vocabulary_ttl(&store);
        let err = concept::add(
            &ctx(),
            &mut store,
            "deep learning",
            &strings(&["nope-one", "ml", "nope-two"]),
        )
        .unwrap_err();
        let d = unknown(&err);
        assert_eq!(d["concept"], "nope-one", "the first, in argument order");
        assert_eq!(vocabulary_ttl(&store), before);
        assert_eq!(
            err.suggested_command.as_deref(),
            Some("hali --dry-run concept add nope-one")
        );
    }

    #[test]
    fn tag_matches_narrower() {
        let store = load("tags");
        let find = |tags: &[&str], exact: bool| -> Vec<String> {
            let query = Query {
                tags: strings(tags),
                exact,
                ..Query::default()
            };
            let found = resource::find(&ctx(), &store, &query, |_| None).unwrap();
            assert_eq!(found.total, found.results.len(), "{tags:?}");
            let mut titles: Vec<String> = found.results.into_iter().map(|r| r.title).collect();
            titles.sort();
            titles
        };

        // A chain: animal <- mammal <- dog. Second parent: dog -> pet. The bird's `broader`
        // triple is in extensions.ttl only. A Resource with two tags is listed once.
        assert_eq!(
            find(&["animal"], false),
            [
                "animal doc",
                "bird doc",
                "dog doc",
                "dog plant doc",
                "mammal doc"
            ]
        );
        assert_eq!(
            find(&["mammal"], false),
            ["dog doc", "dog plant doc", "mammal doc"]
        );
        assert_eq!(
            find(&["pet"], false),
            ["dog doc", "dog plant doc", "pet doc"]
        );
        assert_eq!(find(&["dog"], false), ["dog doc", "dog plant doc"]);
        assert_eq!(find(&["bird"], false), ["bird doc"]);
        assert_eq!(find(&["plant"], false), ["dog plant doc", "plant doc"]);

        // A cycle terminates, and reaches both ends.
        assert_eq!(find(&["loop-a"], false), ["loop doc"]);
        assert_eq!(find(&["loop-b"], false), ["loop doc"]);
        assert!(find(&["loop-b"], true).is_empty());

        // --exact: the asserted subject only.
        assert_eq!(find(&["animal"], true), ["animal doc"]);
        assert_eq!(find(&["dog"], true), ["dog doc", "dog plant doc"]);
        assert_eq!(find(&["pet"], true), ["pet doc"]);
        assert!(find(&["mammal", "pet"], true).is_empty());

        // Repeated tags AND.
        assert_eq!(
            find(&["mammal", "pet"], false),
            ["dog doc", "dog plant doc"]
        );
        assert_eq!(find(&["animal", "plant"], false), ["dog plant doc"]);
        assert!(find(&["plant", "bird"], false).is_empty());
        assert!(find(&["animal", "plant"], true).is_empty());

        // References resolve as everywhere: by label, whitespace and case.
        assert_eq!(find(&["  LOOP   a "], false), ["loop doc"]);

        // --exact without a tag does nothing; no tags is every Resource.
        assert_eq!(find(&[], true).len(), 9);
        assert_eq!(find(&[], false).len(), 9);
    }

    #[test]
    fn find_pages_a_tag_match() {
        let store = load("tags");
        let query = Query {
            tags: strings(&["animal"]),
            limit: 2,
            offset: 1,
            ..Query::default()
        };
        let found = resource::find(&ctx(), &store, &query, |_| None).unwrap();
        assert_eq!(found.total, 5);
        assert_eq!(found.results.len(), 2);
        assert!(found.truncated);
        assert_eq!(found.next_offset, Some(3));
    }

    #[test]
    fn find_with_an_unknown_tag_is_an_error_not_an_empty_result() {
        let store = load("tags");
        let query = Query {
            tags: strings(&["animal", "nope"]),
            ..Query::default()
        };
        let err = resource::find(&ctx(), &store, &query, |_| None).unwrap_err();
        assert_eq!(unknown(&err)["concept"], "nope");
        // Even with --exact, and when the Concept would have matched nothing anyway.
        let query = Query {
            tags: strings(&["nope"]),
            exact: true,
            ..Query::default()
        };
        assert!(resource::find(&ctx(), &store, &query, |_| None).is_err());
    }

    // ---- criterion 4: records include tags: [{id,label}] ------------------------------------

    #[test]
    fn record_tags_sorted_by_label_with_pref_label() {
        // Sorted by label, not id; the label is the prefLabel in the configured language.
        let (mut store, _dir) = store_from(
            "@prefix hali: <https://w3id.org/hali#> .\n\
             @prefix skos: <http://www.w3.org/2004/02/skos/core#> .\n\
             hali:topics a skos:ConceptScheme .\n\
             hali:zeta a skos:Concept ; skos:inScheme hali:topics ;\n\
             \tskos:prefLabel \"Alpha\"@en , \"Zeta\"@de .\n\
             hali:alpha a skos:Concept ; skos:inScheme hali:topics ;\n\
             \tskos:prefLabel \"Zed\"@en ; skos:altLabel \"Aardvark\"@en .\n",
        );
        let added = resource::add_all(
            &ctx(),
            &mut store,
            &[facts("/a/new.txt")],
            None,
            &[],
            &strings(&["alpha", "zeta"]),
            "nate",
        )
        .unwrap();
        let record = &added.value.added[0];
        let got: Vec<(&str, &str)> = record
            .tags
            .iter()
            .map(|t| (t.id.as_str(), t.label.as_str()))
            .collect();
        assert_eq!(got, [("zeta", "Alpha"), ("alpha", "Zed")]);

        let shown = resource::show(&ctx_in("de"), &store, &record.id).unwrap();
        let zeta = shown.record.tags.iter().find(|t| t.id == "zeta").unwrap();
        assert_eq!(zeta.label, "Zeta", "the configured language's prefLabel");
    }
}
