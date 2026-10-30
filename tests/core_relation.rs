//! Goal tests for relations between Resources in `core` (#18, spec §2.4, §2.6, §2.7, §3.1, §9.1,
//! §9.2, §9.4, §9.7): `relate`, `unrelate`, `relation add|list|remove`, relation references,
//! `show` relations, `find --related-to/--via` and `forget` with `--force`.
//!
//! Assumed API (invented here; the implementer follows it, and callers in tests that are not
//! goal-owned must too: the new `forget` arity breaks the existing call sites, which the
//! implementer updates). Module `hali::core::relation`:
//!   `RelationKind { id: String, label: String, parents: Vec<String>, standard: bool }`
//!       `Clone + Debug + PartialEq + Serialize`. A relation kind is one of the four standard
//!       relations (`dcterms:relation`, `dcterms:isPartOf`, `dcterms:references`,
//!       `prov:wasDerivedFrom`) or a named `owl:ObjectProperty` P with `P rdfs:subPropertyOf+` a
//!       standard one, both read from every graph. Its id is the `hali:` local name, else
//!       `prefix:local` from the fixed prefix table (`dcterms:isPartOf`), else the whole IRI.
//!       `label` is the configured-language `rdfs:label` (the smallest if several), else the id.
//!       `parents` are the sorted ids of the direct `rdfs:subPropertyOf` objects that are kinds.
//!   `Page { results: Vec<RelationKind>, total: usize, truncated: bool, next_offset: Option<usize> }`
//!   `Link { from: String, relation: String, to: String }` (`Clone + Debug + PartialEq + Serialize`):
//!       the Resource ids (bare UUIDs) and the kind id.
//!   `check_name(&str) -> Result<(), Error>` / `valid_name(&str) -> bool`: `^[a-z][A-Za-z0-9_-]*$`,
//!       at most 64 characters; anything else is a usage error (exit 2).
//!   `resolve(&Context, &Store, reference: &str) -> Result<String, Error>`: the id of the kind a
//!       reference names (spec §9.2): the exact id; then the reference expanded through the fixed
//!       prefix table equal to a kind's IRI; then an ASCII case-insensitive id match (the first
//!       in IRI order wins). A bare name never resolves to a standard kind. Empty or
//!       whitespace-only: usage error. Otherwise `UNKNOWN_RELATION` (exit 1, details
//!       `{"relation","suggestions"}`).
//!   `add(&Context, &mut Store, name: &str, parent: &str) -> Result<Mutation<RelationKind>, Error>`
//!   `list(&Context, &Store, offset: usize, limit: usize) -> Page`
//!   `remove(&Context, &mut Store, reference: &str) -> Result<Mutation<RelationKind>, Error>`
//!       the `Mutation::value` is the kind as it was; every change is on the vocabulary graph.
//!   `relate(&Context, &mut Store, from: &str, relation: &str, to: &str) -> Result<Mutation<Link>, Error>`
//!   `unrelate(&Context, &mut Store, from: &str, relation: &str, to: &str) -> Result<Mutation<Link>, Error>`
//!       `from` and `to` are Resource ids (what `resource::resolve_ref` returns); `relation` is a
//!       relation reference. Checks, in order: `from` is a cataloged Resource
//!       (`UNKNOWN_RESOURCE`), the relation (`UNKNOWN_RELATION`), `to` is a cataloged Resource
//!       (`UNKNOWN_RESOURCE`; `unrelate` takes any well-formed UUID), `from` = `to` (usage), SHACL.
//!       The CLI resolves its three arguments in that same order, so the first unknown one is the
//!       one reported. `changes` are on the resources graph; on a change, `from`'s catalog
//!       record's `dcterms:modified` is replaced by now, as `edit` does.
//! Module `hali::core::resource`:
//!   `forget(&Context, &mut Store, id: &str, force: bool) -> Result<Mutation<Record>, Error>`
//!   `Query { related_to: Option<String>, via: Option<String>, .. }` (new fields; `Query::default()`
//!       has `None`s): `related_to` is a Resource id, `via` a relation reference.
//!   `Shown.relations`: serialises as the `relations` array of `show`, each entry
//!       `{"direction": "out"|"in", "relation": <kind id>, "resource": <Record>}`; the CLI
//!       observes (`missing`/`stale`) each entry's Resource record as it does the shown one, so
//!       the field holds the `Record` (suggested: `Related { direction: String, relation: String,
//!       resource: Record }`).
//! Error rules (checked by `err.rule`, `err.message`, `err.details`, `err.exit_code()`):
//! `UNKNOWN_RELATION`, `FORGET_BLOCKED_BY_RELATIONS`, and `IN_USE` / `LABEL_TAKEN` with relation
//! details; the constructors are `Error::unknown_relation(reference, &[resource::Tag])`,
//! `Error::relation_in_use(id, used, subproperties)`,
//! `Error::relation_label_taken(label, language, holder)` and
//! `Error::forget_blocked_by_relations(resource, relations, from)`.
//!
//! The tests read tests/fixtures/relations: Type Note; Concept `ml`; the kinds `annotates`
//! (⊑ dcterms:references; labels "annotates"@en and "annote"@fr), `summarises`
//! (⊑ annotates), `excerptOf` ("excerpt of"@en, ⊑ prov:wasDerivedFrom, unused) and `digestOf`
//! (no label, ⊑ dcterms:relation) in vocabulary.ttl; resources r1..r5 (ids …0701..0705, catalog
//! records …0711..0715; r1 Note, r3 tagged ml, r4 Note and tagged ml); relations
//! `r1 dcterms:isPartOf r2`, `r3 hali:annotates r1`, `r3 dcterms:relation r1`,
//! `r4 prov:wasDerivedFrom r1`, `r4 hali:summarises r2`; extensions.ttl: `hali:quotes` (⊑
//! dcterms:references) and `hali:sketch` (⊑ dcterms:isPartOf, unused), kinds that exist only there;
//! `r5 hali:quotes r3` (an inbound relation of r3 only that file holds); a blank node ⊑
//! `hali:digestOf`; a comment on `hali:excerptOf`.
//! The context's ids are not used by any operation here, so one `ctx()` per call is enough.

mod goal {
    use hali::core::relation::{self, Link, RelationKind};
    use hali::core::resource::{self, Query};
    use hali::core::{Context, Error, FixedClock, Mutation, Op, SeededIdGen};
    use hali::storage::{File, Store};
    use serde_json::{Value, json};
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    const H: &str = "https://w3id.org/hali#";
    const DCT: &str = "http://purl.org/dc/terms/";
    const PROV: &str = "http://www.w3.org/ns/prov#";
    const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
    const OWL_OBJECT_PROPERTY: &str = "http://www.w3.org/2002/07/owl#ObjectProperty";
    const LABEL: &str = "http://www.w3.org/2000/01/rdf-schema#label";
    const SUBPROPERTY: &str = "http://www.w3.org/2000/01/rdf-schema#subPropertyOf";
    const DATE_TIME: &str = "http://www.w3.org/2001/XMLSchema#dateTime";
    const NOW: &str = "2026-10-24T09:30:00Z";
    const OLD: &str = "2026-10-21T08:00:00Z";
    const R1: &str = "0192f0c4-0000-7000-8000-000000000701";
    const R2: &str = "0192f0c4-0000-7000-8000-000000000702";
    const R3: &str = "0192f0c4-0000-7000-8000-000000000703";
    const R4: &str = "0192f0c4-0000-7000-8000-000000000704";
    const R5: &str = "0192f0c4-0000-7000-8000-000000000705";
    const C1: &str = "0192f0c4-0000-7000-8000-000000000711";
    const C3: &str = "0192f0c4-0000-7000-8000-000000000713";
    const C5: &str = "0192f0c4-0000-7000-8000-000000000715";
    /// A well-formed UUID that no Resource has.
    const GONE: &str = "0192f0c4-0000-7000-8000-000000000799";
    const GONE2: &str = "0192f0c4-0000-7000-8000-000000000798";
    const PREFIXES: &str = "@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .\n\
        @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .\n\
        @prefix owl: <http://www.w3.org/2002/07/owl#> .\n\
        @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .\n\
        @prefix skos: <http://www.w3.org/2004/02/skos/core#> .\n\
        @prefix dcterms: <http://purl.org/dc/terms/> .\n\
        @prefix prov: <http://www.w3.org/ns/prov#> .\n\
        @prefix hali: <https://w3id.org/hali#> .\n";
    /// A store with a Concept that has no label: every mutation of it fails SHACL.
    const BROKEN: &str = "hali:nolabel a skos:Concept ; skos:inScheme hali:topics .\n";

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

    /// A store made from these files alone (name, Turtle text; the prefixes are put in front).
    fn store_from(files: &[(&str, &str)]) -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        for (name, text) in files {
            std::fs::write(dir.path().join(name), format!("{PREFIXES}{text}")).unwrap();
        }
        (hali::storage::load(dir.path()).unwrap(), dir)
    }

    fn strs(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    /// Every quad in the store, graph included, as text.
    fn all_quads(store: &Store) -> BTreeSet<String> {
        store.iter().map(|q| q.unwrap().to_string()).collect()
    }

    fn holds_in(store: &Store, file: File, s: &str, p: &str, o: &str) -> bool {
        all_quads(store).contains(&format!("<{s}> <{p}> <{o}> {}", file.graph()))
    }

    /// Whether a triple is held in any graph.
    fn holds(store: &Store, s: &str, p: &str, o: &str) -> bool {
        let prefix = format!("<{s}> <{p}> <{o}> ");
        all_quads(store).iter().any(|q| q.starts_with(&prefix))
    }

    fn urn(id: &str) -> String {
        format!("urn:uuid:{id}")
    }

    fn resources_ttl(store: &Store) -> String {
        hali::storage::to_turtle(store, File::Resources)
    }

    fn vocabulary_ttl(store: &Store) -> String {
        hali::storage::to_turtle(store, File::Vocabulary)
    }

    /// The quads in one graph, which an operation that must not touch it leaves alone.
    fn graph_quads(store: &Store, graph: &str) -> BTreeSet<String> {
        all_quads(store)
            .into_iter()
            .filter(|q| q.contains(graph))
            .collect()
    }

    fn extensions(store: &Store) -> BTreeSet<String> {
        graph_quads(store, hali::storage::EXTENSIONS_GRAPH)
    }

    fn changes<T>(m: &Mutation<T>) -> Vec<(Op, String)> {
        m.changes.iter().map(|c| (c.op, c.triple.clone())).collect()
    }

    /// `<subject> <predicate> <object> .` for two Resources.
    fn rel(from: &str, predicate: &str, to: &str) -> String {
        format!("<urn:uuid:{from}> <{predicate}> <urn:uuid:{to}> .")
    }

    fn hali_term(local: &str) -> String {
        format!("{H}{local}")
    }

    fn modified(catalog: &str, at: &str) -> String {
        format!("<urn:uuid:{catalog}> <{DCT}modified> \"{at}\"^^<{DATE_TIME}> .")
    }

    /// The two changes of a bumped `dcterms:modified` on a catalog record.
    fn bump(catalog: &str) -> [(Op, String); 2] {
        [
            (Op::Add, modified(catalog, NOW)),
            (Op::Remove, modified(catalog, OLD)),
        ]
    }

    fn link(from: &str, relation: &str, to: &str) -> Link {
        Link {
            from: from.to_string(),
            relation: relation.to_string(),
            to: to.to_string(),
        }
    }

    fn kind(id: &str, label: &str, parents: &[&str], standard: bool) -> RelationKind {
        RelationKind {
            id: id.to_string(),
            label: label.to_string(),
            parents: strs(parents),
            standard,
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

    fn unknown_relation<'a>(err: &'a Error, reference: &str) -> &'a Value {
        assert_eq!(err.rule, "UNKNOWN_RELATION", "{err:?}");
        assert_eq!(err.exit_code(), 1);
        assert_eq!(err.details["relation"], reference);
        &err.details
    }

    fn unknown_resource(err: &Error, reference: &str) {
        assert_eq!(err.rule, "UNKNOWN_RESOURCE", "{err:?}");
        assert_eq!(err.exit_code(), 1);
        assert_eq!(err.details["ref"], reference);
    }

    fn titles(found: &resource::Found) -> Vec<&str> {
        let mut t: Vec<&str> = found.results.iter().map(|r| r.title.as_str()).collect();
        t.sort_unstable();
        t
    }

    fn find_related(store: &Store, related_to: &str, via: Option<&str>) -> resource::Found {
        let query = Query {
            related_to: Some(related_to.to_string()),
            via: via.map(str::to_string),
            ..Query::default()
        };
        resource::find(&ctx(), store, &query, |_| None).unwrap()
    }

    /// A Resource record as `show` prints it for the fixture, before the CLI observes the file.
    fn rec(n: u32, types: &[&str], tagged: bool) -> Value {
        let tags = if tagged {
            json!([{"id": "ml", "label": "ml"}])
        } else {
            json!([])
        };
        json!({
            "id": format!("0192f0c4-0000-7000-8000-00000000070{n}"),
            "path": format!("/fixtures/relations/r{n}.txt"),
            "title": format!("r{n}"),
            "media_type": "text/plain",
            "editable": true,
            "missing": false,
            "stale": false,
            "types": types,
            "tags": tags,
            "byte_size": 6,
            "modified": "2026-10-20T14:02:11Z",
        })
    }

    fn entry(direction: &str, relation: &str, resource: Value) -> Value {
        json!({"direction": direction, "relation": relation, "resource": resource})
    }

    fn shown_relations(store: &Store, id: &str) -> Value {
        let shown = resource::show(&ctx(), store, id).unwrap();
        serde_json::to_value(&shown.relations).unwrap()
    }

    /// The nine kinds of the fixture and the one more that only extensions.ttl has, in the order
    /// `list` prints them (by label, then id).
    fn all_kinds() -> Vec<RelationKind> {
        vec![
            kind("annotates", "annotates", &["dcterms:references"], false),
            kind("digestOf", "digestOf", &["dcterms:relation"], false),
            kind("excerptOf", "excerpt of", &["prov:wasDerivedFrom"], false),
            kind(
                "dcterms:isPartOf",
                "is part of",
                &["dcterms:relation"],
                true,
            ),
            kind("quotes", "quotes", &["dcterms:references"], false),
            kind(
                "dcterms:references",
                "references",
                &["dcterms:relation"],
                true,
            ),
            kind("dcterms:relation", "relation", &[], true),
            kind("sketch", "sketch", &["dcterms:isPartOf"], false),
            kind("summarises", "summarises", &["annotates"], false),
            kind("prov:wasDerivedFrom", "was derived from", &[], true),
        ]
    }

    // ---- criterion 1: relate and unrelate; one direction stored --------------------------------

    #[test]
    fn relate_writes_one_triple() {
        let mut store = load();
        let ext = extensions(&store);
        let m = relation::relate(&ctx(), &mut store, R5, "annotates", R2).unwrap();
        assert_eq!(m.value, link(R5, "annotates", R2));
        // The single triple, and the bump of the catalog record of the Resource it leaves.
        let [add_modified, remove_modified] = bump(C5);
        assert_eq!(
            changes(&m),
            [
                (Op::Add, rel(R5, &hali_term("annotates"), R2)),
                add_modified,
                remove_modified
            ]
        );
        assert!(holds_in(
            &store,
            File::Resources,
            &urn(R5),
            &hali_term("annotates"),
            &urn(R2)
        ));
        // The inverse is never stored: nothing runs from r2 to r5.
        let backwards = format!("<urn:uuid:{R2}> ");
        assert!(
            !all_quads(&store)
                .iter()
                .any(|q| q.starts_with(&backwards) && q.contains(&format!("<urn:uuid:{R5}>"))),
            "the inverse was stored"
        );
        assert_eq!(extensions(&store), ext, "extensions.ttl is never written");
        let resources = resources_ttl(&store);
        assert!(resources.contains("hali:annotates"), "{resources}");
        assert!(resources.contains(NOW), "{resources}");
    }

    #[test]
    fn relate_resolves_the_relation_reference() {
        // A prefixed standard term, a case-insensitive id and a hali: term, each stored under the
        // kind it names and reported by its id.
        for (reference, id, predicate) in [
            (
                "dcterms:references",
                "dcterms:references",
                format!("{DCT}references"),
            ),
            ("ANNOTATES", "annotates", hali_term("annotates")),
            ("hali:summarises", "summarises", hali_term("summarises")),
            ("quotes", "quotes", hali_term("quotes")),
        ] {
            let mut store = load();
            let m = relation::relate(&ctx(), &mut store, R5, reference, R2).unwrap();
            assert_eq!(m.value, link(R5, id, R2), "{reference}");
            assert!(
                changes(&m).contains(&(Op::Add, rel(R5, &predicate, R2))),
                "{reference}: {:?}",
                m.changes
            );
        }
    }

    #[test]
    fn relate_duplicate_is_noop() {
        let mut store = load();
        let before = all_quads(&store);
        for reference in ["dcterms:isPartOf", "DCTERMS:ISPARTOF"] {
            let m = relation::relate(&ctx(), &mut store, R1, reference, R2).unwrap();
            assert_eq!(m.value, link(R1, "dcterms:isPartOf", R2));
            assert!(m.changes.is_empty(), "{:?}", m.changes);
            assert_eq!(all_quads(&store), before, "no bump either");
        }
    }

    #[test]
    fn relate_adds_triple_held_only_in_extensions() {
        // extensions.ttl holds `r5 quotes r3`; the triple is still written to resources.ttl.
        let mut store = load();
        let ext = extensions(&store);
        let m = relation::relate(&ctx(), &mut store, R5, "quotes", R3).unwrap();
        let [add_modified, remove_modified] = bump(C5);
        assert_eq!(
            changes(&m),
            [
                (Op::Add, rel(R5, &hali_term("quotes"), R3)),
                add_modified,
                remove_modified
            ]
        );
        assert!(holds_in(
            &store,
            File::Resources,
            &urn(R5),
            &hali_term("quotes"),
            &urn(R3)
        ));
        assert_eq!(extensions(&store), ext);
    }

    #[test]
    fn relate_self_is_usage() {
        let mut store = load();
        let before = all_quads(&store);
        let err = refused!(
            &store,
            &before,
            relation::relate(&ctx(), &mut store, R1, "dcterms:relation", R1),
        );
        usage(&err);
        assert!(
            err.message
                .contains("a Resource cannot be related to itself"),
            "{}",
            err.message
        );
    }

    #[test]
    fn relate_order_of_checks() {
        let mut store = load();
        let before = all_quads(&store);
        // A first, then the relation, then B.
        let err = refused!(
            &store,
            &before,
            relation::relate(&ctx(), &mut store, GONE, "nope", GONE2),
        );
        unknown_resource(&err, GONE);
        let err = refused!(
            &store,
            &before,
            relation::relate(&ctx(), &mut store, R1, "nope", GONE2),
        );
        unknown_relation(&err, "nope");
        let err = refused!(
            &store,
            &before,
            relation::relate(&ctx(), &mut store, R1, "annotates", GONE2),
        );
        unknown_resource(&err, GONE2);
        // The relation is resolved before the self-relation usage error.
        let err = refused!(
            &store,
            &before,
            relation::relate(&ctx(), &mut store, R1, "nope", R1),
        );
        unknown_relation(&err, "nope");

        // SHACL comes last: with a store that does not conform, a self-relation is still a usage
        // error and a good call is refused by SHACL.
        let (mut store, _dir) = load_with(&[("vocabulary.ttl", BROKEN)]);
        let before = all_quads(&store);
        let err = refused!(
            &store,
            &before,
            relation::relate(&ctx(), &mut store, R1, "annotates", R1),
        );
        usage(&err);
        let err = refused!(
            &store,
            &before,
            relation::relate(&ctx(), &mut store, R2, "annotates", R3),
        );
        assert_eq!(err.rule, "SHACL_VIOLATION", "{err:?}");
        assert_eq!(err.exit_code(), 1);
    }

    #[test]
    fn relate_non_relation_predicate_is_unknown() {
        // A property that is no relation kind cannot be used as one.
        let mut store = load();
        let before = all_quads(&store);
        for reference in ["dcterms:title", "rdfs:subPropertyOf", "hali:Note", "Note"] {
            let err = refused!(
                &store,
                &before,
                relation::relate(&ctx(), &mut store, R1, reference, R2),
            );
            unknown_relation(&err, reference);
        }
    }

    #[test]
    fn empty_relation_ref_is_usage() {
        let mut store = load();
        let before = all_quads(&store);
        for reference in ["", "  ", "\t"] {
            let err = refused!(
                &store,
                &before,
                relation::relate(&ctx(), &mut store, R1, reference, R2),
            );
            usage(&err);
            let err = refused!(
                &store,
                &before,
                relation::unrelate(&ctx(), &mut store, R1, reference, R2),
            );
            usage(&err);
            usage(&relation::resolve(&ctx(), &store, reference).unwrap_err());
            let err = refused!(
                &store,
                &before,
                relation::remove(&ctx(), &mut store, reference),
            );
            usage(&err);
            let query = Query {
                related_to: Some(R1.to_string()),
                via: Some(reference.to_string()),
                ..Query::default()
            };
            usage(&resource::find(&ctx(), &store, &query, |_| None).unwrap_err());
        }
    }

    #[test]
    fn unrelate_exact_only() {
        // Only the triple `r3 annotates r1`: not its sub-property or super-property triples, not
        // the reverse direction, and `r3 dcterms:relation r1` stays.
        let mut store = load();
        let before = all_quads(&store);
        for (from, reference, to) in [
            (R3, "dcterms:references", R1), // annotates is a sub-property of references
            (R3, "summarises", R1),         // a sub-property of annotates
            (R1, "annotates", R3),          // the reverse direction
            (R3, "annotates", R2),          // another target
        ] {
            let m = relation::unrelate(&ctx(), &mut store, from, reference, to).unwrap();
            assert!(m.changes.is_empty(), "{reference}: {:?}", m.changes);
            assert_eq!(all_quads(&store), before);
        }
        let m = relation::unrelate(&ctx(), &mut store, R3, "annotates", R1).unwrap();
        assert_eq!(m.value, link(R3, "annotates", R1));
        let [add_modified, remove_modified] = bump(C3);
        assert_eq!(
            changes(&m),
            [
                add_modified,
                (Op::Remove, rel(R3, &hali_term("annotates"), R1)),
                remove_modified
            ]
        );
        assert!(!holds(&store, &urn(R3), &hali_term("annotates"), &urn(R1)));
        assert!(holds(&store, &urn(R3), &format!("{DCT}relation"), &urn(R1)));
        assert!(holds(
            &store,
            &urn(R4),
            &format!("{PROV}wasDerivedFrom"),
            &urn(R1)
        ));
    }

    #[test]
    fn unrelate_absent_and_extensions_only_noop() {
        let mut store = load();
        let before = all_quads(&store);
        // `r5 quotes r3` is held only by extensions.ttl: nothing to remove from resources.ttl.
        let m = relation::unrelate(&ctx(), &mut store, R5, "quotes", R3).unwrap();
        assert_eq!(m.value, link(R5, "quotes", R3));
        assert!(m.changes.is_empty(), "{:?}", m.changes);
        assert_eq!(all_quads(&store), before);
        // A triple that is not there at all: no change, no bump.
        let m = relation::unrelate(&ctx(), &mut store, R2, "dcterms:relation", R1).unwrap();
        assert!(m.changes.is_empty(), "{:?}", m.changes);
        assert_eq!(all_quads(&store), before);
    }

    #[test]
    fn unrelate_checks_run_in_order() {
        let mut store = load();
        let before = all_quads(&store);
        let err = refused!(
            &store,
            &before,
            relation::unrelate(&ctx(), &mut store, GONE, "nope", GONE2),
        );
        unknown_resource(&err, GONE);
        let err = refused!(
            &store,
            &before,
            relation::unrelate(&ctx(), &mut store, R1, "nope", GONE2),
        );
        unknown_relation(&err, "nope");
        let err = refused!(
            &store,
            &before,
            relation::unrelate(&ctx(), &mut store, R1, "dcterms:relation", R1),
        );
        usage(&err);
    }

    #[test]
    fn unrelate_dangling_full_uuid() {
        // A hand-edited relation to a Resource that is no longer cataloged: `unrelate` takes the
        // full UUID, removes exactly that triple and the store conforms again.
        let dangling = format!("<urn:uuid:{R1}> dcterms:relation <urn:uuid:{GONE}> .");
        let (mut store, _dir) = load_with(&[("resources.ttl", &dangling)]);
        let m = relation::unrelate(&ctx(), &mut store, R1, "dcterms:relation", GONE).unwrap();
        assert_eq!(m.value, link(R1, "dcterms:relation", GONE));
        let [add_modified, remove_modified] = bump(C1);
        assert_eq!(
            changes(&m),
            [
                add_modified,
                (Op::Remove, rel(R1, &format!("{DCT}relation"), GONE)),
                remove_modified
            ]
        );
        assert!(
            hali::core::validate::validate(&store, 0, 50)
                .unwrap()
                .conforms
        );
    }

    #[test]
    fn dangling_relation_makes_every_mutation_fail_shacl() {
        // `relate` to a node that is no cataloged Resource cannot be asked for (it is
        // UNKNOWN_RESOURCE), but one already in the store breaks the target shape, so every
        // mutation is refused until `unrelate` clears it.
        let dangling = format!("<urn:uuid:{R1}> dcterms:relation <urn:uuid:{GONE}> .");
        let (mut store, _dir) = load_with(&[("resources.ttl", &dangling)]);
        let before = all_quads(&store);
        let err = refused!(
            &store,
            &before,
            relation::relate(&ctx(), &mut store, R2, "annotates", R3),
        );
        assert_eq!(err.rule, "SHACL_VIOLATION", "{err:?}");
        let focus = format!("<urn:uuid:{R1}>");
        assert!(
            err.details["violations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["focus_node"] == focus.as_str()),
            "{}",
            err.details
        );
        let err = refused!(
            &store,
            &before,
            relation::unrelate(&ctx(), &mut store, R3, "annotates", R1),
        );
        assert_eq!(err.rule, "SHACL_VIOLATION", "{err:?}");
    }

    // ---- criterion 2: custom kinds, a mandatory standard parent --------------------------------

    #[test]
    fn add_writes_property_label_parent() {
        let mut store = load();
        let ext = extensions(&store);
        let m = relation::add(&ctx(), &mut store, "cites", "dcterms:references").unwrap();
        assert_eq!(
            m.value,
            kind("cites", "cites", &["dcterms:references"], false)
        );
        let cites = hali_term("cites");
        assert_eq!(
            changes(&m),
            [
                (
                    Op::Add,
                    format!("<{cites}> <{RDF_TYPE}> <{OWL_OBJECT_PROPERTY}> .")
                ),
                (Op::Add, format!("<{cites}> <{LABEL}> \"cites\"@en .")),
                (
                    Op::Add,
                    format!("<{cites}> <{SUBPROPERTY}> <{DCT}references> .")
                ),
            ]
        );
        let vocabulary = vocabulary_ttl(&store);
        assert!(
            vocabulary.contains("hali:cites a owl:ObjectProperty"),
            "{vocabulary}"
        );
        assert!(vocabulary.contains("\"cites\"@en"), "{vocabulary}");
        assert!(
            vocabulary.contains("rdfs:subPropertyOf dcterms:references"),
            "{vocabulary}"
        );
        assert_eq!(extensions(&store), ext);
        // It is listed, and can be related with at once.
        let page = relation::list(&ctx(), &store, 0, 50);
        assert!(
            page.results
                .contains(&kind("cites", "cites", &["dcterms:references"], false))
        );
        relation::relate(&ctx(), &mut store, R5, "cites", R2).unwrap();
    }

    #[test]
    fn add_accepts_every_standard_parent_by_any_reference() {
        for (parent, id) in [
            ("dcterms:relation", "dcterms:relation"),
            ("dcterms:isPartOf", "dcterms:isPartOf"),
            ("DCTERMS:REFERENCES", "dcterms:references"),
            ("prov:wasDerivedFrom", "prov:wasDerivedFrom"),
        ] {
            let mut store = load();
            let m = relation::add(&ctx(), &mut store, "cites", parent).unwrap();
            assert_eq!(m.value.parents, [id], "{parent}");
        }
    }

    #[test]
    fn add_uses_configured_language() {
        let mut store = load();
        let m = relation::add(&ctx_in("de"), &mut store, "zitiert", "dcterms:references").unwrap();
        assert_eq!(
            m.value,
            kind("zitiert", "zitiert", &["dcterms:references"], false)
        );
        assert!(
            changes(&m).contains(&(
                Op::Add,
                format!("<{}> <{LABEL}> \"zitiert\"@de .", hali_term("zitiert"))
            )),
            "{:?}",
            m.changes
        );
        assert!(!changes(&m).iter().any(|(_, t)| t.contains("@en")));
    }

    #[test]
    fn add_parent_must_be_standard() {
        let mut store = load();
        let before = all_quads(&store);
        // A custom kind (in the vocabulary, or only in extensions.ttl) is no parent.
        for parent in ["annotates", "summarises", "quotes"] {
            let err = refused!(
                &store,
                &before,
                relation::add(&ctx(), &mut store, "cites", parent),
            );
            usage(&err);
            assert!(
                err.message.contains(
                    "--parent must be a standard relation: dcterms:relation, dcterms:isPartOf, dcterms:references or prov:wasDerivedFrom"
                ),
                "{}",
                err.message
            );
        }
    }

    #[test]
    fn add_unknown_parent() {
        let mut store = load();
        let before = all_quads(&store);
        let err = refused!(
            &store,
            &before,
            relation::add(&ctx(), &mut store, "cites", "annotate"),
        );
        let details = unknown_relation(&err, "annotate");
        assert_eq!(
            details["suggestions"],
            json!([{"id": "annotates", "label": "annotates"}])
        );
        // A bare name never resolves to a standard kind.
        let err = refused!(
            &store,
            &before,
            relation::add(&ctx(), &mut store, "cites", "references"),
        );
        unknown_relation(&err, "references");
        let err = refused!(
            &store,
            &before,
            relation::add(&ctx(), &mut store, "cites", ""),
        );
        usage(&err);
    }

    #[test]
    fn add_name_grammar_is_usage() {
        let long = "a".repeat(65);
        for bad in [
            "",
            "Annotates",
            "1x",
            "-x",
            "_x",
            "a:b",
            "a b",
            "a.b",
            "résumé",
            " cites",
            "cites ",
            long.as_str(),
        ] {
            let mut store = load();
            let before = all_quads(&store);
            let err = refused!(
                &store,
                &before,
                relation::add(&ctx(), &mut store, bad, "dcterms:relation"),
            );
            usage(&err);
            usage(&relation::check_name(bad).unwrap_err());
        }
        // The edges that are allowed: 64 characters, then `_`, `-` and capitals after the first.
        let mut store = load();
        let longest = format!("a{}", "b".repeat(63));
        assert!(relation::valid_name(&longest));
        let m = relation::add(&ctx(), &mut store, &longest, "dcterms:relation").unwrap();
        assert_eq!(m.value.id, longest);
        let m = relation::add(&ctx(), &mut store, "a_b-C9", "dcterms:relation").unwrap();
        assert_eq!(m.value.id, "a_b-C9");
        assert!(!relation::valid_name("Annotates"));
    }

    #[test]
    fn add_usage_is_checked_before_the_parent() {
        let mut store = load();
        let before = all_quads(&store);
        let err = refused!(
            &store,
            &before,
            relation::add(&ctx(), &mut store, "A b", "nope"),
        );
        usage(&err);
    }

    #[test]
    fn add_taken_is_label_taken() {
        // A folded id (its local part, for a prefixed standard kind) or a configured-language
        // label that another kind holds; a kind that exists only in extensions.ttl holds its
        // name too.
        for (name, holder) in [
            ("references", "dcterms:references"),
            ("relation", "dcterms:relation"),
            ("ispartof", "dcterms:isPartOf"),
            ("wasderivedfrom", "prov:wasDerivedFrom"),
            ("excerptof", "excerptOf"),
            ("digestof", "digestOf"),
            ("quotes", "quotes"),
            ("sketch", "sketch"),
        ] {
            let mut store = load();
            let before = all_quads(&store);
            let err = refused!(
                &store,
                &before,
                relation::add(&ctx(), &mut store, name, "dcterms:relation"),
            );
            assert_eq!(err.rule, "LABEL_TAKEN", "{name}: {err:?}");
            assert_eq!(err.exit_code(), 1);
            assert_eq!(
                *err.details,
                json!({"label": name, "language": "en", "relation": holder})
            );
            assert_eq!(
                err.message,
                format!("label \"{name}\"@en is already used by relation kind {holder}")
            );
            assert_eq!(err.suggested_command.as_deref(), Some("hali relation list"));
            assert!(err.suggested_fix.is_some());
        }
        // The id and the label differ: either one is taken.
        let (mut store, _dir) = store_from(&[(
            "vocabulary.ttl",
            "hali:foo a owl:ObjectProperty ; rdfs:label \"bar\"@en ; rdfs:subPropertyOf dcterms:relation .\n",
        )]);
        for name in ["foo", "bar"] {
            let before = all_quads(&store);
            let err = refused!(
                &store,
                &before,
                relation::add(&ctx(), &mut store, name, "dcterms:relation"),
            );
            assert_eq!(err.rule, "LABEL_TAKEN", "{name}");
            assert_eq!(err.details["relation"], "foo");
        }
        // Another language's label does not hold the name: "annote" is annotates' French label.
        let mut store = load();
        let m = relation::add(&ctx(), &mut store, "annote", "dcterms:relation").unwrap();
        assert_eq!(m.value.id, "annote");
    }

    #[test]
    fn add_used_iri_is_usage() {
        // hali:ml is a Concept and hali:topics the scheme node.
        for name in ["ml", "topics"] {
            let mut store = load();
            let before = all_quads(&store);
            let err = refused!(
                &store,
                &before,
                relation::add(&ctx(), &mut store, name, "dcterms:relation"),
            );
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
            ("hali:x hali:p hali:objonly .\n", "objonly"),
            ("hali:x hali:predonly hali:y .\n", "predonly"),
        ] {
            let (mut store, _dir) = store_from(&[("vocabulary.ttl", turtle)]);
            let before = all_quads(&store);
            let err = refused!(
                &store,
                &before,
                relation::add(&ctx(), &mut store, name, "dcterms:relation"),
            );
            usage(&err);
        }
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
            relation::add(&ctx(), &mut store, "quotes", "nope"),
        );
        assert_eq!(err.rule, "UNKNOWN_RELATION");
        let err = refused!(
            &store,
            &before,
            relation::add(&ctx(), &mut store, "quotes", "annotates"),
        );
        usage(&err);

        let (mut store, _dir) = load_with(&[("vocabulary.ttl", BROKEN)]);
        let before = all_quads(&store);
        let err = refused!(
            &store,
            &before,
            relation::add(&ctx(), &mut store, "cites", "dcterms:relation"),
        );
        assert_eq!(err.rule, "SHACL_VIOLATION");
        let err = refused!(
            &store,
            &before,
            relation::add(&ctx(), &mut store, "quotes", "dcterms:relation"),
        );
        assert_eq!(err.rule, "LABEL_TAKEN");
        let err = refused!(
            &store,
            &before,
            relation::add(&ctx(), &mut store, "ml", "dcterms:relation"),
        );
        usage(&err);
    }

    // ---- criterion 3: relation list and remove --------------------------------------------------

    #[test]
    fn list_standard_and_custom_sorted() {
        let store = load();
        let page = relation::list(&ctx(), &store, 0, 50);
        assert_eq!(page.results, all_kinds());
        assert_eq!(page.total, 10);
        assert!(!page.truncated);
        assert_eq!(page.next_offset, None);
    }

    #[test]
    fn list_label_falls_back_to_id() {
        // In French annotates is "annote"; the other kinds have no French label and show
        // their id. Sorted by label, then id.
        let store = load();
        let page = relation::list(&ctx_in("fr"), &store, 0, 50);
        let shown: Vec<(&str, &str)> = page
            .results
            .iter()
            .map(|k| (k.id.as_str(), k.label.as_str()))
            .collect();
        assert_eq!(
            shown,
            [
                ("annotates", "annote"),
                ("dcterms:isPartOf", "dcterms:isPartOf"),
                ("dcterms:references", "dcterms:references"),
                ("dcterms:relation", "dcterms:relation"),
                ("digestOf", "digestOf"),
                ("excerptOf", "excerptOf"),
                ("prov:wasDerivedFrom", "prov:wasDerivedFrom"),
                ("quotes", "quotes"),
                ("sketch", "sketch"),
                ("summarises", "summarises"),
            ]
        );
        // The smallest label wins when a language has several.
        let (store, _dir) = store_from(&[(
            "vocabulary.ttl",
            "hali:two a owl:ObjectProperty ; rdfs:label \"Zed\"@en , \"Alpha\"@en ; \
             rdfs:subPropertyOf dcterms:relation .\n",
        )]);
        let page = relation::list(&ctx(), &store, 0, 50);
        assert!(
            page.results
                .contains(&kind("two", "Alpha", &["dcterms:relation"], false)),
            "{:?}",
            page.results
        );
    }

    #[test]
    fn list_paging() {
        let store = load();
        let page = relation::list(&ctx(), &store, 3, 3);
        let ids: Vec<&str> = page.results.iter().map(|k| k.id.as_str()).collect();
        assert_eq!(ids, ["dcterms:isPartOf", "quotes", "dcterms:references"]);
        assert_eq!(page.total, 10);
        assert!(page.truncated);
        assert_eq!(page.next_offset, Some(6));
        let page = relation::list(&ctx(), &store, 8, 5);
        let ids: Vec<&str> = page.results.iter().map(|k| k.id.as_str()).collect();
        assert_eq!(ids, ["summarises", "prov:wasDerivedFrom"]);
        assert!(!page.truncated);
        assert_eq!(page.next_offset, None);
        let page = relation::list(&ctx(), &store, 50, 5);
        assert!(page.results.is_empty());
        assert_eq!(page.total, 10);
        assert!(!page.truncated);
    }

    #[test]
    fn kinds_read_from_every_graph() {
        // `quotes` and `sketch` exist only in extensions.ttl; `chained` is defined in
        // resources.ttl, under an IRI outside hali: that is under hali:a in the vocabulary, so
        // the chain to a standard kind crosses graphs. Only an `owl:ObjectProperty` with a
        // standard ancestor is a kind. An id is the hali: local name, else prefix:local, else
        // the whole IRI.
        let (store, _dir) = store_from(&[
            (
                "vocabulary.ttl",
                "hali:a a owl:ObjectProperty ; rdfs:subPropertyOf dcterms:isPartOf .\n\
                 skos:related a owl:ObjectProperty ; rdfs:subPropertyOf dcterms:relation .\n\
                 <http://example.org/rel> a owl:ObjectProperty ; rdfs:subPropertyOf hali:a .\n\
                 hali:untyped rdfs:subPropertyOf dcterms:relation .\n\
                 hali:floating a owl:ObjectProperty .\n\
                 hali:bare a owl:ObjectProperty ; rdfs:subPropertyOf <http://example.org/prop> .\n",
            ),
            (
                "resources.ttl",
                "hali:chained a owl:ObjectProperty ; rdfs:subPropertyOf <http://example.org/rel> .\n",
            ),
        ]);
        let page = relation::list(&ctx(), &store, 0, 50);
        assert_eq!(
            page.results,
            [
                kind("a", "a", &["dcterms:isPartOf"], false),
                kind("chained", "chained", &["http://example.org/rel"], false),
                kind(
                    "http://example.org/rel",
                    "http://example.org/rel",
                    &["a"],
                    false
                ),
                kind(
                    "dcterms:isPartOf",
                    "is part of",
                    &["dcterms:relation"],
                    true
                ),
                kind(
                    "dcterms:references",
                    "references",
                    &["dcterms:relation"],
                    true
                ),
                kind("dcterms:relation", "relation", &[], true),
                kind("skos:related", "skos:related", &["dcterms:relation"], false),
                kind("prov:wasDerivedFrom", "was derived from", &[], true),
            ]
        );
        assert_eq!(page.total, 8);
        // ...and the fixture's extensions-only kinds are listed by `all_kinds`.
        let page = relation::list(&ctx(), &load(), 0, 50);
        assert!(page.results.iter().any(|k| k.id == "quotes"));
    }

    #[test]
    fn remove_used_is_in_use() {
        // annotates: used by `r3 annotates r1`, and summarises is under it.
        let mut store = load();
        let before = all_quads(&store);
        let err = refused!(
            &store,
            &before,
            relation::remove(&ctx(), &mut store, "annotates"),
        );
        assert_eq!(err.rule, "IN_USE", "{err:?}");
        assert_eq!(err.exit_code(), 1);
        assert_eq!(
            *err.details,
            json!({"relation": "annotates", "used": 1, "subproperties": 1})
        );
        // summarises: used by `r4 summarises r2`, nothing under it.
        let err = refused!(
            &store,
            &before,
            relation::remove(&ctx(), &mut store, "summarises"),
        );
        assert_eq!(err.rule, "IN_USE");
        assert_eq!(
            *err.details,
            json!({"relation": "summarises", "used": 1, "subproperties": 0})
        );
    }

    #[test]
    fn in_use_relation_error_texts() {
        let mut store = load();
        let err = relation::remove(&ctx(), &mut store, "annotates").unwrap_err();
        assert_eq!(
            err.message,
            "relation kind annotates is in use: 1 relations, parent of 1 relation kinds; nothing was written"
        );
        assert_eq!(
            err.suggested_fix.as_deref(),
            Some(
                "Remove the relations (hali unrelate <ref> annotates <ref>) and remove its sub-relations first (hali relation remove <S>), then retry."
            )
        );
        assert_eq!(err.suggested_command.as_deref(), Some("hali relation list"));
    }

    #[test]
    fn remove_blank_subproperty_counts() {
        // digestOf is unused, but a blank node in extensions.ttl is a sub-property of it.
        let mut store = load();
        let before = all_quads(&store);
        let err = refused!(
            &store,
            &before,
            relation::remove(&ctx(), &mut store, "digestOf"),
        );
        assert_eq!(err.rule, "IN_USE", "{err:?}");
        assert_eq!(
            *err.details,
            json!({"relation": "digestOf", "used": 0, "subproperties": 1})
        );
    }

    #[test]
    fn remove_used_only_in_extensions_is_in_use() {
        // `quotes` exists only in extensions.ttl and `r5 quotes r3` is only there too.
        let mut store = load();
        let before = all_quads(&store);
        let err = refused!(
            &store,
            &before,
            relation::remove(&ctx(), &mut store, "quotes"),
        );
        assert_eq!(err.rule, "IN_USE", "{err:?}");
        assert_eq!(
            *err.details,
            json!({"relation": "quotes", "used": 1, "subproperties": 0})
        );
    }

    #[test]
    fn in_use_texts_quoted() {
        // An id from the prefix table needs the quotes of a POSIX shell in the suggested fix.
        let (mut store, _dir) = store_from(&[(
            "vocabulary.ttl",
            "skos:related a owl:ObjectProperty ; rdfs:subPropertyOf dcterms:relation .\n\
             hali:sub a owl:ObjectProperty ; rdfs:subPropertyOf skos:related .\n",
        )]);
        let err = relation::remove(&ctx(), &mut store, "skos:related").unwrap_err();
        assert_eq!(err.rule, "IN_USE");
        assert_eq!(
            *err.details,
            json!({"relation": "skos:related", "used": 0, "subproperties": 1})
        );
        assert_eq!(
            err.message,
            "relation kind skos:related is in use: 0 relations, parent of 1 relation kinds; nothing was written"
        );
        assert_eq!(
            err.suggested_fix.as_deref(),
            Some(
                "Remove the relations (hali unrelate <ref> 'skos:related' <ref>) and remove its sub-relations first (hali relation remove <S>), then retry."
            )
        );
    }

    #[test]
    fn remove_standard_is_usage() {
        for reference in [
            "dcterms:relation",
            "dcterms:isPartOf",
            "dcterms:references",
            "prov:wasDerivedFrom",
            "DCTERMS:ISPARTOF",
        ] {
            let mut store = load();
            let before = all_quads(&store);
            let err = refused!(
                &store,
                &before,
                relation::remove(&ctx(), &mut store, reference),
            );
            usage(&err);
            assert!(
                err.message
                    .ends_with("is a standard relation and cannot be removed"),
                "{}",
                err.message
            );
        }
    }

    #[test]
    fn remove_unknown_is_unknown_relation() {
        let mut store = load();
        let before = all_quads(&store);
        let err = refused!(
            &store,
            &before,
            relation::remove(&ctx(), &mut store, "excerpt"),
        );
        unknown_relation(&err, "excerpt");
    }

    #[test]
    fn remove_takes_vocabulary_triples_only() {
        // excerptOf is unused. Its three vocabulary triples go, by any reference; the comment
        // that extensions.ttl holds about it stays.
        let mut store = load();
        let ext = extensions(&store);
        let m = relation::remove(&ctx(), &mut store, "EXCERPTOF").unwrap();
        assert_eq!(
            m.value,
            kind("excerptOf", "excerpt of", &["prov:wasDerivedFrom"], false)
        );
        let subject = hali_term("excerptOf");
        assert_eq!(
            changes(&m),
            [
                (
                    Op::Remove,
                    format!("<{subject}> <{RDF_TYPE}> <{OWL_OBJECT_PROPERTY}> .")
                ),
                (
                    Op::Remove,
                    format!("<{subject}> <{LABEL}> \"excerpt of\"@en .")
                ),
                (
                    Op::Remove,
                    format!("<{subject}> <{SUBPROPERTY}> <{PROV}wasDerivedFrom> .")
                ),
            ]
        );
        assert!(!vocabulary_ttl(&store).contains("excerptOf"));
        assert_eq!(extensions(&store), ext);
        assert!(
            ext.iter().any(|q| q.contains("kept in extensions")),
            "the fixture keeps a comment in extensions.ttl"
        );
        let page = relation::list(&ctx(), &store, 0, 50);
        assert!(!page.results.iter().any(|k| k.id == "excerptOf"));
        assert_eq!(page.total, 9);
    }

    #[test]
    fn remove_takes_inbound_vocabulary_triples_too() {
        // Every triple with the kind as object goes as well; here a see-also link from another
        // vocabulary entry.
        let (mut store, _dir) = load_with(&[(
            "vocabulary.ttl",
            "hali:note rdfs:seeAlso hali:excerptOf .\n",
        )]);
        let m = relation::remove(&ctx(), &mut store, "excerptOf").unwrap();
        assert_eq!(m.changes.len(), 4, "{:?}", m.changes);
        assert!(
            changes(&m).contains(&(
                Op::Remove,
                format!(
                    "<{}> <http://www.w3.org/2000/01/rdf-schema#seeAlso> <{}> .",
                    hali_term("note"),
                    hali_term("excerptOf")
                )
            )),
            "{:?}",
            m.changes
        );
    }

    #[test]
    fn remove_extensions_only_unchanged() {
        // `sketch` exists only in extensions.ttl and nothing uses it: nothing to remove.
        let mut store = load();
        let before = all_quads(&store);
        let m = relation::remove(&ctx(), &mut store, "sketch").unwrap();
        assert_eq!(
            m.value,
            kind("sketch", "sketch", &["dcterms:isPartOf"], false)
        );
        assert!(m.changes.is_empty(), "{:?}", m.changes);
        assert_eq!(all_quads(&store), before);
    }

    #[test]
    fn remove_in_use_before_shacl() {
        // A store that does not conform: IN_USE for a kind in use, SHACL_VIOLATION for one not.
        let (mut store, _dir) = load_with(&[("vocabulary.ttl", BROKEN)]);
        let before = all_quads(&store);
        let err = refused!(
            &store,
            &before,
            relation::remove(&ctx(), &mut store, "annotates"),
        );
        assert_eq!(err.rule, "IN_USE");
        let err = refused!(
            &store,
            &before,
            relation::remove(&ctx(), &mut store, "excerptOf"),
        );
        assert_eq!(err.rule, "SHACL_VIOLATION");
    }

    // ---- relation references (§9.2) -------------------------------------------------------------

    #[test]
    fn resolve_order() {
        let store = load();
        let id = |reference: &str| relation::resolve(&ctx(), &store, reference).unwrap();
        // The exact id.
        assert_eq!(id("annotates"), "annotates");
        assert_eq!(id("dcterms:isPartOf"), "dcterms:isPartOf");
        assert_eq!(id("prov:wasDerivedFrom"), "prov:wasDerivedFrom");
        assert_eq!(id("quotes"), "quotes");
        // The reference expanded through the fixed prefix table.
        assert_eq!(id("hali:annotates"), "annotates");
        assert_eq!(id("hali:quotes"), "quotes");
        // An ASCII case-insensitive id match.
        assert_eq!(id("ANNOTATES"), "annotates");
        assert_eq!(id("Excerptof"), "excerptOf");
        assert_eq!(id("dcterms:ispartof"), "dcterms:isPartOf");
        assert_eq!(id("PROV:WASDERIVEDFROM"), "prov:wasDerivedFrom");
    }

    #[test]
    fn resolve_first_in_iri_order_wins() {
        let (store, _dir) = store_from(&[(
            "vocabulary.ttl",
            "hali:Foo a owl:ObjectProperty ; rdfs:subPropertyOf dcterms:relation .\n\
             hali:foo a owl:ObjectProperty ; rdfs:subPropertyOf dcterms:relation .\n",
        )]);
        let id = |reference: &str| relation::resolve(&ctx(), &store, reference).unwrap();
        // Each exact id wins over the case-insensitive match; the capital sorts first.
        assert_eq!(id("foo"), "foo");
        assert_eq!(id("Foo"), "Foo");
        assert_eq!(id("FOO"), "Foo");
    }

    #[test]
    fn a_bare_name_never_resolves_to_a_standard_kind() {
        let store = load();
        for (reference, suggested) in [
            ("isPartOf", ("dcterms:isPartOf", "is part of")),
            ("references", ("dcterms:references", "references")),
            (
                "wasDerivedFrom",
                ("prov:wasDerivedFrom", "was derived from"),
            ),
        ] {
            let err = relation::resolve(&ctx(), &store, reference).unwrap_err();
            let details = unknown_relation(&err, reference);
            let first = &details["suggestions"][0];
            assert_eq!(first["id"], suggested.0, "{reference}: {details}");
            assert_eq!(first["label"], suggested.1, "{reference}: {details}");
        }
    }

    #[test]
    fn unknown_relation_error() {
        let store = load();
        let err = relation::resolve(&ctx(), &store, "annotate").unwrap_err();
        assert_eq!(err.rule, "UNKNOWN_RELATION");
        assert_eq!(err.exit_code(), 1);
        assert_eq!(err.message, "no relation kind matches annotate");
        assert_eq!(
            *err.details,
            json!({"relation": "annotate",
                   "suggestions": [{"id": "annotates", "label": "annotates"}]})
        );
        assert_eq!(
            err.suggested_fix.as_deref(),
            Some("Use an existing relation kind (hali relation list), or add one first.")
        );
        // The reference meets the name rules: a dry run of `relation add` is suggested.
        assert_eq!(
            err.suggested_command.as_deref(),
            Some("hali --dry-run relation add annotate --parent dcterms:relation")
        );
        // It does not: the list of kinds.
        let err = relation::resolve(&ctx(), &store, "dcterms:title").unwrap_err();
        assert_eq!(err.suggested_command.as_deref(), Some("hali relation list"));
        let err = relation::resolve(&ctx(), &store, "Note").unwrap_err();
        assert_eq!(err.suggested_command.as_deref(), Some("hali relation list"));
        // No suggestion is close.
        let err = relation::resolve(&ctx(), &store, "zzzzzzzz").unwrap_err();
        assert_eq!(
            *err.details,
            json!({"relation": "zzzzzzzz", "suggestions": []})
        );
    }

    #[test]
    fn unknown_relation_suggests_at_most_three() {
        // "re" is contained in several names: at most three, nearest first.
        let store = load();
        let err = relation::resolve(&ctx(), &store, "re").unwrap_err();
        let suggestions = err.details["suggestions"].as_array().unwrap();
        assert!(suggestions.len() <= 3, "{suggestions:?}");
        assert!(!suggestions.is_empty());
    }

    // ---- criterion 4: find --related-to and --via -----------------------------------------------

    #[test]
    fn related_to_both_directions() {
        let store = load();
        // r1: out to r2 (isPartOf); in from r3 (annotates, relation) and r4 (wasDerivedFrom).
        assert_eq!(titles(&find_related(&store, R1, None)), ["r2", "r3", "r4"]);
        // r2: in from r1 (isPartOf) and r4 (summarises, a custom kind two steps down).
        assert_eq!(titles(&find_related(&store, R2, None)), ["r1", "r4"]);
        // r4: out to r1 and r2.
        assert_eq!(titles(&find_related(&store, R4, None)), ["r1", "r2"]);
        // r3: out to r1, and in from r5 by `quotes`, which only extensions.ttl holds.
        assert_eq!(titles(&find_related(&store, R3, None)), ["r1", "r5"]);
        assert_eq!(titles(&find_related(&store, R5, None)), ["r3"]);
        // Total counts the matches.
        assert_eq!(find_related(&store, R1, None).total, 3);
    }

    #[test]
    fn related_to_excludes_self() {
        let (store, _dir) = load_with(&[(
            "resources.ttl",
            &format!("<urn:uuid:{R5}> dcterms:relation <urn:uuid:{R5}> ."),
        )]);
        assert_eq!(titles(&find_related(&store, R5, None)), ["r3"]);
        // ...and a Resource related only to itself matches nothing.
        let (store, _dir) = load_with(&[(
            "resources.ttl",
            &format!("<urn:uuid:{R2}> dcterms:relation <urn:uuid:{R2}> ."),
        )]);
        assert!(!titles(&find_related(&store, R2, None)).contains(&"r2"));
    }

    #[test]
    fn via_standard_includes_custom_subs() {
        let store = load();
        // references covers annotates, summarises (under annotates) and quotes.
        assert_eq!(
            titles(&find_related(&store, R2, Some("dcterms:references"))),
            ["r4"]
        );
        assert_eq!(
            titles(&find_related(&store, R1, Some("dcterms:references"))),
            ["r3"]
        );
        assert_eq!(
            titles(&find_related(&store, R3, Some("dcterms:references"))),
            ["r1", "r5"]
        );
        // isPartOf covers only itself here (sketch is unused).
        assert_eq!(
            titles(&find_related(&store, R1, Some("dcterms:isPartOf"))),
            ["r2"]
        );
        assert_eq!(
            titles(&find_related(&store, R2, Some("dcterms:isPartOf"))),
            ["r1"]
        );
    }

    #[test]
    fn via_relation_includes_dcmi_subs() {
        // dcterms:isPartOf and dcterms:references are sub-properties of dcterms:relation in the
        // core ontology, and annotates is under references; prov:wasDerivedFrom is not.
        let store = load();
        assert_eq!(
            titles(&find_related(&store, R1, Some("dcterms:relation"))),
            ["r2", "r3"]
        );
        assert_eq!(
            titles(&find_related(&store, R2, Some("dcterms:relation"))),
            ["r1", "r4"]
        );
        // Without --via every standard kind counts, so wasDerivedFrom matches too.
        assert_eq!(titles(&find_related(&store, R1, None)), ["r2", "r3", "r4"]);
        // The kind itself, zero steps: r4 wasDerivedFrom r1, in either direction.
        assert_eq!(
            titles(&find_related(&store, R1, Some("prov:wasDerivedFrom"))),
            ["r4"]
        );
        assert_eq!(
            titles(&find_related(&store, R4, Some("prov:wasDerivedFrom"))),
            ["r1"]
        );
    }

    #[test]
    fn via_custom_exact_and_subs() {
        let store = load();
        // annotates: itself (r3 -> r1) and summarises, which is under it (r4 -> r2).
        assert_eq!(titles(&find_related(&store, R1, Some("annotates"))), ["r3"]);
        assert_eq!(titles(&find_related(&store, R2, Some("annotates"))), ["r4"]);
        // summarises: itself only, not its parent's triples.
        assert_eq!(
            titles(&find_related(&store, R2, Some("summarises"))),
            ["r4"]
        );
        assert!(titles(&find_related(&store, R1, Some("summarises"))).is_empty());
        // By any reference form.
        assert_eq!(titles(&find_related(&store, R1, Some("ANNOTATES"))), ["r3"]);
        assert_eq!(
            titles(&find_related(&store, R1, Some("hali:annotates"))),
            ["r3"]
        );
        // A kind nothing uses.
        assert!(titles(&find_related(&store, R1, Some("excerptOf"))).is_empty());
        assert_eq!(find_related(&store, R1, Some("excerptOf")).total, 0);
    }

    #[test]
    fn via_extensions_kind() {
        let store = load();
        assert_eq!(titles(&find_related(&store, R3, Some("quotes"))), ["r5"]);
        assert_eq!(titles(&find_related(&store, R5, Some("quotes"))), ["r3"]);
        assert!(titles(&find_related(&store, R1, Some("quotes"))).is_empty());
    }

    #[test]
    fn via_cycle_terminates() {
        // loopa and loopb are sub-properties of each other, and loopa is under references.
        let (store, _dir) = load_with(&[
            (
                "vocabulary.ttl",
                "hali:loopa a owl:ObjectProperty ; rdfs:subPropertyOf hali:loopb , dcterms:references .\n\
                 hali:loopb a owl:ObjectProperty ; rdfs:subPropertyOf hali:loopa .\n",
            ),
            (
                "resources.ttl",
                &format!("<urn:uuid:{R5}> hali:loopb <urn:uuid:{R4}> ."),
            ),
        ]);
        assert_eq!(titles(&find_related(&store, R4, Some("loopa"))), ["r5"]);
        assert_eq!(titles(&find_related(&store, R4, Some("loopb"))), ["r5"]);
        assert_eq!(
            titles(&find_related(&store, R4, Some("dcterms:references"))),
            ["r2", "r5"]
        );
        assert_eq!(titles(&find_related(&store, R4, None)), ["r1", "r2", "r5"]);
    }

    #[test]
    fn related_anded_with_tag_and_type() {
        let store = load();
        let query = |tags: &[&str], types: &[&str], title: Option<&str>| Query {
            related_to: Some(R1.to_string()),
            tags: strs(tags),
            types: strs(types),
            title: title.map(str::to_string),
            ..Query::default()
        };
        let run = |q: Query| {
            let found = resource::find(&ctx(), &store, &q, |_| None).unwrap();
            titles(&found)
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>()
        };
        assert_eq!(run(query(&["ml"], &[], None)), ["r3", "r4"]);
        assert_eq!(run(query(&[], &["Note"], None)), ["r4"]);
        assert_eq!(run(query(&["ml"], &["Note"], None)), ["r4"]);
        assert_eq!(run(query(&[], &[], Some("r2"))), ["r2"]);
        assert!(run(query(&["ml"], &[], Some("r2"))).is_empty());
        // --via joins the same conjunction.
        let q = Query {
            via: Some("dcterms:references".to_string()),
            ..query(&["ml"], &[], None)
        };
        assert_eq!(run(q), ["r3"]);
    }

    #[test]
    fn related_to_unknown_is_unknown_resource() {
        let store = load();
        let query = Query {
            related_to: Some(GONE.to_string()),
            ..Query::default()
        };
        let err = resource::find(&ctx(), &store, &query, |_| None).unwrap_err();
        unknown_resource(&err, GONE);
        // It is resolved before the tags, the types and the relation.
        let query = Query {
            related_to: Some(GONE.to_string()),
            tags: strs(&["nope-tag"]),
            types: strs(&["NopeType"]),
            via: Some("nope".to_string()),
            ..Query::default()
        };
        let err = resource::find(&ctx(), &store, &query, |_| None).unwrap_err();
        unknown_resource(&err, GONE);
    }

    #[test]
    fn via_unknown_is_unknown_relation() {
        let store = load();
        let query = Query {
            related_to: Some(R1.to_string()),
            via: Some("annotate".to_string()),
            ..Query::default()
        };
        let err = resource::find(&ctx(), &store, &query, |_| None).unwrap_err();
        unknown_relation(&err, "annotate");
        // Tags, then Types, then the relation.
        let query = Query {
            related_to: Some(R1.to_string()),
            tags: strs(&["nope-tag"]),
            types: strs(&["NopeType"]),
            via: Some("nope".to_string()),
            ..Query::default()
        };
        let err = resource::find(&ctx(), &store, &query, |_| None).unwrap_err();
        assert_eq!(err.rule, "UNKNOWN_CONCEPT");
        let query = Query {
            related_to: Some(R1.to_string()),
            types: strs(&["NopeType"]),
            via: Some("nope".to_string()),
            ..Query::default()
        };
        let err = resource::find(&ctx(), &store, &query, |_| None).unwrap_err();
        assert_eq!(err.rule, "UNKNOWN_TYPE");
    }

    #[test]
    fn subproperty_path_over_fixture() {
        // The path `rdfs:subPropertyOf*` has a bound end: a triple whose predicate is the kind
        // itself matches (zero steps), as does one whose predicate is a sub-property of it, and
        // a triple with the parent as predicate does not match a sub-property.
        let (store, _dir) = load_with(&[(
            "resources.ttl",
            &format!(
                "<urn:uuid:{R5}> dcterms:references <urn:uuid:{R4}> .\n\
                 <urn:uuid:{R2}> hali:annotates <urn:uuid:{R5}> ."
            ),
        )]);
        // references: r5's direct triple, and r4 summarises r2.
        assert_eq!(
            titles(&find_related(&store, R4, Some("dcterms:references"))),
            ["r2", "r5"]
        );
        // annotates: r2 annotates r5 directly.
        assert_eq!(titles(&find_related(&store, R5, Some("annotates"))), ["r2"]);
        // ...but r5's `references` triple is no `annotates`.
        assert_eq!(titles(&find_related(&store, R4, Some("annotates"))), ["r2"]);
    }

    // ---- show: relations in both directions -----------------------------------------------------

    #[test]
    fn show_relations_both_directions_sorted() {
        let store = load();
        // r1: out (isPartOf r2); in: annotates and dcterms:relation from r3, wasDerivedFrom
        // from r4. Sorted: out before in, then relation id, then Resource id.
        assert_eq!(
            shown_relations(&store, R1),
            json!([
                entry("out", "dcterms:isPartOf", rec(2, &[], false)),
                entry("in", "annotates", rec(3, &[], true)),
                entry("in", "dcterms:relation", rec(3, &[], true)),
                entry("in", "prov:wasDerivedFrom", rec(4, &["Note"], true)),
            ])
        );
        assert_eq!(
            shown_relations(&store, R2),
            json!([
                entry("in", "dcterms:isPartOf", rec(1, &["Note"], false)),
                entry("in", "summarises", rec(4, &["Note"], true)),
            ])
        );
        assert_eq!(
            shown_relations(&store, R4),
            json!([
                entry("out", "prov:wasDerivedFrom", rec(1, &["Note"], false)),
                entry("out", "summarises", rec(2, &[], false)),
            ])
        );
        let shown = resource::show(&ctx(), &store, R1).unwrap();
        assert_eq!(shown.record.id, R1, "the record is unchanged");
    }

    #[test]
    fn show_relation_from_extensions() {
        // r3: out to r1 twice (annotates, dcterms:relation); in from r5 by `quotes`, which
        // extensions.ttl holds, with a kind that only extensions.ttl declares.
        let store = load();
        assert_eq!(
            shown_relations(&store, R3),
            json!([
                entry("out", "annotates", rec(1, &["Note"], false)),
                entry("out", "dcterms:relation", rec(1, &["Note"], false)),
                entry("in", "quotes", rec(5, &[], false)),
            ])
        );
        assert_eq!(
            shown_relations(&store, R5),
            json!([entry("out", "quotes", rec(3, &[], true))])
        );
    }

    #[test]
    fn show_lists_a_triple_once_and_a_self_relation_as_out() {
        // The same triple in two graphs is one entry; a self-relation is listed once, as `out`.
        let (store, _dir) = load_with(&[
            (
                "extensions.ttl",
                &format!("<urn:uuid:{R1}> dcterms:isPartOf <urn:uuid:{R2}> ."),
            ),
            (
                "resources.ttl",
                &format!("<urn:uuid:{R5}> dcterms:relation <urn:uuid:{R5}> ."),
            ),
        ]);
        let relations = shown_relations(&store, R1);
        let outs = relations
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["direction"] == "out")
            .count();
        assert_eq!(outs, 1, "{relations}");
        assert_eq!(
            shown_relations(&store, R5),
            json!([
                entry("out", "dcterms:relation", rec(5, &[], false)),
                entry("out", "quotes", rec(3, &[], true)),
            ])
        );
    }

    #[test]
    fn show_skips_dangling() {
        // A relation to, and one from, a node that is no cataloged Resource is not listed
        // (`validate` reports them); neither is a link by a property that is no relation kind.
        let (store, _dir) = load_with(&[(
            "resources.ttl",
            &format!(
                "<urn:uuid:{R1}> dcterms:relation <urn:uuid:{GONE}> .\n\
                 <urn:uuid:{GONE2}> dcterms:relation <urn:uuid:{R1}> .\n\
                 <urn:uuid:{R2}> <http://example.org/mentions> <urn:uuid:{R1}> ."
            ),
        )]);
        assert_eq!(
            shown_relations(&store, R1),
            json!([
                entry("out", "dcterms:isPartOf", rec(2, &[], false)),
                entry("in", "annotates", rec(3, &[], true)),
                entry("in", "dcterms:relation", rec(3, &[], true)),
                entry("in", "prov:wasDerivedFrom", rec(4, &["Note"], true)),
            ])
        );
    }

    #[test]
    fn show_without_relations_is_empty() {
        // resources.ttl alone: r5's only relation is the one extensions.ttl holds.
        let dir = tempfile::tempdir().unwrap();
        std::fs::copy(
            fixture_dir().join("resources.ttl"),
            dir.path().join("resources.ttl"),
        )
        .unwrap();
        let store = hali::storage::load(dir.path()).unwrap();
        assert_eq!(shown_relations(&store, R5), json!([]));
    }

    // ---- criterion 5: forget is blocked while Resources relate to it ----------------------------

    #[test]
    fn forget_inbound_blocked() {
        // r1: three relations (annotates and dcterms:relation from r3, wasDerivedFrom from r4)
        // from two other Resources.
        let mut store = load();
        let before = all_quads(&store);
        let err = refused!(
            &store,
            &before,
            resource::forget(&ctx(), &mut store, R1, false),
        );
        assert_eq!(err.rule, "FORGET_BLOCKED_BY_RELATIONS", "{err:?}");
        assert_eq!(err.exit_code(), 1);
        assert_eq!(
            err.message,
            format!(
                "Resource {R1} has 3 inbound relations from 2 other Resources; nothing was written"
            )
        );
        assert_eq!(
            *err.details,
            json!({"resource": R1, "relations": 3, "from": 2})
        );
        assert_eq!(
            err.suggested_fix.as_deref(),
            Some(
                format!(
                    "Remove them (hali unrelate <ref> <relation> {R1}), or forget with --force to remove them too, then retry."
                )
                .as_str()
            )
        );
        assert_eq!(
            err.suggested_command.as_deref(),
            Some(format!("hali show {R1}").as_str())
        );
        // r2: from r1 (isPartOf) and r4 (summarises).
        let err = refused!(
            &store,
            &before,
            resource::forget(&ctx(), &mut store, R2, false),
        );
        assert_eq!(err.details["relations"], 2);
        assert_eq!(err.details["from"], 2);
    }

    #[test]
    fn forget_blocked_counts_extensions_inbound() {
        // r3's only inbound relation is `r5 quotes r3`, which extensions.ttl holds.
        let mut store = load();
        let before = all_quads(&store);
        let err = refused!(
            &store,
            &before,
            resource::forget(&ctx(), &mut store, R3, false),
        );
        assert_eq!(err.rule, "FORGET_BLOCKED_BY_RELATIONS");
        assert_eq!(
            err.message,
            format!(
                "Resource {R3} has 1 inbound relations from 1 other Resources; nothing was written"
            )
        );
        assert_eq!(
            *err.details,
            json!({"resource": R3, "relations": 1, "from": 1})
        );
    }

    #[test]
    fn forget_outbound_only_ok() {
        // r5 relates to r3 (in extensions.ttl) and nothing relates to r5.
        let mut store = load();
        let ext = extensions(&store);
        let m = resource::forget(&ctx(), &mut store, R5, false).unwrap();
        assert_eq!(m.value.id, R5);
        assert!(!m.changes.is_empty());
        assert!(m.changes.iter().all(|c| c.op == Op::Remove));
        assert!(!resources_ttl(&store).contains(R5));
        assert_eq!(extensions(&store), ext, "extensions.ttl is never written");
    }

    #[test]
    fn forget_self_relation_not_blocking() {
        let (mut store, _dir) = load_with(&[(
            "resources.ttl",
            &format!("<urn:uuid:{R5}> dcterms:relation <urn:uuid:{R5}> ."),
        )]);
        let m = resource::forget(&ctx(), &mut store, R5, false).unwrap();
        assert!(m.changes.iter().all(|c| c.op == Op::Remove));
        assert!(
            m.changes
                .iter()
                .any(|c| c.triple == rel(R5, &format!("{DCT}relation"), R5)),
            "the self-relation goes with the Resource: {:?}",
            m.changes
        );
        assert!(!resources_ttl(&store).contains(R5));
    }

    #[test]
    fn forget_non_relation_links_do_not_block() {
        // A triple by a property that is no relation kind does not count.
        let (mut store, _dir) = load_with(&[(
            "resources.ttl",
            &format!("<urn:uuid:{R3}> <http://example.org/mentions> <urn:uuid:{R5}> ."),
        )]);
        let m = resource::forget(&ctx(), &mut store, R5, false).unwrap();
        assert!(m.changes.iter().all(|c| c.op == Op::Remove));
    }

    #[test]
    fn forget_force_removes_inbound() {
        let mut store = load();
        let m = resource::forget(&ctx(), &mut store, R1, true).unwrap();
        assert_eq!(m.value.id, R1);
        assert!(m.changes.iter().all(|c| c.op == Op::Remove));
        let removed: BTreeSet<String> = m.changes.iter().map(|c| c.triple.clone()).collect();
        for gone in [
            rel(R3, &hali_term("annotates"), R1),
            rel(R3, &format!("{DCT}relation"), R1),
            rel(R4, &format!("{PROV}wasDerivedFrom"), R1),
            rel(R1, &format!("{DCT}isPartOf"), R2),
        ] {
            assert!(removed.contains(&gone), "no {gone} in {removed:?}");
        }
        let resources = resources_ttl(&store);
        assert!(!resources.contains(R1), "{resources}");
        // r4 and r3 stay, with their other triples.
        assert!(holds(&store, &urn(R4), &hali_term("summarises"), &urn(R2)));
        assert!(
            hali::core::validate::validate(&store, 0, 50)
                .unwrap()
                .conforms
        );
        // Without --force the same Resource is refused, and --force on one that nothing relates
        // to changes nothing about the result.
        let mut a = load();
        let mut b = load();
        let plain = resource::forget(&ctx(), &mut a, R5, false).unwrap();
        let forced = resource::forget(&ctx(), &mut b, R5, true).unwrap();
        assert_eq!(plain, forced);
    }

    #[test]
    fn forget_force_extensions_inbound_is_shacl() {
        // `r5 quotes r3` is in extensions.ttl, which hali never writes: forgetting r3 would
        // leave a relation to a node that is no Resource, so SHACL refuses it.
        let mut store = load();
        let before = all_quads(&store);
        let err = refused!(
            &store,
            &before,
            resource::forget(&ctx(), &mut store, R3, true),
        );
        assert_eq!(err.rule, "SHACL_VIOLATION", "{err:?}");
        assert_eq!(err.exit_code(), 1);
        let focus = format!("<urn:uuid:{R5}>");
        assert!(
            err.details["violations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["focus_node"] == focus.as_str()),
            "{}",
            err.details
        );
    }

    #[test]
    fn forget_blocked_before_shacl() {
        // A store that does not conform: the relations block first; --force then meets SHACL.
        let (mut store, _dir) = load_with(&[("vocabulary.ttl", BROKEN)]);
        let before = all_quads(&store);
        let err = refused!(
            &store,
            &before,
            resource::forget(&ctx(), &mut store, R1, false),
        );
        assert_eq!(err.rule, "FORGET_BLOCKED_BY_RELATIONS");
        let err = refused!(
            &store,
            &before,
            resource::forget(&ctx(), &mut store, R1, true),
        );
        assert_eq!(err.rule, "SHACL_VIOLATION");
    }

    #[test]
    fn forget_unknown_is_checked_first() {
        let mut store = load();
        let before = all_quads(&store);
        for force in [false, true] {
            let err = refused!(
                &store,
                &before,
                resource::forget(&ctx(), &mut store, GONE, force),
            );
            unknown_resource(&err, GONE);
        }
    }
}
