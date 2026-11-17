//! Goal tests for `hali query` in `core` (#9, spec §9.9, §9.4, §3.4, §14.1).
//!
//! Assumed API (invented here; the implementer follows it). Module `hali::core::query`:
//!   `parse(text: &str) -> Result<Prepared, Error>`
//!       Parses SPARQL 1.1 Query text with the §3.4 prefix table predeclared (a query's own
//!       `PREFIX` overrides it) and no base IRI. It never sees the store. Text that parses as
//!       SPARQL Update (INSERT, DELETE, LOAD, CLEAR, DROP, CREATE, ADD, MOVE, COPY, several
//!       operations) is `Error::read_only_query()`: rule `READ_ONLY_QUERY`, exit code 1, message
//!       `hali query is read-only; SPARQL Update is refused and nothing was run`, details `{}`,
//!       a `suggested_fix`. Detection is by grammar: `INSERT` in a comment changes nothing. Text
//!       that parses as neither is a usage error (rule `USAGE`, exit code 2) whose message is
//!       `invalid SPARQL query: <parser message>`, the parser message capped at 1,000 characters.
//!       `Prepared` is opaque; it need not be `Debug`.
//!   `run(store: &Store, query: &Prepared, limits: Limits) -> Result<Answer, Error>`
//!       Evaluates over `Store` (`hali::storage::Store`, one named graph per source). The default
//!       graph is the RDF merge (set union) of the four named graphs
//!       `https://w3id.org/hali/graph/{core,vocabulary,resources,extensions}`: a triple held in
//!       two graphs matches once. `GRAPH` works with those IRIs; `FROM` and `FROM NAMED` choose
//!       among them and are never fetched. It builds its evaluator with
//!       `SparqlEvaluator::without_default_http_service_handler()`, so it never opens a network
//!       connection: a non-silent `SERVICE` is `Error::query_failed("service", Some(iri))`
//!       (rule `QUERY_FAILED`, exit code 1, details `{"reason":"service","service":"<iri>"}`),
//!       and `SERVICE SILENT` follows SPARQL's silent semantics (no rows from the service).
//!       It never writes to the store.
//!   `Limits { timeout: Duration, max_results: usize }` `Clone + Copy + Debug + PartialEq`,
//!       with `Limits::default()` = 30 seconds and 100,000 (the constants the CLI uses). A query
//!       still running after `timeout` is cancelled: `QUERY_FAILED`, details reason `timeout`. More
//!       than `max_results` solutions (SELECT) or triples (CONSTRUCT, DESCRIBE) is `QUERY_FAILED`,
//!       details reason `too_large`, with a `suggested_fix` that says to add `LIMIT`; exactly
//!       `max_results` is fine, and an ASK is never too large.
//!   `Answer` `Clone + Debug + PartialEq`:
//!       `Solutions { variables: Vec<String>, rows: Vec<Vec<Option<oxrdf::Term>>> }`
//!           variable names without `?` in query order; each row has one cell per variable, in
//!           the same order, `None` for an unbound variable; rows in the order the query gives.
//!       `Boolean(bool)`
//!       `Triples(Vec<oxrdf::Triple>)` (CONSTRUCT and DESCRIBE, in any order; the CLI sorts).
//!   `Error` is `hali::core::Error` (`rule`, `message`, `details`, `suggested_fix`, `exit_code()`).
//!       The constructors `Error::read_only_query()` and `Error::query_failed(reason, service)` are
//!       the implementer's; these tests only look at the errors that `parse` and `run` return.
//!
//! The tests read tests/fixtures/relations (5 Resources, Concept `ml`), the embedded core alone
//! (a data dir that does not exist) and stores built in a temp dir. Every SELECT has an ORDER BY.

#[cfg(feature = "goal")]
mod goal {
    use hali::core::Error;
    use hali::core::query::{Answer, Limits, Prepared, parse, run};
    use hali::storage::Store;
    use oxrdf::{Literal, NamedNode, Term, Triple};
    use serde_json::json;
    use std::collections::BTreeSet;
    use std::net::TcpListener;
    use std::path::{Path, PathBuf};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    const GRAPH: &str = "https://w3id.org/hali/graph";
    const READ_ONLY_MESSAGE: &str =
        "hali query is read-only; SPARQL Update is refused and nothing was run";

    fn limits() -> Limits {
        Limits::default()
    }

    fn fixture_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/relations")
    }

    fn load() -> Store {
        hali::storage::load(&fixture_dir()).unwrap()
    }

    /// The embedded core ontology and shapes alone: the data dir does not exist.
    fn core_only() -> Store {
        let dir = tempfile::tempdir().unwrap();
        hali::storage::load(&dir.path().join("absent")).unwrap()
    }

    /// A store whose `vocabulary.ttl`, `resources.ttl` and `extensions.ttl` hold the given
    /// Turtle text (full IRIs only); the temp dir is kept alive with it.
    fn load_files(
        vocabulary: &str,
        resources: &str,
        extensions: &str,
    ) -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        for (name, text) in [
            ("vocabulary.ttl", vocabulary),
            ("resources.ttl", resources),
            ("extensions.ttl", extensions),
        ] {
            std::fs::write(dir.path().join(name), text).unwrap();
        }
        (hali::storage::load(dir.path()).unwrap(), dir)
    }

    fn prepared(text: &str) -> Prepared {
        match parse(text) {
            Ok(p) => p,
            Err(e) => panic!("`{text}` should parse, got {}: {}", e.rule, e.message),
        }
    }

    fn parse_err(text: &str) -> Error {
        match parse(text) {
            Err(e) => e,
            Ok(_) => panic!("`{text}` should not parse as a query"),
        }
    }

    fn answer_with(store: &Store, text: &str, limits: Limits) -> Result<Answer, Error> {
        run(store, &prepared(text), limits)
    }

    fn answer(store: &Store, text: &str) -> Answer {
        match answer_with(store, text, limits()) {
            Ok(a) => a,
            Err(e) => panic!("`{text}` failed: {} {} {}", e.rule, e.message, e.details),
        }
    }

    fn failure(store: &Store, text: &str, limits: Limits) -> Error {
        match answer_with(store, text, limits) {
            Err(e) => e,
            Ok(a) => panic!("`{text}` should fail, got {a:?}"),
        }
    }

    /// Runs `f` on another thread and fails, instead of hanging, if it takes longer than `secs`.
    fn within<T: Send + 'static>(secs: u64, f: impl FnOnce() -> T + Send + 'static) -> T {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(f());
        });
        rx.recv_timeout(Duration::from_secs(secs))
            .unwrap_or_else(|_| panic!("no answer within {secs} s: the query was not bounded"))
    }

    fn iri(text: &str) -> Term {
        Term::NamedNode(NamedNode::new(text).unwrap())
    }

    fn lang(text: &str, tag: &str) -> Term {
        Term::Literal(Literal::new_language_tagged_literal(text, tag).unwrap())
    }

    fn triple(s: &str, p: &str, o: Term) -> Triple {
        Triple::new(NamedNode::new(s).unwrap(), NamedNode::new(p).unwrap(), o)
    }

    fn rows(a: Answer) -> (Vec<String>, Vec<Vec<Option<Term>>>) {
        match a {
            Answer::Solutions { variables, rows } => (variables, rows),
            other => panic!("expected solutions, got {other:?}"),
        }
    }

    fn triples(a: Answer) -> BTreeSet<String> {
        match a {
            Answer::Triples(t) => t.iter().map(ToString::to_string).collect(),
            other => panic!("expected triples, got {other:?}"),
        }
    }

    fn column(a: Answer) -> Vec<String> {
        rows(a)
            .1
            .into_iter()
            .map(|row| {
                assert_eq!(row.len(), 1, "one cell per row");
                match &row[0] {
                    Some(Term::NamedNode(n)) => n.as_str().to_string(),
                    other => panic!("expected an IRI, got {other:?}"),
                }
            })
            .collect()
    }

    fn uuid(n: u32) -> String {
        format!("urn:uuid:0192f0c4-0000-7000-8000-000000000{n}")
    }

    // ---- criterion 3: SELECT, ASK, CONSTRUCT and DESCRIBE each evaluate ------------------------

    #[test]
    fn select() {
        let store = load();
        let a = answer(
            &store,
            "SELECT ?id WHERE { ?id a hali:Resource } ORDER BY ?id",
        );
        let (variables, got) = rows(a.clone());
        assert_eq!(variables, ["id"]);
        let want: Vec<Vec<Option<Term>>> = (701..=705).map(|n| vec![Some(iri(&uuid(n)))]).collect();
        assert_eq!(got, want);
        assert_eq!(column(a), (701..=705).map(uuid).collect::<Vec<_>>());
    }

    #[test]
    fn select_cells_and_unbound() {
        let store = core_only();
        // Two variables, in query order; an OPTIONAL that matches nothing leaves `None`.
        let a = answer(
            &store,
            "SELECT ?label ?r ?note WHERE { VALUES ?r { dcterms:relation } \
             ?r rdfs:label ?label OPTIONAL { ?r rdfs:comment ?note } }",
        );
        let (variables, got) = rows(a);
        assert_eq!(variables, ["label", "r", "note"]);
        assert_eq!(
            got,
            vec![vec![
                Some(lang("relation", "en")),
                Some(iri("http://purl.org/dc/terms/relation")),
                None
            ]]
        );
        // `SELECT *` lists the variables of the pattern, and a query with no match has no rows.
        let (variables, got) = rows(answer(
            &store,
            "SELECT * WHERE { ?x a <urn:x:nothing> } ORDER BY ?x",
        ));
        assert_eq!(variables, ["x"]);
        assert!(got.is_empty());
    }

    #[test]
    fn select_uses_a_query_prefix_over_the_table() {
        let store = core_only();
        // `dcterms` is predeclared; a query's own PREFIX overrides it.
        let a = answer(
            &store,
            "PREFIX dcterms: <http://example.org/> \
             SELECT ?x WHERE { VALUES ?x { dcterms:relation } } ORDER BY ?x",
        );
        assert_eq!(column(a), ["http://example.org/relation"]);
        let a = answer(
            &store,
            "SELECT ?x WHERE { VALUES ?x { dcterms:relation skos:Concept hali:ml foaf:name \
             spdx:checksum sh:NodeShape dcat:Distribution prov:Entity owl:Class rdf:type \
             rdfs:label xsd:string } } ORDER BY ?x",
        );
        assert_eq!(column(a).len(), 12);
    }

    #[test]
    fn ask() {
        let store = load();
        assert_eq!(
            answer(&store, "ASK { hali:ml a skos:Concept }"),
            Answer::Boolean(true)
        );
        assert_eq!(
            answer(&store, "ASK { hali:nonesuch a skos:Concept }"),
            Answer::Boolean(false)
        );
        // The core graph is part of the default graph.
        assert_eq!(
            answer(&store, "ASK { dcterms:relation a owl:ObjectProperty }"),
            Answer::Boolean(true)
        );
    }

    #[test]
    fn construct() {
        let store = core_only();
        let a = answer(
            &store,
            "CONSTRUCT { ?r rdfs:label ?l } WHERE { ?r a owl:ObjectProperty ; rdfs:label ?l }",
        );
        let want: BTreeSet<String> = [
            triple(
                "http://purl.org/dc/terms/isPartOf",
                "http://www.w3.org/2000/01/rdf-schema#label",
                lang("is part of", "en"),
            ),
            triple(
                "http://purl.org/dc/terms/references",
                "http://www.w3.org/2000/01/rdf-schema#label",
                lang("references", "en"),
            ),
            triple(
                "http://purl.org/dc/terms/relation",
                "http://www.w3.org/2000/01/rdf-schema#label",
                lang("relation", "en"),
            ),
            triple(
                "http://www.w3.org/ns/prov#wasDerivedFrom",
                "http://www.w3.org/2000/01/rdf-schema#label",
                lang("was derived from", "en"),
            ),
        ]
        .iter()
        .map(ToString::to_string)
        .collect();
        assert_eq!(triples(a), want);
        // A CONSTRUCT that matches nothing is an empty set of triples, not an error.
        assert_eq!(
            triples(answer(
                &store,
                "CONSTRUCT { ?s ?p ?o } WHERE { ?s a <urn:x:nothing> ; ?p ?o }"
            )),
            BTreeSet::new()
        );
    }

    #[test]
    fn construct_template_blank_nodes_are_fresh() {
        let store = core_only();
        let a = answer(
            &store,
            "CONSTRUCT { _:b <urn:x:about> ?r } WHERE { ?r a owl:ObjectProperty }",
        );
        match a {
            Answer::Triples(t) => {
                assert_eq!(t.len(), 4);
                let subjects: BTreeSet<String> = t.iter().map(|x| x.subject.to_string()).collect();
                assert_eq!(subjects.len(), 4, "one fresh blank node per solution");
                assert!(subjects.iter().all(|s| s.starts_with("_:")));
            }
            other => panic!("expected triples, got {other:?}"),
        }
    }

    #[test]
    fn describe() {
        let store = load();
        let t = triples(answer(&store, "DESCRIBE hali:ml"));
        let pref = triple(
            "https://w3id.org/hali#ml",
            "http://www.w3.org/2004/02/skos/core#prefLabel",
            lang("ml", "en"),
        )
        .to_string();
        let kind = triple(
            "https://w3id.org/hali#ml",
            "http://www.w3.org/1999/02/22-rdf-syntax-ns#type",
            iri("http://www.w3.org/2004/02/skos/core#Concept"),
        )
        .to_string();
        assert!(t.contains(&pref), "prefLabel missing from {t:?}");
        assert!(t.contains(&kind), "rdf:type missing from {t:?}");
        // Describing an IRI the store has never heard of gives no triples, not an error.
        assert!(triples(answer(&store, "DESCRIBE <urn:x:nothing>")).is_empty());
    }

    #[test]
    fn syntax_errors_are_usage_errors() {
        for text in [
            "SELEKT ?x WHERE { ?x ?p ?o }",
            "SELECT ?x WHERE {",
            "SELECT",
            "not sparql at all",
            // Looks like the start of an update but is neither a query nor a valid update.
            "INSERT DATA {",
            "ASK { undeclared:x a ?y }",
        ] {
            let e = parse_err(text);
            assert_eq!(e.rule, "USAGE", "{text}");
            assert_eq!(e.exit_code(), 2, "{text}");
            assert!(
                e.message.starts_with("invalid SPARQL query: "),
                "{text}: {}",
                e.message
            );
        }
        // A huge bad query gives a bounded message: 1,000 characters of parser message at most.
        let e = parse_err(&format!(
            "SELECT ?x WHERE {{ {} }}",
            "?x ?x ?x . ".repeat(5_000) + "@@"
        ));
        assert_eq!(e.rule, "USAGE");
        assert!(
            e.message.chars().count() <= "invalid SPARQL query: ".len() + 1_000,
            "{} characters",
            e.message.chars().count()
        );
    }

    // ---- criterion 7: the dataset ----------------------------------------------------------------

    /// `s1 p "both"` is in vocabulary.ttl and extensions.ttl; `s2 p "vocab"` only in the first,
    /// `s3 p "ext"` only in the second, `s4 p "res"` only in resources.ttl.
    fn two_graphs() -> (Store, tempfile::TempDir) {
        load_files(
            "<urn:x:s1> <urn:x:p> \"both\" .\n<urn:x:s2> <urn:x:p> \"vocab\" .\n",
            "<urn:x:s4> <urn:x:p> \"res\" .\n",
            "<urn:x:s1> <urn:x:p> \"both\" .\n<urn:x:s3> <urn:x:p> \"ext\" .\n",
        )
    }

    fn strings(a: Answer) -> Vec<String> {
        rows(a)
            .1
            .into_iter()
            .map(|row| match &row[0] {
                Some(Term::Literal(l)) => l.value().to_string(),
                Some(Term::NamedNode(n)) => n.as_str().to_string(),
                other => panic!("unexpected cell {other:?}"),
            })
            .collect()
    }

    #[test]
    fn dataset_merge() {
        let (store, _dir) = two_graphs();
        // The default graph is a set merge: the triple both graphs hold matches once.
        let a = answer(
            &store,
            "SELECT ?o WHERE { <urn:x:s1> <urn:x:p> ?o } ORDER BY ?o",
        );
        assert_eq!(strings(a), ["both"]);
        // Every graph contributes: vocabulary, resources and extensions, and the core graph.
        let a = answer(&store, "SELECT ?o WHERE { ?s <urn:x:p> ?o } ORDER BY ?o");
        assert_eq!(strings(a), ["both", "ext", "res", "vocab"]);
        let (_, got) = rows(answer(
            &store,
            "SELECT (COUNT(*) AS ?n) WHERE { <urn:x:s1> <urn:x:p> ?o }",
        ));
        assert_eq!(
            got,
            vec![vec![Some(Term::Literal(Literal::new_typed_literal(
                "1",
                NamedNode::new("http://www.w3.org/2001/XMLSchema#integer").unwrap()
            )))]]
        );
        assert_eq!(
            answer(&store, "ASK { dcterms:relation a owl:ObjectProperty }"),
            Answer::Boolean(true)
        );
        // A triple pattern with no variables sees the merge as well.
        assert_eq!(
            answer(&store, "ASK { <urn:x:s3> <urn:x:p> \"ext\" }"),
            Answer::Boolean(true)
        );
    }

    #[test]
    fn graph_and_from() {
        let (store, _dir) = two_graphs();
        let ext = format!("{GRAPH}/extensions");
        let voc = format!("{GRAPH}/vocabulary");
        let core = format!("{GRAPH}/core");
        let res = format!("{GRAPH}/resources");

        // GRAPH sees one named graph.
        let a = answer(
            &store,
            &format!("SELECT ?o WHERE {{ GRAPH <{ext}> {{ ?s <urn:x:p> ?o }} }} ORDER BY ?o"),
        );
        assert_eq!(strings(a), ["both", "ext"]);
        let a = answer(
            &store,
            &format!("SELECT ?o WHERE {{ GRAPH <{res}> {{ ?s <urn:x:p> ?o }} }} ORDER BY ?o"),
        );
        assert_eq!(strings(a), ["res"]);
        // GRAPH ?g lists the graphs that hold a triple.
        let a = answer(
            &store,
            "SELECT ?g WHERE { GRAPH ?g { <urn:x:s1> <urn:x:p> ?o } } ORDER BY ?g",
        );
        assert_eq!(strings(a), [ext.clone(), voc.clone()]);
        // The core graph is a named graph too.
        assert_eq!(
            answer(
                &store,
                &format!("ASK {{ GRAPH <{core}> {{ dcterms:relation a owl:ObjectProperty }} }}")
            ),
            Answer::Boolean(true)
        );

        // FROM restricts the default graph (the merge of the graphs named).
        let a = answer(
            &store,
            &format!("SELECT ?o FROM <{ext}> WHERE {{ ?s <urn:x:p> ?o }} ORDER BY ?o"),
        );
        assert_eq!(strings(a), ["both", "ext"]);
        let a = answer(
            &store,
            &format!("SELECT ?o FROM <{ext}> FROM <{res}> WHERE {{ ?s <urn:x:p> ?o }} ORDER BY ?o"),
        );
        assert_eq!(strings(a), ["both", "ext", "res"]);
        // ...so what only another graph holds is not there, the core graph included.
        assert_eq!(
            answer(
                &store,
                &format!("ASK FROM <{ext}> {{ dcterms:relation a owl:ObjectProperty }}")
            ),
            Answer::Boolean(false)
        );
        assert_eq!(
            answer(&store, &format!("ASK FROM <{core}> {{ ?s <urn:x:p> ?o }}")),
            Answer::Boolean(false)
        );
        // FROM NAMED restricts which graphs GRAPH ?g ranges over.
        let a = answer(
            &store,
            &format!(
                "SELECT ?g FROM NAMED <{voc}> WHERE {{ GRAPH ?g {{ <urn:x:s1> <urn:x:p> ?o }} }} ORDER BY ?g"
            ),
        );
        assert_eq!(strings(a), [voc]);
    }

    // ---- criterion 8: Update is refused -----------------------------------------------------------

    #[test]
    fn update_refused() {
        let graph = format!("{GRAPH}/extensions");
        let updates = [
            "INSERT DATA { <urn:x:a> <urn:x:b> <urn:x:c> }".to_string(),
            "DELETE WHERE { ?s ?p ?o }".to_string(),
            "DELETE DATA { <urn:x:a> <urn:x:b> <urn:x:c> }".to_string(),
            "DELETE { ?s ?p ?o } INSERT { ?s ?p 1 } WHERE { ?s ?p ?o }".to_string(),
            format!("WITH <{graph}> DELETE {{ ?s ?p ?o }} WHERE {{ ?s ?p ?o }}"),
            "LOAD <file:///etc/passwd>".to_string(),
            "LOAD SILENT <http://127.0.0.1:9/> INTO GRAPH <urn:x:g>".to_string(),
            "CLEAR ALL".to_string(),
            "CLEAR DEFAULT".to_string(),
            format!("DROP GRAPH <{graph}>"),
            "DROP ALL".to_string(),
            "CREATE GRAPH <urn:x:g>".to_string(),
            format!("ADD <{graph}> TO DEFAULT"),
            format!("MOVE DEFAULT TO <{graph}>"),
            format!("COPY <{graph}> TO <urn:x:g>"),
            "INSERT DATA { <urn:x:a> <urn:x:b> <urn:x:c> } ; DELETE WHERE { ?s ?p ?o }".to_string(),
            "CLEAR ALL ; CLEAR ALL".to_string(),
            "PREFIX x: <urn:x:> INSERT DATA { x:a x:b x:c }".to_string(),
            "# a comment first\nINSERT DATA { <urn:x:a> <urn:x:b> <urn:x:c> }".to_string(),
        ];
        let store = load();
        let before = hali::storage::to_turtle(&store, hali::storage::File::Resources);
        for text in updates {
            let e = parse_err(&text);
            assert_eq!(e.rule, "READ_ONLY_QUERY", "{text}");
            assert_eq!(e.exit_code(), 1, "{text}");
            assert_eq!(e.message, READ_ONLY_MESSAGE, "{text}");
            assert_eq!(*e.details, json!({}), "{text}");
            assert!(
                e.suggested_fix
                    .as_deref()
                    .is_some_and(|f| f.contains("SELECT")),
                "{text}: the fix points to the read forms: {:?}",
                e.suggested_fix
            );
        }
        // Nothing parsed an update into the store.
        assert_eq!(
            hali::storage::to_turtle(&store, hali::storage::File::Resources),
            before
        );
    }

    #[test]
    fn a_comment_is_not_an_update() {
        let store = core_only();
        // `INSERT` in a comment, in a string and in an IRI changes nothing: the grammar decides.
        for text in [
            "# INSERT DATA {}\nASK {}",
            "ASK { } # DELETE WHERE { ?s ?p ?o }",
            "ASK { ?s ?p \"INSERT DATA {}\" }",
            "ASK { <urn:x:DROP> ?p ?o }",
        ] {
            let a = answer(&store, text);
            assert!(matches!(a, Answer::Boolean(_)), "{text}: {a:?}");
        }
        assert_eq!(
            answer(&store, "# INSERT DATA {}\nASK {}"),
            Answer::Boolean(true)
        );
    }

    // ---- criterion 11: SERVICE never reaches the network ------------------------------------------

    fn listener() -> (TcpListener, u16) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.set_nonblocking(true).unwrap();
        let port = l.local_addr().unwrap().port();
        (l, port)
    }

    fn no_connection(l: &TcpListener) {
        match l.accept() {
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => panic!("accept failed: {e}"),
            Ok((_, from)) => panic!("hali connected to the listener (from {from})"),
        }
    }

    #[test]
    fn service_offline() {
        let (l, port) = listener();
        let service = format!("http://127.0.0.1:{port}/");

        // A non-silent SERVICE is QUERY_FAILED naming the service; nothing connects.
        let text = format!("SELECT * WHERE {{ SERVICE <{service}> {{ ?s ?p ?o }} }} ORDER BY ?s");
        let e = within(20, move || failure(&core_only(), &text, Limits::default()));
        assert_eq!(e.rule, "QUERY_FAILED");
        assert_eq!(e.exit_code(), 1);
        assert_eq!(*e.details, json!({"reason": "service", "service": service}));
        no_connection(&l);

        // The same inside ASK, CONSTRUCT and a group that also matches local data.
        for text in [
            format!("ASK {{ SERVICE <{service}> {{ ?s ?p ?o }} }}"),
            format!("CONSTRUCT {{ ?s ?p ?o }} WHERE {{ SERVICE <{service}> {{ ?s ?p ?o }} }}"),
            format!(
                "SELECT ?x WHERE {{ ?x a owl:ObjectProperty . SERVICE <{service}> {{ ?x ?p ?o }} }} ORDER BY ?x"
            ),
        ] {
            let t = text.clone();
            let e = within(20, move || failure(&core_only(), &t, Limits::default()));
            assert_eq!(e.rule, "QUERY_FAILED", "{text}");
            assert_eq!(e.details["reason"], "service", "{text}");
            assert_eq!(e.details["service"], service, "{text}");
        }
        no_connection(&l);

        // SERVICE SILENT follows SPARQL's silent semantics: it succeeds with nothing from the
        // service, and still never connects.
        let text = format!(
            "SELECT ?x ?o WHERE {{ ?x a owl:ObjectProperty . \
             OPTIONAL {{ SERVICE SILENT <{service}> {{ ?x <urn:x:p> ?o }} }} }} ORDER BY ?x"
        );
        let a = within(20, move || answer(&core_only(), &text));
        let (variables, got) = rows(a);
        assert_eq!(variables, ["x", "o"]);
        assert_eq!(got.len(), 4, "the four object properties, each without ?o");
        assert!(got.iter().all(|row| row[1].is_none()), "{got:?}");
        let text = format!("ASK {{ SERVICE SILENT <{service}> {{ ?s ?p ?o }} }}");
        let a = within(20, move || answer(&core_only(), &text));
        assert!(matches!(a, Answer::Boolean(_)));
        no_connection(&l);

        // FROM and FROM NAMED name a graph; they are never fetched.
        for text in [
            format!("SELECT ?s FROM <{service}> WHERE {{ ?s ?p ?o }} ORDER BY ?s"),
            format!(
                "SELECT ?s FROM NAMED <{service}> WHERE {{ GRAPH ?g {{ ?s ?p ?o }} }} ORDER BY ?s"
            ),
        ] {
            let t = text.clone();
            let a = within(20, move || answer(&core_only(), &t));
            let (_, got) = rows(a);
            assert!(got.is_empty(), "{text}: an unknown graph is empty");
        }
        no_connection(&l);

        // LOAD is an update: refused at parse time, so it cannot fetch either.
        let e = parse_err(&format!("LOAD <{service}>"));
        assert_eq!(e.rule, "READ_ONLY_QUERY");
        no_connection(&l);
    }

    // ---- criterion 12: bounds ---------------------------------------------------------------------

    #[test]
    fn default_limits() {
        let d = Limits::default();
        assert_eq!(d.timeout, Duration::from_secs(30));
        assert_eq!(d.max_results, 100_000);
        assert_eq!(
            d,
            Limits {
                timeout: Duration::from_secs(30),
                max_results: 100_000
            }
        );
    }

    #[test]
    fn timeout() {
        // Three unconnected patterns over the fixture, and for each of their millions of
        // combinations a scan of the whole store (the NOT EXISTS): far more than a second of
        // work that reads the store all the time, so the query only ends by being cancelled.
        let text = "SELECT (COUNT(*) AS ?n) WHERE { ?a ?b ?c . ?d ?e ?f . ?g ?h ?i . \
            FILTER NOT EXISTS { ?x ?y ?z . FILTER(?x = ?a && ?y = ?e && ?z = ?i) } }";
        let started = Instant::now();
        let e = within(60, move || {
            failure(
                &load(),
                text,
                Limits {
                    timeout: Duration::from_millis(100),
                    max_results: 100_000,
                },
            )
        });
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(e.rule, "QUERY_FAILED");
        assert_eq!(e.exit_code(), 1);
        assert_eq!(e.details["reason"], "timeout");
        // A query that fits in the time limit is not touched by it.
        let a = within(60, || {
            answer_with(
                &load(),
                "ASK { hali:ml a skos:Concept }",
                Limits {
                    timeout: Duration::from_secs(30),
                    max_results: 100_000,
                },
            )
        });
        assert_eq!(a.unwrap(), Answer::Boolean(true));
    }

    #[test]
    fn too_large() {
        let store = core_only();
        let at = |max_results| Limits {
            timeout: Duration::from_secs(30),
            max_results,
        };
        let select = "SELECT ?r WHERE { ?r a owl:ObjectProperty } ORDER BY ?r";
        // Four solutions: three is too few, four is exactly enough.
        let e = failure(&store, select, at(3));
        assert_eq!(e.rule, "QUERY_FAILED");
        assert_eq!(e.exit_code(), 1);
        assert_eq!(e.details["reason"], "too_large");
        assert!(
            e.suggested_fix
                .as_deref()
                .is_some_and(|f| f.contains("LIMIT")),
            "{:?}",
            e.suggested_fix
        );
        assert_eq!(rows(answer_with(&store, select, at(4)).unwrap()).1.len(), 4);
        // The user's own LIMIT keeps it under the cap.
        let limited = "SELECT ?r WHERE { ?r a owl:ObjectProperty } ORDER BY ?r LIMIT 3";
        assert_eq!(
            rows(answer_with(&store, limited, at(3)).unwrap()).1.len(),
            3
        );
        // Triples count the same way.
        let construct =
            "CONSTRUCT { ?r rdfs:label ?l } WHERE { ?r a owl:ObjectProperty ; rdfs:label ?l }";
        let e = failure(&store, construct, at(3));
        assert_eq!(e.rule, "QUERY_FAILED");
        assert_eq!(e.details["reason"], "too_large");
        assert_eq!(
            triples(answer_with(&store, construct, at(4)).unwrap()).len(),
            4
        );
        let e = failure(&store, "DESCRIBE dcterms:relation", at(1));
        assert_eq!(e.details["reason"], "too_large");
        // An ASK has one answer, whatever the cap.
        assert_eq!(
            answer_with(&store, "ASK { ?s ?p ?o }", at(0)).unwrap(),
            Answer::Boolean(true)
        );
    }

    // ---- rows 21-25 (review of #9) ----------------------------------------------------------------

    fn at_100ms() -> Limits {
        Limits {
            timeout: Duration::from_millis(100),
            max_results: 100_000,
        }
    }

    /// `n` cross-joined `VALUES` of ten rows each, starting at `?v0`: 10^n solutions, none of
    /// them read from the store.
    fn values(n: usize) -> String {
        (0..n)
            .map(|i| format!("VALUES ?v{i} {{ 1 2 3 4 5 6 7 8 9 10 }}"))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Row 21: the time limit holds for a query that never reads the store (a billion
    /// solutions, no scan), not only for store scans.
    #[test]
    fn timeout_covers_a_query_that_never_reads_the_store() {
        let text = format!("SELECT (COUNT(*) AS ?c) WHERE {{ {} }}", values(9));
        let started = Instant::now();
        let e = within(30, move || failure(&core_only(), &text, at_100ms()));
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(e.rule, "QUERY_FAILED");
        assert_eq!(e.exit_code(), 1);
        assert_eq!(e.details["reason"], "timeout");
    }

    /// Row 21 (security F9): the sort phase of ORDER BY is cancelled too.
    #[test]
    fn timeout_covers_the_sort_of_a_cross_join() {
        let text = format!(
            "SELECT ?v0 WHERE {{ {} }} ORDER BY DESC(?v0 * ?v1 + ?v2 - ?v3 * ?v4 + ?v5)",
            values(6)
        );
        let started = Instant::now();
        let result = within(30, move || {
            answer_with(
                &core_only(),
                &text,
                Limits {
                    timeout: Duration::from_millis(100),
                    max_results: usize::MAX,
                },
            )
        });
        let e = match result {
            Err(e) => e,
            Ok(_) => panic!("a million-row sort ran to the end although the limit is 100 ms"),
        };
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(e.rule, "QUERY_FAILED");
        assert_eq!(e.details["reason"], "timeout");
    }

    /// Row 22: an evaluation error the user's text causes (a custom function nothing defines)
    /// is a usage error naming the function, never STORE_IO.
    #[test]
    fn unsupported_custom_functions_are_usage_errors() {
        let store = load();
        for (text, name) in [
            (
                "SELECT * WHERE { ?x <http://x/p> ?y . FILTER(<http://x/f>(?y)) }",
                "http://x/f",
            ),
            ("SELECT (<urn:f>(1) AS ?y) {}", "urn:f"),
            ("SELECT (<urn:f>() AS ?y) {}", "urn:f"),
            ("SELECT (<urn:f>(1, 2, 3) AS ?y) {}", "urn:f"),
            ("ASK { FILTER(<urn:f>(1)) }", "urn:f"),
        ] {
            let e = match run(&store, &prepared(text), limits()) {
                Err(e) => e,
                Ok(a) => panic!("`{text}` should fail, got {a:?}"),
            };
            assert_ne!(e.rule, "STORE_IO", "{text}: {}", e.message);
            assert_eq!(e.rule, "USAGE", "{text}: {}", e.message);
            assert_eq!(e.exit_code(), 2, "{text}");
            assert!(
                e.message.starts_with("invalid SPARQL query: "),
                "{text}: {}",
                e.message
            );
            assert!(e.message.contains(name), "{text}: {}", e.message);
        }
    }

    /// Row 24: nothing to query is a usage error; READ_ONLY_QUERY stays for real updates.
    #[test]
    fn empty_text_is_a_usage_error() {
        for text in [
            "",
            "   \n\t",
            "# only a comment",
            "# a\n# b\n",
            "PREFIX ex: <x:>",
        ] {
            let e = parse_err(text);
            assert_eq!(e.rule, "USAGE", "{text:?}: {}", e.message);
            assert_eq!(e.exit_code(), 2, "{text:?}");
            assert!(
                e.message.starts_with("invalid SPARQL query: "),
                "{text:?}: {}",
                e.message
            );
        }
        assert_eq!(parse_err("INSERT DATA {}").rule, "READ_ONLY_QUERY");
    }

    /// Row 25 (F5): `SERVICE ?s` that is bound to an IRI, bound to a non-IRI, or unbound is
    /// QUERY_FAILED service, never store_io, and never a connection.
    #[test]
    fn service_variables_are_service_failures() {
        let store = core_only();
        for text in [
            "SELECT * WHERE { BIND(<urn:x:s> AS ?s) SERVICE ?s { ?a ?b ?c } }",
            "SELECT * WHERE { BIND(\"not an iri\" AS ?s) SERVICE ?s { ?a ?b ?c } }",
            "SELECT * WHERE { SERVICE ?s { ?a ?b ?c } }",
            "ASK { SERVICE ?s {} }",
            "SELECT * WHERE { VALUES ?s { \"not an iri\" } SERVICE ?s { ?a ?b ?c } }",
            "SELECT * WHERE { VALUES ?s { <urn:x:s> } SERVICE ?s { ?a ?b ?c } }",
            "ASK { BIND(<urn:x:s> AS ?s) SERVICE ?s {} }",
        ] {
            let e = match run(&store, &prepared(text), limits()) {
                Err(e) => e,
                Ok(a) => panic!("`{text}` should fail, got {a:?}"),
            };
            assert_eq!(e.rule, "QUERY_FAILED", "{text}: {}", e.message);
            assert_eq!(e.details["reason"], "service", "{text}: {}", e.details);
        }
        // SILENT never fails, and an ASK over it is true: the service adds one empty solution.
        let a = answer(
            &store,
            "ASK { SERVICE SILENT <urn:x:nowhere> { ?s ?p ?o } }",
        );
        assert_eq!(a, Answer::Boolean(true));
    }
}
