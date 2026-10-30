//! Goal tests for Types in `core` (#15, spec §2.2, §2.8, §3.5, §9.1, §9.2, §9.4, §9.7, §14.1,
//! §14.3): `type add|list|remove`, Type assignment by `add` and `edit`, and `find --type`.
//!
//! Assumed API (invented here; the implementer follows it, and callers in tests that are not
//! goal-owned must too: the new `add_all` arity breaks the existing call sites, which the
//! implementer updates). Every operation takes Type *references* (§9.2), resolved by
//! `types::resolve`:
//!   `hali::core::types::Type { id: String, label: String, parents: Vec<String> }`
//!       `Clone + Debug + PartialEq + Serialize`; the label is in the configured language
//!       (the smallest if several, else the id); `parents` are the sorted ids of the direct
//!       `rdfs:subClassOf` objects that are Types.
//!   `types::Page { results: Vec<Type>, total: usize, truncated: bool, next_offset: Option<usize> }`
//!   `types::resolve(&Context, &Store, reference: &str) -> Result<String, Error>`
//!       the id of the Type a reference names: exact id, then ASCII case-insensitive id, then a
//!       configured-language label compared folded; among several folded matches the first in
//!       IRI order. Empty or whitespace-only, or `hali:Resource`: usage error (exit 2).
//!       Otherwise `UNKNOWN_TYPE` (exit 1, details `{"type","suggestions"}`).
//!   `types::add(&Context, &mut Store, name: &str, parent: Option<&str>)
//!        -> Result<Mutation<Type>, Error>`
//!   `types::list(&Context, &Store, offset: usize, limit: usize) -> types::Page`
//!   `types::remove(&Context, &mut Store, reference: &str) -> Result<Mutation<Type>, Error>`
//!       the `Mutation::value` is the Type as it was; every change is on the vocabulary graph;
//!       `changes` are sorted (adds before removes, then the whole triple string, byte order).
//!   `resource::add_all(&Context, &mut Store, &[FileFacts], title: Option<&str>,
//!        types: &[String], tags: &[String], user_name: &str) -> Result<Mutation<AddedAll>, Error>`
//!   `resource::Edit { title, add_types, remove_types, add_tags, remove_tags }` (`Default`)
//!   `resource::Query { types: Vec<String>, .. }` (a new field; `Query::default()` has none)
//! Error rules (checked by `err.rule`, `err.message`, `err.details`, `err.exit_code()`):
//! `UNKNOWN_TYPE`, `IN_USE` with Type details `{"type","typed","subtypes"}`, `LABEL_TAKEN` with
//! `{"label","language","type"}`; the constructors are `Error::unknown_type`,
//! `Error::type_in_use` and `Error::type_label_taken`.
//!
//! The tests read tests/fixtures/types: Types Document, Paper (⊑ Document; "Article"@fr),
//! Note, Receipt ("Reçu"@fr; used by nothing), Ticket, Invoice, Loop1 and Loop2 (a cycle) in
//! vocabulary.ttl, one Concept `ml`; resources r1..r6 (r1 Paper and tagged ml, r2 Document,
//! r3 Note and tagged ml, r4 untyped, r5 Memo, r6 Loop2); extensions.ttl: Memo ⊑ Note (a Type
//! only there), Draft (a Type only there, unused), a blank node ⊑ Ticket, `r4 a hali:Invoice`.

mod goal {
    use hali::core::resource::{self, Edit, FileFacts, Query};
    use hali::core::types::{self, Type};
    use hali::core::{Context, Error, FixedClock, Mutation, Op, SeededIdGen};
    use hali::storage::{File, Store};
    use serde_json::json;
    use std::collections::BTreeSet;
    use std::path::Path;

    const H: &str = "https://w3id.org/hali#";
    const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
    const OWL_CLASS: &str = "http://www.w3.org/2002/07/owl#Class";
    const LABEL: &str = "http://www.w3.org/2000/01/rdf-schema#label";
    const SUBCLASS: &str = "http://www.w3.org/2000/01/rdf-schema#subClassOf";
    const MODIFIED: &str = "http://purl.org/dc/terms/modified";
    const NOW: &str = "2026-10-24T09:30:00Z";
    const SHA: &str = "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03";
    const R1: &str = "0192f0c4-0000-7000-8000-000000000501";
    const R2: &str = "0192f0c4-0000-7000-8000-000000000502";
    const R4: &str = "0192f0c4-0000-7000-8000-000000000504";
    const C1: &str = "urn:uuid:0192f0c4-0000-7000-8000-000000000601";
    const PREFIXES: &str = "@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .\n\
        @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .\n\
        @prefix owl: <http://www.w3.org/2002/07/owl#> .\n\
        @prefix skos: <http://www.w3.org/2004/02/skos/core#> .\n\
        @prefix hali: <https://w3id.org/hali#> .\n";

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

    /// A Context with its own IdGen seed, for a second call against the same store.
    fn ctx_seeded(seed: u64) -> Context {
        Context::new(
            "en",
            Box::new(FixedClock::new(NOW)),
            Box::new(SeededIdGen::new(seed)),
        )
    }

    fn load() -> Store {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/types");
        hali::storage::load(&dir).unwrap()
    }

    /// A store made from a vocabulary.ttl text (the prefixes above are put in front).
    fn store_from(vocabulary: &str) -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("vocabulary.ttl"),
            format!("{PREFIXES}{vocabulary}"),
        )
        .unwrap();
        (hali::storage::load(dir.path()).unwrap(), dir)
    }

    fn strs(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
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

    /// Every quad in the store, graph included, as text.
    fn all_quads(store: &Store) -> BTreeSet<String> {
        store.iter().map(|q| q.unwrap().to_string()).collect()
    }

    fn holds(store: &Store, s: &str, p: &str, o: &str) -> bool {
        let prefix = format!("<{s}> <{p}> <{o}> ");
        all_quads(store).iter().any(|q| q.starts_with(&prefix))
    }

    fn resources_ttl(store: &Store) -> String {
        hali::storage::to_turtle(store, File::Resources)
    }

    fn vocabulary_ttl(store: &Store) -> String {
        hali::storage::to_turtle(store, File::Vocabulary)
    }

    /// The quads in the extensions graph, which no Type operation may touch.
    fn extensions(store: &Store) -> BTreeSet<String> {
        all_quads(store)
            .into_iter()
            .filter(|q| q.contains(hali::storage::EXTENSIONS_GRAPH))
            .collect()
    }

    fn changes<T>(m: &Mutation<T>) -> Vec<(Op, String)> {
        m.changes.iter().map(|c| (c.op, c.triple.clone())).collect()
    }

    fn change_set<T>(m: &Mutation<T>) -> BTreeSet<(Op, String)> {
        changes(m).into_iter().collect()
    }

    fn tr(s: &str, p: &str, o: &str) -> String {
        format!("<{H}{s}> <{p}> <{o}> .")
    }

    fn label(s: &str, text: &str, lang: &str) -> String {
        format!("<{H}{s}> <{LABEL}> \"{text}\"@{lang} .")
    }

    fn ty(id: &str, label: &str, parents: &[&str]) -> Type {
        Type {
            id: id.to_string(),
            label: label.to_string(),
            parents: strs(parents),
        }
    }

    /// The error of a failed call, which is evaluated first (it borrows the store mutably);
    /// the store must be as it was.
    macro_rules! refused {
        ($store:expr, $before:expr, $call:expr $(,)?) => {{
            let result = $call;
            let err = result.expect_err("the call should be refused");
            assert_eq!(
                &all_quads($store),
                $before,
                "a refused call changed the store"
            );
            err
        }};
    }

    fn usage(err: &Error) {
        assert_eq!(err.rule, "USAGE", "{err:?}");
        assert_eq!(err.exit_code(), 2);
    }

    fn unknown_type<'a>(err: &'a Error, reference: &str) -> &'a serde_json::Value {
        assert_eq!(err.rule, "UNKNOWN_TYPE", "{err:?}");
        assert_eq!(err.exit_code(), 1);
        assert_eq!(err.details["type"], reference);
        &err.details
    }

    fn titles(found: &resource::Found) -> Vec<&str> {
        let mut t: Vec<&str> = found.results.iter().map(|r| r.title.as_str()).collect();
        t.sort_unstable();
        t
    }

    fn find_types(store: &Store, refs: &[&str]) -> resource::Found {
        let query = Query {
            types: strs(refs),
            ..Query::default()
        };
        resource::find(&ctx(), store, &query, |_| None).unwrap()
    }

    // ---- criterion 1: Types are owl:Classes under hali:Resource ------------------------------

    #[test]
    fn add_writes_class_label_subclass() {
        let mut store = load();
        let m = types::add(&ctx(), &mut store, "Report", None).unwrap();
        assert_eq!(m.value, ty("Report", "Report", &[]));
        assert_eq!(
            changes(&m),
            [
                (Op::Add, tr("Report", RDF_TYPE, OWL_CLASS)),
                (Op::Add, label("Report", "Report", "en")),
                (Op::Add, tr("Report", SUBCLASS, &format!("{H}Resource"))),
            ]
        );
        let vocabulary = vocabulary_ttl(&store);
        assert!(
            vocabulary.contains("hali:Report a owl:Class"),
            "{vocabulary}"
        );
        assert!(vocabulary.contains("\"Report\"@en"), "{vocabulary}");
        assert!(holds(
            &store,
            &format!("{H}Report"),
            SUBCLASS,
            &format!("{H}Resource")
        ));
        let page = types::list(&ctx(), &store, 0, 50);
        assert!(page.results.contains(&ty("Report", "Report", &[])));
    }

    #[test]
    fn add_with_parent_writes_only_parent_link() {
        let mut store = load();
        // The parent by a folded label, and not a direct link to hali:Resource.
        let m = types::add(&ctx(), &mut store, "Report", Some("paper")).unwrap();
        assert_eq!(m.value, ty("Report", "Report", &["Paper"]));
        assert_eq!(
            changes(&m),
            [
                (Op::Add, tr("Report", RDF_TYPE, OWL_CLASS)),
                (Op::Add, label("Report", "Report", "en")),
                (Op::Add, tr("Report", SUBCLASS, &format!("{H}Paper"))),
            ]
        );
        assert!(!holds(
            &store,
            &format!("{H}Report"),
            SUBCLASS,
            &format!("{H}Resource")
        ));
    }

    #[test]
    fn add_uses_configured_language() {
        let mut store = load();
        let m = types::add(&ctx_in("de"), &mut store, "Bericht", None).unwrap();
        assert_eq!(m.value, ty("Bericht", "Bericht", &[]));
        assert!(
            changes(&m).contains(&(Op::Add, label("Bericht", "Bericht", "de"))),
            "{:?}",
            m.changes
        );
        assert!(!changes(&m).iter().any(|(_, t)| t.contains("@en")));
    }

    #[test]
    fn add_parent_defined_in_extensions() {
        let mut store = load();
        let ext = extensions(&store);
        let m = types::add(&ctx(), &mut store, "Minute", Some("Memo")).unwrap();
        assert_eq!(m.value, ty("Minute", "Minute", &["Memo"]));
        assert!(changes(&m).contains(&(Op::Add, tr("Minute", SUBCLASS, &format!("{H}Memo")))));
        assert_eq!(extensions(&store), ext, "extensions.ttl is never written");
        assert!(
            !vocabulary_ttl(&store).contains("\nhali:Memo "),
            "Memo is not defined in vocabulary.ttl"
        );
    }

    #[test]
    fn add_name_grammar_is_usage() {
        let long = "a".repeat(65);
        for bad in [
            "",
            "a b",
            "1x",
            "-x",
            "_x",
            "a.b",
            "Résumé",
            " Report",
            "Report ",
            long.as_str(),
        ] {
            let mut store = load();
            let before = all_quads(&store);
            let err = refused!(&store, &before, types::add(&ctx(), &mut store, bad, None));
            usage(&err);
        }
        // The edges that are allowed: 64 characters, `_` and `-` after the first.
        let mut store = load();
        let longest = format!("A{}", "a".repeat(63));
        let m = types::add(&ctx(), &mut store, &longest, None).unwrap();
        assert_eq!(m.value.id, longest);
        let m = types::add(&ctx(), &mut store, "a_b-c9", None).unwrap();
        assert_eq!(m.value, ty("a_b-c9", "a_b-c9", &[]));
    }

    #[test]
    fn add_usage_is_checked_before_the_parent() {
        let mut store = load();
        let before = all_quads(&store);
        let err = refused!(
            &store,
            &before,
            types::add(&ctx(), &mut store, "a b", Some("nope")),
        );
        usage(&err);
    }

    #[test]
    fn add_resource_is_usage() {
        for name in ["Resource", "resource", "RESOURCE"] {
            let mut store = load();
            let before = all_quads(&store);
            let err = refused!(&store, &before, types::add(&ctx(), &mut store, name, None));
            usage(&err);
            assert!(
                err.message
                    .contains("Resource is the root class hali:Resource, not a Type"),
                "{}",
                err.message
            );
        }
        // Nor is it a parent: a reference to the root is a usage error.
        let mut store = load();
        let before = all_quads(&store);
        let err = refused!(
            &store,
            &before,
            types::add(&ctx(), &mut store, "Report", Some("Resource")),
        );
        usage(&err);
    }

    #[test]
    fn add_used_iri_is_usage() {
        // hali:ml is a Concept, hali:ResourceShape a core shape, hali:topics the scheme node.
        for name in ["ml", "ResourceShape", "topics"] {
            let mut store = load();
            let before = all_quads(&store);
            let err = refused!(&store, &before, types::add(&ctx(), &mut store, name, None));
            usage(&err);
            assert!(
                err.message.contains(&format!(
                    "hali:{name} already names something in the store; choose another name"
                )),
                "{}",
                err.message
            );
        }
        // A name used only as an object, or only as a predicate, in any graph.
        for (turtle, name) in [
            ("hali:x hali:p hali:Obj .\n", "Obj"),
            ("hali:x hali:pred hali:y .\n", "pred"),
        ] {
            let (mut store, _dir) = store_from(turtle);
            let before = all_quads(&store);
            let err = refused!(&store, &before, types::add(&ctx(), &mut store, name, None));
            usage(&err);
        }
    }

    #[test]
    fn add_unknown_parent_is_unknown_type() {
        let mut store = load();
        let before = all_quads(&store);
        let err = refused!(
            &store,
            &before,
            types::add(&ctx(), &mut store, "Report", Some("Papr")),
        );
        let details = unknown_type(&err, "Papr");
        assert_eq!(
            details["suggestions"],
            json!([{"id": "Paper", "label": "Paper"}])
        );
        assert_eq!(
            err.suggested_command.as_deref(),
            Some("hali --dry-run type add Papr")
        );
    }

    #[test]
    fn add_taken_is_label_taken() {
        // A folded id, or a configured-language label, that another Type holds. A Type that
        // exists only in extensions.ttl holds its label too.
        for (name, holder) in [("paper", "Paper"), ("MEMO", "Memo"), ("draft", "Draft")] {
            let mut store = load();
            let before = all_quads(&store);
            let err = refused!(&store, &before, types::add(&ctx(), &mut store, name, None));
            assert_eq!(err.rule, "LABEL_TAKEN", "{name}: {err:?}");
            assert_eq!(err.exit_code(), 1);
            assert_eq!(
                *err.details,
                json!({"label": name, "language": "en", "type": holder})
            );
            assert_eq!(
                err.message,
                format!("label \"{name}\"@en is already used by Type {holder}")
            );
            assert_eq!(err.suggested_command.as_deref(), Some("hali type list"));
        }
        // The id and the label differ: either one is taken.
        let (mut store, _dir) = store_from(
            "hali:Foo a owl:Class ; rdfs:label \"Bar\"@en ; rdfs:subClassOf hali:Resource .\n",
        );
        for name in ["foo", "bar", "BAR"] {
            let before = all_quads(&store);
            let err = refused!(&store, &before, types::add(&ctx(), &mut store, name, None));
            assert_eq!(err.rule, "LABEL_TAKEN", "{name}");
            assert_eq!(err.details["type"], "Foo");
        }
        // Another language's label does not hold the name: "Article" is Paper's French label.
        let mut store = load();
        let m = types::add(&ctx(), &mut store, "Article", None).unwrap();
        assert_eq!(m.value.id, "Article");
    }

    #[test]
    fn add_checks_run_in_order() {
        // The parent is resolved before LABEL_TAKEN, which comes before the used-IRI usage
        // error, which comes before SHACL.
        let mut store = load();
        let before = all_quads(&store);
        let err = refused!(
            &store,
            &before,
            types::add(&ctx(), &mut store, "Paper", Some("nope")),
        );
        assert_eq!(err.rule, "UNKNOWN_TYPE");

        // SHACL last: a store with a Concept that has no prefLabel refuses the change.
        let (mut store, _dir) = store_from(
            "hali:nolabel a skos:Concept ; skos:inScheme hali:topics .\n\
             hali:topics a skos:ConceptScheme .\n",
        );
        let before = all_quads(&store);
        let err = refused!(
            &store,
            &before,
            types::add(&ctx(), &mut store, "Report", None),
        );
        assert_eq!(err.rule, "SHACL_VIOLATION");
        // ...but a usage error comes first.
        let err = refused!(&store, &before, types::add(&ctx(), &mut store, "a b", None));
        usage(&err);
        let err = refused!(
            &store,
            &before,
            types::add(&ctx(), &mut store, "nolabel", None),
        );
        usage(&err);
    }

    #[test]
    fn list_sorted_with_parents() {
        let store = load();
        let page = types::list(&ctx(), &store, 0, 50);
        assert_eq!(
            page.results,
            [
                ty("Document", "Document", &[]),
                ty("Draft", "Draft", &[]),
                ty("Invoice", "Invoice", &[]),
                ty("Loop1", "Loop1", &["Loop2"]),
                ty("Loop2", "Loop2", &["Loop1"]),
                ty("Memo", "Memo", &["Note"]),
                ty("Note", "Note", &[]),
                ty("Paper", "Paper", &["Document"]),
                ty("Receipt", "Receipt", &[]),
                ty("Ticket", "Ticket", &[]),
            ]
        );
        assert_eq!(page.total, 10);
        assert!(!page.truncated);
        assert_eq!(page.next_offset, None);
    }

    #[test]
    fn list_label_falls_back_to_id() {
        // In French, Paper's label is "Article" and Receipt's "Reçu"; the others have no
        // French label and show their id. Sorted by label, then id.
        let store = load();
        let page = types::list(&ctx_in("fr"), &store, 0, 50);
        let shown: Vec<(&str, &str)> = page
            .results
            .iter()
            .map(|t| (t.id.as_str(), t.label.as_str()))
            .collect();
        assert_eq!(
            shown,
            [
                ("Paper", "Article"),
                ("Document", "Document"),
                ("Draft", "Draft"),
                ("Invoice", "Invoice"),
                ("Loop1", "Loop1"),
                ("Loop2", "Loop2"),
                ("Memo", "Memo"),
                ("Note", "Note"),
                ("Receipt", "Reçu"),
                ("Ticket", "Ticket"),
            ]
        );
        // The smallest label wins when a language has several.
        let (store, _dir) = store_from(
            "hali:Two a owl:Class ; rdfs:label \"Zed\"@en , \"Alpha\"@en ; \
             rdfs:subClassOf hali:Resource .\n",
        );
        let page = types::list(&ctx(), &store, 0, 50);
        assert_eq!(page.results, [ty("Two", "Alpha", &[])]);
    }

    #[test]
    fn list_paging() {
        let store = load();
        let page = types::list(&ctx(), &store, 3, 3);
        let ids: Vec<&str> = page.results.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["Loop1", "Loop2", "Memo"]);
        assert_eq!(page.total, 10);
        assert!(page.truncated);
        assert_eq!(page.next_offset, Some(6));
        let page = types::list(&ctx(), &store, 8, 5);
        let ids: Vec<&str> = page.results.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["Receipt", "Ticket"]);
        assert!(!page.truncated);
        assert_eq!(page.next_offset, None);
        let page = types::list(&ctx(), &store, 50, 5);
        assert!(page.results.is_empty());
        assert_eq!(page.total, 10);
        assert!(!page.truncated);
    }

    #[test]
    fn types_are_owl_classes_under_resource() {
        // Only an owl:Class that is a (transitive) subclass of hali:Resource is a Type:
        // not hali:Resource itself, not an alignment class above the hierarchy, not a
        // subclass without `a owl:Class`, not a class outside the hierarchy. An IRI outside
        // the hali namespace is a Type with its whole IRI as id.
        let (store, _dir) = store_from(
            "hali:Doc a owl:Class ; rdfs:label \"Doc\"@en ; \
                rdfs:subClassOf hali:Resource , <http://schema.org/CreativeWork> .\n\
             <http://schema.org/CreativeWork> a owl:Class ; rdfs:label \"Creative work\"@en .\n\
             hali:Plain rdfs:subClassOf hali:Resource ; rdfs:label \"Plain\"@en .\n\
             hali:Orphan a owl:Class ; rdfs:label \"Orphan\"@en .\n\
             <http://example.org/Other> a owl:Class ; rdfs:label \"Other\"@en ; \
                rdfs:subClassOf hali:Doc .\n",
        );
        let page = types::list(&ctx(), &store, 0, 50);
        assert_eq!(
            page.results,
            [
                ty("Doc", "Doc", &[]),
                ty("http://example.org/Other", "Other", &["Doc"]),
            ]
        );
        assert_eq!(page.total, 2);
    }

    // ---- criterion 2: several Types per Resource; untyped is just hali:Resource ---------------

    #[test]
    fn add_all_with_types_writes_rdf_type() {
        let mut store = load();
        let added = resource::add_all(
            &ctx(),
            &mut store,
            &[facts("/a/new.txt")],
            Some("new"),
            &strs(&["paper", "Memo"]),
            &strs(&["ml"]),
            "nate",
        )
        .unwrap();
        let record = &added.value.added[0];
        assert_eq!(record.types, ["Memo", "Paper"]);
        assert_eq!(record.tags[0].id, "ml");
        let subject = format!("urn:uuid:{}", record.id);
        let set = change_set(&added);
        for t in ["Paper", "Memo", "Resource"] {
            let want = (Op::Add, format!("<{subject}> <{RDF_TYPE}> <{H}{t}> ."));
            assert!(set.contains(&want), "no {want:?} in {:?}", added.changes);
            assert!(holds(&store, &subject, RDF_TYPE, &format!("{H}{t}")));
        }
        assert!(resources_ttl(&store).contains(&subject));
        // Untyped when no Type is given: just hali:Resource.
        let added = resource::add_all(
            &ctx_seeded(2),
            &mut store,
            &[facts("/a/plain.txt")],
            Some("plain"),
            &[],
            &[],
            "nate",
        )
        .unwrap();
        assert_eq!(added.value.added[0].types, Vec::<String>::new());
    }

    #[test]
    fn add_all_types_ignored_for_existing_path() {
        let mut store = load();
        let before = resources_ttl(&store);
        let quads = all_quads(&store);
        let added = resource::add_all(
            &ctx(),
            &mut store,
            &[facts("/fixtures/types/r2.txt")],
            None,
            &strs(&["Paper"]),
            &[],
            "nate",
        )
        .unwrap();
        assert!(added.value.added.is_empty());
        assert_eq!(added.value.existing.len(), 1);
        assert_eq!(added.value.existing[0].types, ["Document"]);
        assert!(added.changes.is_empty());
        assert_eq!(resources_ttl(&store), before);
        assert_eq!(all_quads(&store), quads);
    }

    #[test]
    fn add_all_unknown_type_writes_nothing() {
        let mut store = load();
        let before = all_quads(&store);
        let err = refused!(
            &store,
            &before,
            resource::add_all(
                &ctx(),
                &mut store,
                &[facts("/a/one.txt"), facts("/a/two.txt")],
                None,
                &strs(&["Paper", "Nope", "Nada"]),
                &[],
                "nate",
            ),
        );
        unknown_type(&err, "Nope");
        // The root is not a Type.
        let err = refused!(
            &store,
            &before,
            resource::add_all(
                &ctx(),
                &mut store,
                &[facts("/a/one.txt")],
                None,
                &strs(&["Resource"]),
                &[],
                "nate",
            ),
        );
        usage(&err);
    }

    #[test]
    fn add_all_resolves_types_before_tags() {
        let mut store = load();
        let before = all_quads(&store);
        let err = refused!(
            &store,
            &before,
            resource::add_all(
                &ctx(),
                &mut store,
                &[facts("/a/one.txt")],
                None,
                &strs(&["NopeType"]),
                &strs(&["nope-tag"]),
                "nate",
            ),
        );
        unknown_type(&err, "NopeType");
        let err = refused!(
            &store,
            &before,
            resource::add_all(
                &ctx(),
                &mut store,
                &[facts("/a/one.txt")],
                None,
                &strs(&["Paper"]),
                &strs(&["nope-tag"]),
                "nate",
            ),
        );
        assert_eq!(err.rule, "UNKNOWN_CONCEPT");
    }

    #[test]
    fn untyped_record_has_empty_types() {
        // A Resource record lists asserted Types only, never inherited supertypes, and an
        // untyped one has none (hali:Resource is not listed).
        let store = load();
        let shown = resource::show(&ctx(), &store, R1).unwrap();
        assert_eq!(shown.record.types, ["Paper"]);
        let shown = resource::show(&ctx(), &store, R2).unwrap();
        assert_eq!(shown.record.types, ["Document"]);
        let mut store = load();
        let added = resource::add_all(
            &ctx(),
            &mut store,
            &[facts("/a/untyped.txt")],
            None,
            &[],
            &[],
            "nate",
        )
        .unwrap();
        assert!(added.value.added[0].types.is_empty());
    }

    #[test]
    fn edit_adds_and_removes_types() {
        let mut store = load();
        let edit = Edit {
            add_types: strs(&["Note"]),
            remove_types: strs(&["paper"]),
            ..Edit::default()
        };
        let m = resource::edit(&ctx(), &mut store, R1, &edit).unwrap();
        assert_eq!(m.value.types, ["Note"]);
        let r1 = format!("urn:uuid:{R1}");
        let r1_type = |t: &str| format!("<{r1}> <{RDF_TYPE}> <{H}{t}> .");
        assert_eq!(
            change_set(&m),
            BTreeSet::from([
                (Op::Add, r1_type("Note")),
                (Op::Remove, r1_type("Paper")),
                (
                    Op::Remove,
                    format!(
                        "<{C1}> <{MODIFIED}> \"2026-10-21T08:00:00Z\"^^<http://www.w3.org/2001/XMLSchema#dateTime> ."
                    )
                ),
                (
                    Op::Add,
                    format!(
                        "<{C1}> <{MODIFIED}> \"{NOW}\"^^<http://www.w3.org/2001/XMLSchema#dateTime> ."
                    )
                ),
            ])
        );
        // hali:Resource stays, and the other tags and types are untouched.
        assert!(holds(&store, &r1, RDF_TYPE, &format!("{H}Resource")));
        assert!(holds(&store, &r1, RDF_TYPE, &format!("{H}Note")));
        assert!(!holds(&store, &r1, RDF_TYPE, &format!("{H}Paper")));
        assert_eq!(m.value.tags.len(), 1);
    }

    #[test]
    fn edit_type_noops_do_not_bump_modified() {
        let cases: [(&str, Edit); 4] = [
            // an asserted Type added
            (
                R1,
                Edit {
                    add_types: strs(&["Paper"]),
                    ..Edit::default()
                },
            ),
            // an unasserted Type removed
            (
                R1,
                Edit {
                    remove_types: strs(&["Note"]),
                    ..Edit::default()
                },
            ),
            // a Type held only through a subclass (Paper is a Document)
            (
                R1,
                Edit {
                    remove_types: strs(&["Document"]),
                    ..Edit::default()
                },
            ),
            // a Type asserted only in extensions.ttl
            (
                R4,
                Edit {
                    remove_types: strs(&["Invoice"]),
                    ..Edit::default()
                },
            ),
        ];
        for (id, edit) in cases {
            let mut store = load();
            let before = all_quads(&store);
            let m = resource::edit(&ctx(), &mut store, id, &edit).unwrap();
            assert!(m.changes.is_empty(), "{edit:?}: {:?}", m.changes);
            assert_eq!(all_quads(&store), before, "{edit:?}");
        }
    }

    #[test]
    fn edit_never_removes_hali_resource() {
        let mut store = load();
        let before = all_quads(&store);
        let edit = Edit {
            remove_types: strs(&["Resource"]),
            ..Edit::default()
        };
        let err = refused!(
            &store,
            &before,
            resource::edit(&ctx(), &mut store, R1, &edit),
        );
        usage(&err);
        let edit = Edit {
            add_types: strs(&["resource"]),
            ..Edit::default()
        };
        let err = refused!(
            &store,
            &before,
            resource::edit(&ctx(), &mut store, R1, &edit),
        );
        usage(&err);
    }

    #[test]
    fn edit_type_in_both_is_usage() {
        let mut store = load();
        let before = all_quads(&store);
        let edit = Edit {
            add_types: strs(&["Paper"]),
            remove_types: strs(&["paper"]),
            ..Edit::default()
        };
        let err = refused!(
            &store,
            &before,
            resource::edit(&ctx(), &mut store, R1, &edit),
        );
        usage(&err);
        assert!(
            err.message
                .contains("a Type cannot be both added and removed"),
            "{}",
            err.message
        );
        // Checked before the same check for tags.
        let edit = Edit {
            add_types: strs(&["Paper"]),
            remove_types: strs(&["Paper"]),
            add_tags: strs(&["ml"]),
            remove_tags: strs(&["ml"]),
            ..Edit::default()
        };
        let err = refused!(
            &store,
            &before,
            resource::edit(&ctx(), &mut store, R1, &edit),
        );
        assert!(err.message.contains("Type"), "{}", err.message);
    }

    #[test]
    fn edit_resolution_order() {
        let run = |edit: Edit| {
            let mut store = load();
            resource::edit(&ctx(), &mut store, R1, &edit).unwrap_err()
        };
        // add-type, remove-type, add-tag, remove-tag; each in argument order.
        let err = run(Edit {
            add_types: strs(&["Paper", "N1", "N1b"]),
            remove_types: strs(&["N2"]),
            add_tags: strs(&["T1"]),
            remove_tags: strs(&["T2"]),
            ..Edit::default()
        });
        unknown_type(&err, "N1");
        let err = run(Edit {
            remove_types: strs(&["N2"]),
            add_tags: strs(&["T1"]),
            remove_tags: strs(&["T2"]),
            ..Edit::default()
        });
        unknown_type(&err, "N2");
        let err = run(Edit {
            add_tags: strs(&["T1"]),
            remove_tags: strs(&["T2"]),
            ..Edit::default()
        });
        assert_eq!(err.rule, "UNKNOWN_CONCEPT");
        assert_eq!(err.details["concept"], "T1");
        let err = run(Edit {
            remove_tags: strs(&["T2"]),
            ..Edit::default()
        });
        assert_eq!(err.details["concept"], "T2");
    }

    #[test]
    fn edit_types_only_satisfies_change_requirement() {
        let mut store = load();
        let edit = Edit {
            add_types: strs(&["Note"]),
            ..Edit::default()
        };
        resource::edit(&ctx(), &mut store, R2, &edit).unwrap();
        let mut store = load();
        let edit = Edit {
            remove_types: strs(&["Document"]),
            ..Edit::default()
        };
        resource::edit(&ctx(), &mut store, R2, &edit).unwrap();
        let before = all_quads(&store);
        let err = refused!(
            &store,
            &before,
            resource::edit(&ctx(), &mut store, R2, &Edit::default()),
        );
        usage(&err);
        assert_eq!(
            err.message,
            "edit needs --title, --add-type, --remove-type, --add-tag or --remove-tag"
        );
    }

    // ---- criterion 3: UNKNOWN_TYPE and IN_USE; type remove -----------------------------------

    #[test]
    fn unknown_type_error() {
        let store = load();
        let err = types::resolve(&ctx(), &store, "Papr").unwrap_err();
        assert_eq!(err.exit_code(), 1);
        assert_eq!(err.rule, "UNKNOWN_TYPE");
        assert_eq!(err.message, "no Type matches Papr");
        assert_eq!(
            *err.details,
            json!({"type": "Papr", "suggestions": [{"id": "Paper", "label": "Paper"}]})
        );
        assert_eq!(
            err.suggested_fix.as_deref(),
            Some("Use the name of an existing Type, or add the Type first.")
        );
        assert_eq!(
            err.suggested_command.as_deref(),
            Some("hali --dry-run type add Papr")
        );

        // At most 3 suggestions, ranked by distance, then label, then id: "e" is contained in
        // many names; Memo and Note are the closest, then Paper.
        let err = types::resolve(&ctx(), &store, "e").unwrap_err();
        assert_eq!(
            err.details["suggestions"],
            json!([
                {"id": "Memo", "label": "Memo"},
                {"id": "Note", "label": "Note"},
                {"id": "Paper", "label": "Paper"},
            ])
        );

        // A reference that is not a valid Type name: list the Types instead.
        for bad in ["1x", "b c", "-x", "a.b"] {
            let err = types::resolve(&ctx(), &store, bad).unwrap_err();
            assert_eq!(err.rule, "UNKNOWN_TYPE", "{bad}");
            assert_eq!(
                err.suggested_command.as_deref(),
                Some("hali type list"),
                "{bad}"
            );
            assert_eq!(err.message, format!("no Type matches {bad}"));
        }
        // Nothing near: no suggestions.
        let err = types::resolve(&ctx(), &store, "Zzzzzzzz").unwrap_err();
        assert_eq!(err.details["suggestions"], json!([]));
        // A configured-language label only: "Article" is French.
        assert!(types::resolve(&ctx(), &store, "Article").is_err());
        assert_eq!(
            types::resolve(&ctx_in("fr"), &store, "Article").unwrap(),
            "Paper"
        );
    }

    #[test]
    fn resolve_order() {
        let (store, _dir) = store_from(
            "hali:Alpha a owl:Class ; rdfs:label \"Beta\"@en ; rdfs:subClassOf hali:Resource .\n\
             hali:Beta a owl:Class ; rdfs:label \"Gamma\"@en ; rdfs:subClassOf hali:Resource .\n\
             hali:CamelCase a owl:Class ; rdfs:label \"Zed  Zed\"@en ; \
                rdfs:subClassOf hali:Resource .\n\
             hali:Aaa a owl:Class ; rdfs:label \"Dup\"@en ; rdfs:subClassOf hali:Resource .\n\
             hali:Bbb a owl:Class ; rdfs:label \"Dup\"@en ; rdfs:subClassOf hali:Resource .\n",
        );
        let resolve = |r: &str| types::resolve(&ctx(), &store, r).unwrap();
        // The exact id wins over another Type's label, then a folded id over a label.
        assert_eq!(resolve("Beta"), "Beta");
        assert_eq!(resolve("beta"), "Beta");
        assert_eq!(resolve("camelcase"), "CamelCase");
        // A label, folded and whitespace-normalised; a label no id equals.
        assert_eq!(resolve("gamma"), "Beta");
        assert_eq!(resolve("  ZED   zed "), "CamelCase");
        // Several folded matches: the first in IRI order.
        assert_eq!(resolve("dup"), "Aaa");
        assert_eq!(resolve("DUP"), "Aaa");
    }

    #[test]
    fn empty_type_ref_is_usage() {
        let store = load();
        for empty in ["", " ", "\t \n"] {
            usage(&types::resolve(&ctx(), &store, empty).unwrap_err());
        }
        let mut store = load();
        let before = all_quads(&store);
        for empty in ["", "  "] {
            let err = refused!(&store, &before, types::remove(&ctx(), &mut store, empty));
            usage(&err);
        }
    }

    #[test]
    fn resource_ref_is_usage() {
        let mut store = load();
        for root in ["Resource", "resource", "RESOURCE"] {
            usage(&types::resolve(&ctx(), &store, root).unwrap_err());
        }
        let before = all_quads(&store);
        let err = refused!(
            &store,
            &before,
            types::remove(&ctx(), &mut store, "Resource"),
        );
        usage(&err);
    }

    #[test]
    fn remove_typed_is_in_use() {
        let mut store = load();
        let before = all_quads(&store);
        let err = refused!(&store, &before, types::remove(&ctx(), &mut store, "Paper"));
        assert_eq!(err.rule, "IN_USE");
        assert_eq!(err.exit_code(), 1);
        assert_eq!(
            *err.details,
            json!({"type": "Paper", "typed": 1, "subtypes": 0})
        );
        assert_eq!(
            err.message,
            "Type Paper is in use: 1 typed, parent of 0 Types; nothing was written"
        );
        // Also by a folded reference: the id is reported.
        let err = refused!(&store, &before, types::remove(&ctx(), &mut store, "paper"));
        assert_eq!(err.details["type"], "Paper");
    }

    #[test]
    fn remove_parent_is_in_use() {
        let mut store = load();
        let before = all_quads(&store);
        // Document: r2 is typed with it, and Paper is under it.
        let err = refused!(
            &store,
            &before,
            types::remove(&ctx(), &mut store, "Document"),
        );
        assert_eq!(
            *err.details,
            json!({"type": "Document", "typed": 1, "subtypes": 1})
        );
        // Note: r3 is typed with it, and Memo is under it by a link only extensions.ttl holds.
        let err = refused!(&store, &before, types::remove(&ctx(), &mut store, "Note"));
        assert_eq!(
            *err.details,
            json!({"type": "Note", "typed": 1, "subtypes": 1})
        );
        // A parent that nothing is typed with; the cycle counts too.
        let err = refused!(&store, &before, types::remove(&ctx(), &mut store, "Loop1"));
        assert_eq!(
            *err.details,
            json!({"type": "Loop1", "typed": 0, "subtypes": 1})
        );
        let err = refused!(&store, &before, types::remove(&ctx(), &mut store, "Loop2"));
        assert_eq!(
            *err.details,
            json!({"type": "Loop2", "typed": 1, "subtypes": 1})
        );
        // Memo is typed by r5 in resources.ttl although it is defined only in extensions.ttl.
        let err = refused!(&store, &before, types::remove(&ctx(), &mut store, "Memo"));
        assert_eq!(
            *err.details,
            json!({"type": "Memo", "typed": 1, "subtypes": 0})
        );
    }

    #[test]
    fn remove_blank_subclass_counts() {
        let mut store = load();
        let before = all_quads(&store);
        let err = refused!(&store, &before, types::remove(&ctx(), &mut store, "Ticket"));
        assert_eq!(err.rule, "IN_USE");
        assert_eq!(
            *err.details,
            json!({"type": "Ticket", "typed": 0, "subtypes": 1})
        );
    }

    #[test]
    fn remove_typed_in_extensions_is_in_use() {
        let mut store = load();
        let before = all_quads(&store);
        let err = refused!(
            &store,
            &before,
            types::remove(&ctx(), &mut store, "Invoice"),
        );
        assert_eq!(err.rule, "IN_USE");
        assert_eq!(
            *err.details,
            json!({"type": "Invoice", "typed": 1, "subtypes": 0})
        );
    }

    #[test]
    fn in_use_type_error_texts() {
        let mut store = load();
        let err = types::remove(&ctx(), &mut store, "Paper").unwrap_err();
        assert_eq!(
            err.suggested_fix.as_deref(),
            Some(
                "Remove the Type from the Resources (hali edit <ref> --remove-type Paper) and \
                 remove its subtypes first (hali type remove <S>), then retry."
            )
        );
        assert_eq!(
            err.suggested_command.as_deref(),
            Some("hali find --type Paper")
        );
        // Nothing typed: list the Types instead.
        let err = types::remove(&ctx(), &mut store, "Loop1").unwrap_err();
        assert_eq!(err.suggested_command.as_deref(), Some("hali type list"));

        // Ids from a hand-edited store: a leading `-` is attached with `=`; an id with a shell
        // metacharacter is single-quoted.
        let subject = "<urn:uuid:0192f0c4-0000-7000-8000-0000000007ff>";
        let (mut store, _dir) = store_from(&format!(
            "<{H}-x> a owl:Class ; rdfs:label \"dash x\"@en ; rdfs:subClassOf hali:Resource .\n\
             <{H}a;b> a owl:Class ; rdfs:label \"semi\"@en ; rdfs:subClassOf hali:Resource .\n\
             {subject} a <{H}-x> , <{H}a;b> .\n"
        ));
        let err = types::remove(&ctx(), &mut store, "dash x").unwrap_err();
        assert_eq!(err.rule, "IN_USE");
        assert_eq!(
            err.suggested_fix.as_deref(),
            Some(
                "Remove the Type from the Resources (hali edit <ref> --remove-type=-x) and \
                 remove its subtypes first (hali type remove <S>), then retry."
            )
        );
        assert_eq!(
            err.suggested_command.as_deref(),
            Some("hali find --type=-x")
        );
        let err = types::remove(&ctx(), &mut store, "semi").unwrap_err();
        assert!(
            err.suggested_fix
                .as_deref()
                .unwrap()
                .contains("--remove-type 'a;b')"),
            "{:?}",
            err.suggested_fix
        );
        assert_eq!(
            err.suggested_command.as_deref(),
            Some("hali find --type 'a;b'")
        );
    }

    #[test]
    fn remove_in_use_before_shacl() {
        // A store that does not conform (a Concept without a prefLabel) and holds one Type in
        // use and one that is not: IN_USE for the first, SHACL_VIOLATION for the second.
        let (mut store, _dir) = store_from(
            "hali:Used a owl:Class ; rdfs:label \"Used\"@en ; rdfs:subClassOf hali:Resource .\n\
             hali:Unused a owl:Class ; rdfs:label \"Unused\"@en ; rdfs:subClassOf hali:Resource .\n\
             <urn:uuid:0192f0c4-0000-7000-8000-0000000007fe> a hali:Used .\n\
             hali:nolabel a skos:Concept ; skos:inScheme hali:topics .\n\
             hali:topics a skos:ConceptScheme .\n",
        );
        let before = all_quads(&store);
        let err = refused!(&store, &before, types::remove(&ctx(), &mut store, "Used"));
        assert_eq!(err.rule, "IN_USE");
        let err = refused!(&store, &before, types::remove(&ctx(), &mut store, "Unused"));
        assert_eq!(err.rule, "SHACL_VIOLATION");
        assert_eq!(err.exit_code(), 1);
    }

    #[test]
    fn remove_takes_vocabulary_triples_only() {
        let mut store = load();
        let resources = resources_ttl(&store);
        let ext = extensions(&store);
        let m = types::remove(&ctx(), &mut store, "receipt").unwrap();
        assert_eq!(m.value, ty("Receipt", "Receipt", &[]));
        // Every triple with Receipt as subject, every language, and nothing else.
        assert_eq!(
            changes(&m),
            [
                (Op::Remove, tr("Receipt", RDF_TYPE, OWL_CLASS)),
                (Op::Remove, label("Receipt", "Receipt", "en")),
                (Op::Remove, label("Receipt", "Reçu", "fr")),
                (Op::Remove, tr("Receipt", SUBCLASS, &format!("{H}Resource"))),
            ]
        );
        assert_eq!(
            resources_ttl(&store),
            resources,
            "resources.ttl is untouched"
        );
        assert_eq!(extensions(&store), ext, "extensions.ttl is untouched");
        let page = types::list(&ctx(), &store, 0, 50);
        assert_eq!(page.total, 9);
        assert!(!page.results.iter().any(|t| t.id == "Receipt"));
        assert!(
            page.results.iter().any(|t| t.id == "Paper"),
            "other Types stay"
        );
        assert!(!vocabulary_ttl(&store).contains("Receipt"));
    }

    #[test]
    fn remove_removes_inbound_triples() {
        let (mut store, _dir) = store_from(
            "hali:Gone a owl:Class ; rdfs:label \"Gone\"@en ; rdfs:subClassOf hali:Resource .\n\
             hali:Other a owl:Class ; rdfs:label \"Other\"@en ; rdfs:subClassOf hali:Resource ; \
                rdfs:seeAlso hali:Gone .\n",
        );
        let m = types::remove(&ctx(), &mut store, "Gone").unwrap();
        let see_also = "http://www.w3.org/2000/01/rdf-schema#seeAlso";
        assert!(
            changes(&m).contains(&(Op::Remove, tr("Other", see_also, &format!("{H}Gone")))),
            "{:?}",
            m.changes
        );
        assert_eq!(m.changes.len(), 4, "{:?}", m.changes);
        assert!(!vocabulary_ttl(&store).contains("Gone"));
        assert!(vocabulary_ttl(&store).contains("hali:Other"));
    }

    #[test]
    fn remove_extensions_only_type_is_unchanged() {
        // Draft exists only in extensions.ttl and nothing uses it: nothing to remove.
        let mut store = load();
        let before = all_quads(&store);
        let m = types::remove(&ctx(), &mut store, "Draft").unwrap();
        assert_eq!(m.value, ty("Draft", "Draft", &[]));
        assert!(m.changes.is_empty(), "{:?}", m.changes);
        assert_eq!(all_quads(&store), before);
    }

    #[test]
    fn remove_hali_resource_is_usage() {
        let mut store = load();
        let before = all_quads(&store);
        for root in ["Resource", "resource"] {
            let err = refused!(&store, &before, types::remove(&ctx(), &mut store, root));
            usage(&err);
        }
        let err = refused!(&store, &before, types::remove(&ctx(), &mut store, "Nope"));
        unknown_type(&err, "Nope");
    }

    // ---- criterion 4: the SPARQL subclass query over the fixture graph ------------------------

    #[test]
    fn find_type_matches_subclasses() {
        let store = load();
        let found = find_types(&store, &["Document"]);
        assert_eq!(titles(&found), ["r1", "r2"]);
        assert_eq!(found.total, 2);
        // By a folded id and a label too; a leaf type matches only itself.
        assert_eq!(titles(&find_types(&store, &["document"])), ["r1", "r2"]);
        assert_eq!(titles(&find_types(&store, &["Paper"])), ["r1"]);
    }

    #[test]
    fn find_type_does_not_match_superclasses_or_unused() {
        let store = load();
        let found = find_types(&store, &["Receipt"]);
        assert!(found.results.is_empty());
        assert_eq!(found.total, 0);
        // Ticket has only a blank-node subclass: nothing matches, and nothing breaks.
        assert!(find_types(&store, &["Ticket"]).results.is_empty());
    }

    #[test]
    fn find_type_through_extensions_subclass() {
        let store = load();
        // Memo is a subclass of Note only in extensions.ttl; r5 is a Memo.
        assert_eq!(titles(&find_types(&store, &["Note"])), ["r3", "r5"]);
        assert_eq!(titles(&find_types(&store, &["Memo"])), ["r5"]);
    }

    #[test]
    fn find_type_cycle_terminates() {
        let store = load();
        assert_eq!(titles(&find_types(&store, &["Loop1"])), ["r6"]);
        assert_eq!(titles(&find_types(&store, &["Loop2"])), ["r6"]);
    }

    #[test]
    fn find_type_and_tag_are_anded() {
        let store = load();
        let query = Query {
            types: strs(&["Document"]),
            tags: strs(&["ml"]),
            ..Query::default()
        };
        let found = resource::find(&ctx(), &store, &query, |_| None).unwrap();
        assert_eq!(titles(&found), ["r1"]);
        let query = Query {
            types: strs(&["Note"]),
            tags: strs(&["ml"]),
            ..Query::default()
        };
        let found = resource::find(&ctx(), &store, &query, |_| None).unwrap();
        assert_eq!(titles(&found), ["r3"]);
        let query = Query {
            types: strs(&["Document"]),
            title: Some("r2".to_string()),
            ..Query::default()
        };
        let found = resource::find(&ctx(), &store, &query, |_| None).unwrap();
        assert_eq!(titles(&found), ["r2"]);
    }

    #[test]
    fn find_types_anded() {
        let store = load();
        assert!(find_types(&store, &["Document", "Note"]).results.is_empty());
        assert_eq!(titles(&find_types(&store, &["Document", "Paper"])), ["r1"]);
        assert_eq!(titles(&find_types(&store, &["Paper", "paper"])), ["r1"]);
    }

    #[test]
    fn find_exact_does_not_affect_type() {
        let store = load();
        let query = Query {
            types: strs(&["Document"]),
            exact: true,
            ..Query::default()
        };
        let found = resource::find(&ctx(), &store, &query, |_| None).unwrap();
        assert_eq!(titles(&found), ["r1", "r2"]);
    }

    #[test]
    fn find_unknown_type_is_unknown_type() {
        let store = load();
        let query = |types: &[&str], tags: &[&str]| Query {
            types: strs(types),
            tags: strs(tags),
            ..Query::default()
        };
        let err = resource::find(&ctx(), &store, &query(&["Nope"], &[]), |_| None).unwrap_err();
        unknown_type(&err, "Nope");
        let err = resource::find(
            &ctx(),
            &store,
            &query(&["Paper", "Nope", "Nada"], &[]),
            |_| None,
        )
        .unwrap_err();
        unknown_type(&err, "Nope");
        // Tags are resolved before Types.
        let err =
            resource::find(&ctx(), &store, &query(&["Nope"], &["nope-tag"]), |_| None).unwrap_err();
        assert_eq!(err.rule, "UNKNOWN_CONCEPT");
        // The root is not a Type.
        let err = resource::find(&ctx(), &store, &query(&["Resource"], &[]), |_| None).unwrap_err();
        usage(&err);
    }

    #[test]
    fn subclass_path_over_fixture() {
        // The query is `rdf:type/rdfs:subClassOf*` over the union of the graphs:
        // direct (Paper), transitive (Document), a link in extensions.ttl (Note), a cycle
        // (Loop1), and a blank-node subclass, which matches nothing and breaks nothing.
        let store = load();
        // r4 is a hali:Invoice only in extensions.ttl, which the query sees.
        let cases: [(&str, &[&str]); 6] = [
            ("Paper", &["r1"]),
            ("Document", &["r1", "r2"]),
            ("Note", &["r3", "r5"]),
            ("Loop1", &["r6"]),
            ("Ticket", &[]),
            ("Invoice", &["r4"]),
        ];
        for (type_ref, want) in cases {
            assert_eq!(titles(&find_types(&store, &[type_ref])), want, "{type_ref}");
        }
    }
}
