//! Goal tests for `hali query` through the real binary (#9, spec §9.9, §9.3, §9.4, §9.5).
//! Locked golden snapshots sit in tests/snapshots/ (hand-written from the issue's contract and
//! the W3C SPARQL 1.1 Query Results JSON format): `cli_query__select_json` and
//! `cli_query__ask_json` (stdout, the W3C format, no hali envelope, validated against
//! `hali schema query`) and `cli_query__read_only_query_json_envelope` (stderr, the error
//! envelope of an UPDATE). CONSTRUCT and DESCRIBE print Turtle, which cannot be an `insta`
//! snapshot (tests/schema_contract.rs requires every `.snap` to be JSON), so those tests compare
//! exact text and parse the Turtle back instead.
//!
//! Assumed CLI (the implementer follows it): `hali query '<sparql>'`, one positional argument and
//! nothing else (no file, no stdin, no `--limit`, `--offset` or `--fields`); the global flags
//! `--json`, `--dry-run` and `--yes` are accepted before or after `query`, and the last two are
//! ignored. The §3.4 prefix table is predeclared. Core limits are the `Limits::default()` of
//! tests/core_query.rs.
//!
//! Wording the contract leaves open and these tests fix (change them, with the human, if the
//! wording should differ):
//! - the text of a SELECT cell is the N-Triples form of the term with an IRI shortened through
//!   the prefix table when its local part is a plain name (`dcterms:isPartOf`, as `describe`
//!   shows relation ids), `<urn:x:a>` otherwise, `"text"@en` for a language-tagged literal; the
//!   escapes of §9.3 are applied to the lexical form, so an ESC inside a literal prints as
//!   `\u{1b}` (never as the N-Triples `\u001B`);
//! - a SELECT with no solutions prints the header line only; an ASK prints `true` or `false`;
//! - the `suggested_fix` of `READ_ONLY_QUERY` is the one in the locked snapshot;
//! - an escaped character in CONSTRUCT/DESCRIBE Turtle is written `\uXXXX` or `\UXXXXXXXX`; the
//!   tests compare it case-insensitively. Layout (LF, and the TAB the serializer indents with)
//!   stays raw;
//! - an empty CONSTRUCT result may print nothing or only the prefix lines; it must parse as
//!   Turtle with no triple.
//!
//! The `hali schema` list gains `query` (it sorts between `forget` and `refresh`). Older tests
//! assert the old list and are not goal-owned, so the implementer must update them: the
//! `NAMES` and the text list of tests/cli_schema.rs, the name list of tests/core_schema.rs, the
//! exact lists of tests/cli_describe.rs, tests/cli_find.rs, tests/cli_refresh.rs,
//! tests/cli_relation.rs, tests/cli_relink.rs, tests/cli_scan.rs and tests/cli_type.rs, the three
//! snapshots that hold the list (tests/snapshots/cli_schema__, cli_scan__ and
//! cli_relink__schema_list_json.snap; `.goal/checks/03-existing_goldens_unchanged.sh` holds each
//! to the one added `"query",`), and a `query` row in `CASES` of tests/dry_run_guard.rs
//! (`.goal/checks/04-dry_run_guard_query.sh`). tests/schema_contract.rs is goal-owned and
//! already accepts the longer list.
//!
//! Isolation: `HOME`, `XDG_CONFIG_HOME`, `XDG_STATE_HOME`, `HALI_DATA_DIR` point at temp paths
//! (the state dir is not created) and the rest of the environment is cleared, so the real store,
//! config and `~/.local/state` are never touched. The temp root is canonicalised.

#[cfg(feature = "goal")]
mod goal {
    use assert_cmd::Command;
    use oxrdf::{Literal, NamedNode, Term, Triple};
    use serde_json::{Value, json};
    use std::collections::{BTreeMap, HashSet};
    use std::ffi::OsStr;
    use std::net::TcpListener;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime};
    use tempfile::TempDir;

    const NOW: &str = "2026-10-24T09:30:00Z";
    const EXT: &str = "https://w3id.org/hali/graph/extensions";
    const READ_ONLY_MESSAGE: &str =
        "hali query is read-only; SPARQL Update is refused and nothing was run";

    /// The four standard relations of the core graph, with their labels.
    const SELECT_RELATIONS: &str = "SELECT ?relation ?label WHERE { ?relation a owl:ObjectProperty ; \
         rdfs:label ?label } ORDER BY ?relation";
    const ASK_RELATION: &str = "ASK { dcterms:relation a owl:ObjectProperty }";
    const CONSTRUCT_LABELS: &str =
        "CONSTRUCT { ?r rdfs:label ?l } WHERE { ?r a owl:ObjectProperty ; rdfs:label ?l }";

    /// The prefix lines of tool-managed Turtle (§3.4), without the header comment.
    const PREFIX_LINES: &str = "@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .\n\
        @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .\n\
        @prefix owl: <http://www.w3.org/2002/07/owl#> .\n\
        @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .\n\
        @prefix skos: <http://www.w3.org/2004/02/skos/core#> .\n\
        @prefix dcterms: <http://purl.org/dc/terms/> .\n\
        @prefix dcat: <http://www.w3.org/ns/dcat#> .\n\
        @prefix prov: <http://www.w3.org/ns/prov#> .\n\
        @prefix foaf: <http://xmlns.com/foaf/0.1/> .\n\
        @prefix spdx: <http://spdx.org/rdf/terms#> .\n\
        @prefix sh: <http://www.w3.org/ns/shacl#> .\n\
        @prefix hali: <https://w3id.org/hali#> .\n";

    struct Env {
        _tmp: TempDir,
        root: PathBuf,
        data: PathBuf,
    }

    struct Out {
        code: Option<i32>,
        stdout: String,
        stderr: String,
    }

    #[derive(Debug, PartialEq)]
    enum Entry {
        Dir(SystemTime),
        File(SystemTime, Vec<u8>),
        Other(SystemTime),
    }

    impl Env {
        /// No data dir at all.
        fn absent() -> Env {
            let tmp = tempfile::tempdir().unwrap();
            let root = std::fs::canonicalize(tmp.path()).unwrap();
            for d in ["cfg", "home"] {
                std::fs::create_dir_all(root.join(d)).unwrap();
            }
            let data = root.join("data");
            Env {
                _tmp: tmp,
                root,
                data,
            }
        }

        /// An empty data dir.
        fn empty() -> Env {
            let env = Env::absent();
            std::fs::create_dir_all(&env.data).unwrap();
            env
        }

        /// tests/fixtures/relations in the data dir (5 Resources, Concept `ml`, 10 relation
        /// kinds), as tests/cli_describe.rs uses it.
        fn new() -> Env {
            let env = Env::empty();
            let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/relations");
            for entry in std::fs::read_dir(&fixture).unwrap() {
                let entry = entry.unwrap();
                std::fs::copy(entry.path(), env.data.join(entry.file_name())).unwrap();
            }
            env
        }

        /// An empty data dir whose `extensions.ttl` is `turtle` (full IRIs only).
        fn with_extensions(turtle: &str) -> Env {
            let env = Env::empty();
            std::fs::write(env.data.join("extensions.ttl"), turtle).unwrap();
            env
        }

        fn write_config(&self, toml: &str) {
            let dir = self.root.join("cfg/hali");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("config.toml"), toml).unwrap();
        }

        fn state(&self) -> PathBuf {
            self.root.join("state")
        }

        fn cmd(&self) -> Command {
            let mut cmd = Command::cargo_bin("hali").unwrap();
            cmd.env_clear()
                .env("XDG_CONFIG_HOME", self.root.join("cfg"))
                .env("XDG_STATE_HOME", self.state())
                .env("HOME", self.root.join("home"))
                .env("HALI_DATA_DIR", &self.data)
                .env("HALI_TEST_NOW", NOW)
                .env("HALI_TEST_ID_SEED", "1")
                .timeout(Duration::from_secs(60));
            cmd
        }

        fn clean(&self, bytes: Vec<u8>) -> String {
            String::from_utf8(bytes)
                .unwrap()
                .replace(self.root.to_str().unwrap(), "/TMP")
        }

        fn out(&self, out: std::process::Output) -> Out {
            Out {
                code: out.status.code(),
                stdout: self.clean(out.stdout),
                stderr: self.clean(out.stderr),
            }
        }

        fn run<I, S>(&self, args: I) -> Out
        where
            I: IntoIterator<Item = S>,
            S: AsRef<OsStr>,
        {
            let out = self.cmd().args(args).output().unwrap();
            self.out(out)
        }

        /// Everything under the root: names, kinds, mtimes and file bytes.
        fn tree(&self) -> BTreeMap<String, Entry> {
            fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<String, Entry>) {
                for entry in std::fs::read_dir(dir).unwrap() {
                    let path = entry.unwrap().path();
                    let meta = std::fs::symlink_metadata(&path).unwrap();
                    let name = path.strip_prefix(root).unwrap().display().to_string();
                    let mtime = meta.modified().unwrap();
                    if meta.is_dir() {
                        out.insert(name, Entry::Dir(mtime));
                        walk(&path, root, out);
                    } else if meta.is_file() {
                        out.insert(name, Entry::File(mtime, std::fs::read(&path).unwrap()));
                    } else {
                        out.insert(name, Entry::Other(mtime));
                    }
                }
            }
            let mut out = BTreeMap::new();
            walk(&self.root, &self.root, &mut out);
            out
        }
    }

    fn json(s: &str) -> Value {
        assert_eq!(
            s.matches('\n').count(),
            1,
            "one compact line expected: {s:?}"
        );
        assert!(s.ends_with('\n'), "{s:?}");
        serde_json::from_str(s).unwrap_or_else(|e| panic!("not JSON ({e}): {s:?}"))
    }

    fn ok_text(out: &Out) -> &str {
        assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
        assert_eq!(out.stderr, "");
        &out.stdout
    }

    /// The error envelope of a failed `--json` run: nothing on stdout.
    fn failed(out: &Out, code: i32, rule: &str) -> Value {
        assert_eq!(out.code, Some(code), "stderr: {}", out.stderr);
        assert_eq!(out.stdout, "");
        let v = json(&out.stderr);
        assert_eq!(v["schema_version"], 1);
        assert_eq!(v["error"]["rule"], rule, "{}", out.stderr);
        v
    }

    fn snapshot_stdout(name: &str, out: &Out) {
        assert_eq!(out.code, Some(0), "{}", out.stderr);
        assert_eq!(out.stderr, "");
        // The locked snapshots are named without the `goal__` module segment.
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!(name, out.stdout);
        });
    }

    fn snapshot_stderr(name: &str, out: &Out) {
        assert_eq!(out.stdout, "");
        insta::with_settings!({prepend_module_to_snapshot => false}, {
            insta::assert_snapshot!(name, out.stderr);
        });
    }

    fn named(iri: &str) -> NamedNode {
        NamedNode::new_unchecked(iri)
    }

    /// The triples of a Turtle document, as the parser reads them.
    fn parse_turtle(text: &str) -> HashSet<Triple> {
        oxttl::TurtleParser::new()
            .for_slice(text.as_bytes())
            .map(|t| t.unwrap_or_else(|e| panic!("not Turtle ({e}):\n{text}")))
            .collect()
    }

    /// Characters §9.3 would escape that are not layout (LF, and the TAB the serializer
    /// indents with).
    fn terminal_unsafe(text: &str) -> Vec<char> {
        text.chars()
            .filter(|&c| {
                (c.is_control() && c != '\n' && c != '\t')
                    || matches!(c,
                        '\u{2028}' | '\u{2029}' | '\u{061C}' | '\u{200E}' | '\u{200F}'
                        | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
                        | '\u{00AD}' | '\u{180E}' | '\u{200B}' | '\u{2060}'..='\u{2064}'
                        | '\u{206A}'..='\u{206F}' | '\u{FEFF}' | '\u{FFF9}'..='\u{FFFB}'
                        | '\u{E0000}'..='\u{E007F}')
            })
            .collect()
    }

    // ---- criterion 1: SELECT --json ---------------------------------------------------------------

    #[test]
    fn select_json() {
        let env = Env::absent();
        let out = env.run(["query", "--json", SELECT_RELATIONS]);
        snapshot_stdout("cli_query__select_json", &out);
        // Not the hali envelope; the same bytes whichever side of `query` the flag is on.
        assert!(!out.stdout.contains("schema_version"));
        let before = env.run(["--json", "query", SELECT_RELATIONS]);
        assert_eq!(before.stdout, out.stdout);
        assert_eq!(before.code, Some(0));
        // Nothing was created: no data dir, no state dir.
        assert!(!env.data.exists());
        assert!(!env.state().exists());
    }

    #[test]
    fn select_json_terms() {
        let env = Env::absent();
        let out = env.run([
            "query",
            "--json",
            "SELECT ?i ?s ?n ?l ?b ?u WHERE { BIND(<urn:x:a> AS ?i) BIND(\"t\" AS ?s) \
             BIND(5 AS ?n) BIND(\"x\"@fr AS ?l) BIND(BNODE(\"k\") AS ?b) } ORDER BY ?i",
        ]);
        let v = json(ok_text(&out));
        assert_eq!(v["head"], json!({"vars": ["i", "s", "n", "l", "b", "u"]}));
        let bindings = v["results"]["bindings"].as_array().unwrap();
        assert_eq!(bindings.len(), 1);
        let row = &bindings[0];
        assert_eq!(row["i"], json!({"type": "uri", "value": "urn:x:a"}));
        assert_eq!(row["s"], json!({"type": "literal", "value": "t"}));
        assert_eq!(
            row["n"],
            json!({"type": "literal", "value": "5",
                "datatype": "http://www.w3.org/2001/XMLSchema#integer"})
        );
        assert_eq!(
            row["l"],
            json!({"type": "literal", "value": "x", "xml:lang": "fr"})
        );
        assert_eq!(row["b"]["type"], "bnode");
        // An unbound variable has no key in its solution.
        assert!(row.get("u").is_none(), "{row}");
    }

    // ---- criterion 2: ASK --json ------------------------------------------------------------------

    #[test]
    fn ask_json() {
        let env = Env::absent();
        let out = env.run(["query", "--json", ASK_RELATION]);
        snapshot_stdout("cli_query__ask_json", &out);
        assert_eq!(out.stdout, "{\"head\":{},\"boolean\":true}\n");
        let no = env.run(["query", "--json", "ASK { <urn:x:nothing> ?p ?o }"]);
        assert_eq!(ok_text(&no), "{\"head\":{},\"boolean\":false}\n");
    }

    // ---- criterion 4: CONSTRUCT and DESCRIBE print Turtle ------------------------------------------

    #[test]
    fn construct_turtle() {
        let env = Env::absent();
        let text = env.run(["query", CONSTRUCT_LABELS]);
        let want = format!(
            "{PREFIX_LINES}\n\
             dcterms:isPartOf rdfs:label \"is part of\"@en .\n\
             dcterms:references rdfs:label \"references\"@en .\n\
             dcterms:relation rdfs:label \"relation\"@en .\n\
             prov:wasDerivedFrom rdfs:label \"was derived from\"@en .\n"
        );
        assert_eq!(ok_text(&text), want);
        // The same Turtle under --json: it is not wrapped and not turned into JSON.
        let json = env.run(["query", "--json", CONSTRUCT_LABELS]);
        assert_eq!(json.stdout, text.stdout);
        assert_eq!(json.code, Some(0));
        assert_eq!(json.stderr, "");
        // It parses back to the same triples.
        let label = named("http://www.w3.org/2000/01/rdf-schema#label");
        let triple = |s: &str, l: &str| {
            Triple::new(
                named(s),
                label.clone(),
                Literal::new_language_tagged_literal(l, "en").unwrap(),
            )
        };
        let expected: HashSet<Triple> = [
            triple("http://purl.org/dc/terms/isPartOf", "is part of"),
            triple("http://purl.org/dc/terms/references", "references"),
            triple("http://purl.org/dc/terms/relation", "relation"),
            triple(
                "http://www.w3.org/ns/prov#wasDerivedFrom",
                "was derived from",
            ),
        ]
        .into_iter()
        .collect();
        assert_eq!(parse_turtle(&text.stdout), expected);
        assert!(!text.stdout.starts_with('#'), "no header comment");
        assert!(!env.data.exists() && !env.state().exists());
    }

    #[test]
    fn construct_is_sorted_like_the_store() {
        // The triples of one subject come out as hali writes them (§3.4): `rdf:type` first, then
        // the predicates in order, one statement with `;` and a TAB.
        let env = Env::new();
        let out = env.run([
            "query",
            "CONSTRUCT { hali:ml ?p ?o } WHERE { hali:ml ?p ?o }",
        ]);
        assert_eq!(
            ok_text(&out),
            format!(
                "{PREFIX_LINES}\n\
                 hali:ml a skos:Concept ;\n\
                 \tskos:inScheme hali:topics ;\n\
                 \tskos:prefLabel \"ml\"@en .\n"
            )
        );
    }

    #[test]
    fn construct_empty_is_valid_turtle() {
        let env = Env::absent();
        let out = env.run([
            "query",
            "CONSTRUCT { ?s ?p ?o } WHERE { ?s a <urn:x:nothing> ; ?p ?o }",
        ]);
        let text = ok_text(&out);
        assert!(parse_turtle(text).is_empty(), "{text:?}");
    }

    #[test]
    fn describe_turtle() {
        let env = Env::new();
        let out = env.run(["query", "DESCRIBE hali:ml"]);
        let text = ok_text(&out);
        assert!(text.starts_with(PREFIX_LINES), "{text}");
        assert!(!text.starts_with('#'), "no header comment");
        let triples = parse_turtle(text);
        let ml = named("https://w3id.org/hali#ml");
        let pref = Triple::new(
            ml.clone(),
            named("http://www.w3.org/2004/02/skos/core#prefLabel"),
            Literal::new_language_tagged_literal("ml", "en").unwrap(),
        );
        assert!(triples.contains(&pref), "prefLabel missing from\n{text}");
        // Identical under --json, and the Concept's own triples are all there.
        let json = env.run(["query", "--json", "DESCRIBE hali:ml"]);
        assert_eq!(json.stdout, out.stdout);
        let own = env.run([
            "query",
            "CONSTRUCT { hali:ml ?p ?o } WHERE { hali:ml ?p ?o }",
        ]);
        for t in parse_turtle(&own.stdout) {
            assert!(triples.contains(&t), "{t} missing from\n{text}");
        }
    }

    // ---- criterion 5: Turtle output is terminal-safe -----------------------------------------------

    const HOSTILE: &str = "<urn:x:s1> <urn:x:p> \"a\\u202Eb\\u0007c\" .\n\
        <urn:x:s2> <urn:x:p> <https://w3id.org/hali#a\\uFEFFb> .\n\
        <urn:x:s3> <urn:x:p> \"t\\U000E0041x\" .\n";

    #[test]
    fn turtle_escaped() {
        let env = Env::with_extensions(HOSTILE);
        let query = format!("CONSTRUCT {{ ?s ?p ?o }} WHERE {{ GRAPH <{EXT}> {{ ?s ?p ?o }} }}");
        let out = env.run(["query", &query]);
        let text = ok_text(&out);
        assert_eq!(terminal_unsafe(text), Vec::<char>::new(), "{text:?}");
        let lower = text.to_lowercase();
        assert!(lower.contains("\\u202e"), "{text}");
        assert!(lower.contains("\\u0007"), "{text}");
        assert!(lower.contains("\\u000e0041"), "{text}");
        // An IRI holding such a character is written whole, not as a prefixed name.
        assert!(
            lower.contains("<https://w3id.org/hali#a\\ufeffb>"),
            "{text}"
        );
        assert!(!text.contains("hali:a"), "{text}");
        // Still valid Turtle, and the same triples as the store holds.
        let p = named("urn:x:p");
        let expected: HashSet<Triple> = [
            Triple::new(
                named("urn:x:s1"),
                p.clone(),
                Literal::new_simple_literal("a\u{202E}b\u{7}c"),
            ),
            Triple::new(
                named("urn:x:s2"),
                p.clone(),
                Term::NamedNode(named("https://w3id.org/hali#a\u{FEFF}b")),
            ),
            Triple::new(
                named("urn:x:s3"),
                p,
                Literal::new_simple_literal("t\u{E0041}x"),
            ),
        ]
        .into_iter()
        .collect();
        assert_eq!(parse_turtle(text), expected);
        // --json prints the same Turtle; DESCRIBE is escaped the same way.
        let json = env.run(["query", "--json", &query]);
        assert_eq!(json.stdout, out.stdout);
        let described = env.run(["query", "DESCRIBE <urn:x:s1> <urn:x:s2>"]);
        let described = ok_text(&described);
        assert_eq!(
            terminal_unsafe(described),
            Vec::<char>::new(),
            "{described:?}"
        );
        assert!(described.to_lowercase().contains("\\u202e"), "{described}");
    }

    // ---- criterion 6: text mode ---------------------------------------------------------------------

    #[test]
    fn select_text() {
        let env = Env::absent();
        let out = env.run(["query", SELECT_RELATIONS]);
        assert_eq!(
            ok_text(&out),
            "relation\tlabel\n\
             dcterms:isPartOf\t\"is part of\"@en\n\
             dcterms:references\t\"references\"@en\n\
             dcterms:relation\t\"relation\"@en\n\
             prov:wasDerivedFrom\t\"was derived from\"@en\n"
        );
        // An unbound cell is empty; an IRI the table cannot shorten stays in angle brackets.
        let out = env.run([
            "query",
            "SELECT ?r ?x ?u WHERE { VALUES ?r { dcterms:relation <urn:x:a> } \
             OPTIONAL { ?r rdfs:comment ?x } } ORDER BY ?r",
        ]);
        assert_eq!(
            ok_text(&out),
            "r\tx\tu\ndcterms:relation\t\t\n<urn:x:a>\t\t\n"
        );
        // No solution: the header line alone.
        let out = env.run([
            "query",
            "SELECT ?x WHERE { ?x a <urn:x:nothing> } ORDER BY ?x",
        ]);
        assert_eq!(ok_text(&out), "x\n");
        // The `--json` form is the W3C JSON, never the table.
        assert_ne!(env.run(["query", "--json", SELECT_RELATIONS]).stdout, "");
    }

    #[test]
    fn ask_text() {
        let env = Env::absent();
        assert_eq!(ok_text(&env.run(["query", ASK_RELATION])), "true\n");
        assert_eq!(
            ok_text(&env.run(["query", "ASK { <urn:x:nothing> ?p ?o }"])),
            "false\n"
        );
    }

    #[test]
    fn text_escaped() {
        let env = Env::with_extensions(
            "<urn:x:s1> <urn:x:p> \"a\\u001Bb\" .\n\
             <urn:x:s2> <urn:x:p> \"x\\u202Ey\" .\n\
             <urn:x:a\\u202Eb> <urn:x:p> <urn:x:o> .\n",
        );
        let query =
            format!("SELECT ?s ?o WHERE {{ GRAPH <{EXT}> {{ ?s <urn:x:p> ?o }} }} ORDER BY ?s");
        let out = env.run(["query", &query]);
        assert_eq!(
            ok_text(&out),
            "s\to\n\
             <urn:x:a\\u{202e}b>\t<urn:x:o>\n\
             <urn:x:s1>\t\"a\\u{1b}b\"\n\
             <urn:x:s2>\t\"x\\u{202e}y\"\n"
        );
        assert!(!out.stdout.contains('\u{1b}'));
        assert_eq!(terminal_unsafe(&out.stdout), Vec::<char>::new());
    }

    // ---- criterion 8: UPDATE is refused ---------------------------------------------------------------

    fn updates() -> Vec<String> {
        vec![
            "INSERT DATA { <urn:x:a> <urn:x:b> <urn:x:c> }".to_string(),
            "DELETE WHERE { ?s ?p ?o }".to_string(),
            "DELETE { ?s ?p ?o } INSERT { ?s ?p 1 } WHERE { ?s ?p ?o }".to_string(),
            "LOAD <file:///etc/passwd>".to_string(),
            "CLEAR ALL".to_string(),
            "DROP ALL".to_string(),
            "CREATE GRAPH <urn:x:g>".to_string(),
            format!("ADD <{EXT}> TO DEFAULT"),
            format!("MOVE DEFAULT TO <{EXT}>"),
            format!("COPY <{EXT}> TO <urn:x:g>"),
            "INSERT DATA { <urn:x:a> <urn:x:b> <urn:x:c> } ; CLEAR ALL".to_string(),
        ]
    }

    #[test]
    fn read_only_query() {
        let insert = "INSERT DATA { <urn:x:a> <urn:x:b> <urn:x:c> }";
        let out = Env::new().run(["query", "--json", insert]);
        assert_eq!(out.code, Some(1));
        snapshot_stderr("cli_query__read_only_query_json_envelope", &out);

        for env in [Env::new(), Env::absent(), Env::empty()] {
            let before = env.tree();
            for text in updates() {
                // --json: the envelope, with no details.
                let out = env.run(["query", "--json", &text]);
                let v = failed(&out, 1, "READ_ONLY_QUERY");
                assert_eq!(v["error"]["message"], READ_ONLY_MESSAGE, "{text}");
                assert_eq!(v["error"]["details"], json!({}), "{text}");
                assert!(v["error"]["suggested_fix"].is_string(), "{text}");
                // Text: the error line and the hint.
                let out = env.run(["query", &text]);
                assert_eq!(out.code, Some(1), "{text}: {}", out.stderr);
                assert_eq!(out.stdout, "", "{text}");
                assert!(
                    out.stderr
                        .starts_with(&format!("error: {READ_ONLY_MESSAGE}\nhint: ")),
                    "{text}: {}",
                    out.stderr
                );
                // Nothing was run, written or created.
                assert_eq!(env.tree(), before, "{text}");
            }
            assert!(!env.state().exists());
        }
        // The data dir that was never there is still not there.
        let env = Env::absent();
        env.run(["query", "CLEAR ALL"]);
        assert!(!env.data.exists());
    }

    #[test]
    fn read_only_query_leaves_the_store_byte_identical() {
        // The store is queried after the refusals exactly as before.
        let env = Env::new();
        let before = std::fs::read(env.data.join("resources.ttl")).unwrap();
        let count = "SELECT (COUNT(*) AS ?n) WHERE { ?s ?p ?o }";
        let first = env.run(["query", "--json", count]);
        assert_eq!(first.code, Some(0), "{}", first.stderr);
        for text in updates() {
            let out = env.run(["query", &text]);
            assert_eq!(out.code, Some(1), "{text}: {}", out.stderr);
        }
        let second = env.run(["query", "--json", count]);
        assert_eq!(first.stdout, second.stdout);
        assert_eq!(
            std::fs::read(env.data.join("resources.ttl")).unwrap(),
            before
        );
    }

    #[test]
    fn query_is_parsed_after_the_config_and_before_the_store() {
        // A store that cannot be parsed: an UPDATE is refused and bad SPARQL is a usage error
        // without the store being read; only a valid query reaches it.
        let env = Env::empty();
        std::fs::write(env.data.join("resources.ttl"), "this is { not turtle\n").unwrap();
        let out = env.run(["query", "--json", "CLEAR ALL"]);
        failed(&out, 1, "READ_ONLY_QUERY");
        let out = env.run(["query", "--json", "SELEKT"]);
        failed(&out, 2, "USAGE");
        let out = env.run(["query", "--json", "ASK {}"]);
        failed(&out, 3, "STORE_UNPARSEABLE");

        // A config that cannot be read comes first.
        let env = Env::new();
        env.write_config("this is = = not toml");
        let out = env.run(["query", "--json", "CLEAR ALL"]);
        failed(&out, 3, "CONFIG_INVALID");
        let out = env.run(["query", "--json", "SELEKT"]);
        failed(&out, 3, "CONFIG_INVALID");
    }

    // ---- criterion 9: a comment is not an update ------------------------------------------------------

    #[test]
    fn comment_not_update() {
        let env = Env::new();
        let before = env.tree();
        let out = env.run(["query", "# INSERT DATA {}\nASK {}"]);
        assert_eq!(ok_text(&out), "true\n");
        let out = env.run(["query", "--json", "# DELETE WHERE { ?s ?p ?o }\nASK {}"]);
        assert_eq!(ok_text(&out), "{\"head\":{},\"boolean\":true}\n");
        assert_eq!(env.tree(), before);
    }

    // ---- criterion 10: bad SPARQL is a usage error ---------------------------------------------------

    #[test]
    fn syntax_error() {
        let env = Env::new();
        let before = env.tree();
        for text in [
            "SELEKT ?x WHERE { ?x ?p ?o }",
            "SELECT ?x WHERE {",
            "INSERT DATA {",
        ] {
            let out = env.run(["query", "--json", text]);
            let v = failed(&out, 2, "USAGE");
            let message = v["error"]["message"].as_str().unwrap();
            assert!(message.starts_with("invalid SPARQL query: "), "{message}");
            assert!(message.chars().count() <= 22 + 1_000, "{message}");
            let out = env.run(["query", text]);
            assert_eq!(out.code, Some(2), "{}", out.stderr);
            assert_eq!(out.stdout, "");
            assert!(
                out.stderr.contains("invalid SPARQL query: "),
                "{}",
                out.stderr
            );
        }
        // The text of a hostile query never reaches the terminal raw.
        for text in [
            "ASK { bad\u{1b}[31m:thing a ?x }",
            "SELECT \u{202e}\u{7} ?x WHERE {",
            "ASK { <urn:x:\u{1b}> a ?x }",
        ] {
            let out = env.run(["query", text]);
            assert_eq!(out.code, Some(2), "{}", out.stderr);
            assert_eq!(
                terminal_unsafe(&out.stderr),
                Vec::<char>::new(),
                "{:?}",
                out.stderr
            );
        }
        assert_eq!(env.tree(), before);
    }

    // ---- criterion 11: SERVICE never connects ---------------------------------------------------------

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
    fn service_never_connects() {
        let (l, port) = listener();
        let service = format!("http://127.0.0.1:{port}/");
        let env = Env::new();

        // A SERVICE clause is QUERY_FAILED, naming the service.
        let out = env.run([
            "query",
            "--json",
            &format!("SELECT * WHERE {{ SERVICE <{service}> {{ ?s ?p ?o }} }} ORDER BY ?s"),
        ]);
        let v = failed(&out, 1, "QUERY_FAILED");
        assert_eq!(
            v["error"]["details"],
            json!({"reason": "service", "service": service})
        );
        no_connection(&l);
        let out = env.run([
            "query",
            &format!("ASK {{ SERVICE <{service}> {{ ?s ?p ?o }} }}"),
        ]);
        assert_eq!(out.code, Some(1), "{}", out.stderr);
        assert_eq!(out.stdout, "");
        assert!(out.stderr.starts_with("error: "), "{}", out.stderr);
        no_connection(&l);

        // SERVICE SILENT succeeds with nothing bound by the service.
        let out = env.run([
            "query",
            "--json",
            &format!("SELECT * WHERE {{ SERVICE SILENT <{service}> {{ ?s ?p ?o }} }} ORDER BY ?s"),
        ]);
        let v = json(ok_text(&out));
        // `SELECT *` lists the variables of the pattern; their order is the evaluator's.
        let mut vars: Vec<&str> = v["head"]["vars"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap())
            .collect();
        vars.sort_unstable();
        assert_eq!(vars, ["o", "p", "s"]);
        for row in v["results"]["bindings"].as_array().unwrap() {
            assert_eq!(row, &json!({}), "the service bound nothing");
        }
        no_connection(&l);

        // FROM and FROM NAMED name graphs; they are never fetched, and an unknown graph is empty.
        for text in [
            format!("SELECT ?s FROM <{service}> WHERE {{ ?s ?p ?o }} ORDER BY ?s"),
            format!(
                "SELECT ?s FROM NAMED <{service}> WHERE {{ GRAPH ?g {{ ?s ?p ?o }} }} ORDER BY ?s"
            ),
        ] {
            let out = env.run(["query", "--json", &text]);
            let v = json(ok_text(&out));
            assert_eq!(v["results"]["bindings"], json!([]), "{text}");
        }
        no_connection(&l);

        // LOAD is an update, refused before it can fetch.
        let out = env.run(["query", "--json", &format!("LOAD <{service}>")]);
        failed(&out, 1, "READ_ONLY_QUERY");
        no_connection(&l);
    }

    // ---- criterion 13: no --limit -----------------------------------------------------------------------

    #[test]
    fn no_limit_flag() {
        let env = Env::new();
        for flag in [
            vec!["--limit", "5"],
            vec!["--limit=5"],
            vec!["--offset", "1"],
            vec!["--fields", "x"],
        ] {
            let mut args = vec!["query"];
            args.extend(&flag);
            args.push("ASK {}");
            let out = env.run(&args);
            assert_eq!(out.code, Some(2), "{flag:?}: {}", out.stderr);
            assert_eq!(out.stdout, "", "{flag:?}");
            let name = flag[0].split('=').next().unwrap();
            assert!(out.stderr.contains(name), "{flag:?}: {}", out.stderr);
        }
        // The query is the one argument: nothing from a file or stdin, and no second argument.
        let out = env.run(["query"]);
        assert_eq!(out.code, Some(2), "{}", out.stderr);
        assert_eq!(out.stdout, "");
        let out = env
            .cmd()
            .arg("query")
            .write_stdin("ASK {}")
            .output()
            .unwrap();
        let out = env.out(out);
        assert_eq!(out.code, Some(2), "{}", out.stderr);
        assert_eq!(out.stdout, "");
        let out = env.run(["query", "ASK {}", "ASK {}"]);
        assert_eq!(out.code, Some(2), "{}", out.stderr);
        // Paging is the query's own LIMIT.
        let out = env.run([
            "query",
            "SELECT ?r WHERE { ?r a owl:ObjectProperty } ORDER BY ?r LIMIT 2",
        ]);
        assert_eq!(ok_text(&out).lines().count(), 3, "{}", out.stdout);
        // The help does not offer them.
        let help = env.run(["query", "--help"]);
        assert_eq!(help.code, Some(0), "{}", help.stderr);
        for flag in ["--limit", "--offset", "--fields"] {
            assert!(!help.stdout.contains(flag), "{}", help.stdout);
        }
    }

    // ---- criterion 14: --dry-run and --yes change nothing ----------------------------------------------

    #[test]
    fn dry_run_identical() {
        let env = Env::new();
        let before = env.tree();
        let texts = [
            (SELECT_RELATIONS, 0),
            (ASK_RELATION, 0),
            (CONSTRUCT_LABELS, 0),
            ("DESCRIBE hali:ml", 0),
            ("INSERT DATA { <urn:x:a> <urn:x:b> <urn:x:c> }", 1),
            ("SELEKT", 2),
        ];
        for (text, code) in texts {
            for json in [false, true] {
                let mode: &[&str] = if json { &["--json"] } else { &[] };
                let run = |extra: &[&str], after: &[&str]| {
                    let mut args: Vec<&str> = mode.to_vec();
                    args.extend(extra);
                    args.push("query");
                    args.extend(after);
                    args.push(text);
                    env.run(args)
                };
                let plain = run(&[], &[]);
                for (extra, after) in [
                    (vec!["--dry-run"], vec![]),
                    (vec!["--yes"], vec![]),
                    (vec!["--dry-run", "--yes"], vec![]),
                    (vec![], vec!["--dry-run"]),
                    (vec![], vec!["--yes"]),
                    (vec![], vec!["--dry-run", "--yes"]),
                ] {
                    let other = run(&extra, &after);
                    assert_eq!(other.code, plain.code, "{text} {extra:?} {after:?}");
                    assert_eq!(other.stdout, plain.stdout, "{text} {extra:?} {after:?}");
                    assert_eq!(other.stderr, plain.stderr, "{text} {extra:?} {after:?}");
                }
                assert_eq!(plain.code, Some(code), "{text}: {}", plain.stderr);
                assert_eq!(plain.stdout.is_empty(), code != 0, "{text}");
                assert!(!plain.stdout.contains("dry_run"), "{}", plain.stdout);
            }
        }
        assert_eq!(env.tree(), before);
    }

    // ---- criterion 15: the store ------------------------------------------------------------------------

    #[test]
    fn absent_store() {
        // An absent data dir and an empty one are queried as the core graph alone: exit 0,
        // nothing created.
        for env in [Env::absent(), Env::empty()] {
            let before = env.tree();
            let out = env.run(["query", SELECT_RELATIONS]);
            assert!(ok_text(&out).starts_with("relation\tlabel\ndcterms:isPartOf\t"));
            assert_eq!(ok_text(&out).lines().count(), 5);
            let out = env.run(["query", "--json", "ASK { hali:ml ?p ?o }"]);
            assert_eq!(ok_text(&out), "{\"head\":{},\"boolean\":false}\n");
            assert_eq!(env.tree(), before);
            assert!(!env.state().exists());
        }
        assert!(!Env::absent().data.exists());
        // A Concept in the fixture is not in the core graph.
        let env = Env::new();
        let out = env.run(["query", "--json", "ASK { hali:ml ?p ?o }"]);
        assert_eq!(ok_text(&out), "{\"head\":{},\"boolean\":true}\n");
    }

    #[test]
    fn unparseable_store() {
        for file in ["vocabulary.ttl", "resources.ttl", "extensions.ttl"] {
            let env = Env::empty();
            std::fs::write(env.data.join(file), "this is { not turtle\n").unwrap();
            let before = env.tree();
            let out = env.run(["query", "--json", ASK_RELATION]);
            let v = failed(&out, 3, "STORE_UNPARSEABLE");
            assert!(
                v["error"]["message"].as_str().unwrap().contains(file),
                "{file}: {v}"
            );
            let out = env.run(["query", ASK_RELATION]);
            assert_eq!(out.code, Some(3), "{}", out.stderr);
            assert_eq!(out.stdout, "");
            assert_eq!(env.tree(), before);
        }
    }

    // ---- rows 21-26 (review of #9) ----------------------------------------------------------------
    //
    // Row 21 (CLI half): there is no test-hooks way to shorten the 30 s limit (the only
    // injected values are HALI_TEST_NOW, HALI_TEST_ID_SEED and HALI_TEST_TTY), so a CLI test of
    // a timed-out query exiting cannot be written without a new hook; it is skipped. The
    // process half that is testable is the opposite one: a quick query must not wait out the
    // limit (row 25, mutation 7).

    #[test]
    fn a_quick_query_does_not_wait_for_the_time_limit() {
        let env = Env::absent();
        for args in [
            vec!["query", ASK_RELATION],
            vec!["query", "--json", SELECT_RELATIONS],
        ] {
            let started = std::time::Instant::now();
            let out = env.run(args);
            assert_eq!(out.code, Some(0), "{}", out.stderr);
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "a query that answers at once took {:?}",
                started.elapsed()
            );
        }
    }

    /// Row 22: an evaluation error the user's text causes is a usage error, never STORE_IO, and
    /// never talks about the data dir.
    #[test]
    fn unsupported_custom_functions_are_usage_errors() {
        let env = Env::with_extensions("<urn:x:s> <http://x/p> \"1\" .\n");
        let before = env.tree();
        for (text, name) in [
            (
                "SELECT * WHERE { ?x <http://x/p> ?y . FILTER(<http://x/f>(?y)) }",
                "http://x/f",
            ),
            ("SELECT (<urn:f>(1) AS ?y) {}", "urn:f"),
            ("SELECT (<urn:f>() AS ?y) {}", "urn:f"),
            ("SELECT (<urn:f>(1, 2, 3) AS ?y) {}", "urn:f"),
        ] {
            let out = env.run(["query", text]);
            assert_eq!(out.code, Some(2), "{text}: {}", out.stderr);
            assert_eq!(out.stdout, "");
            assert!(
                out.stderr.contains("invalid SPARQL query"),
                "{text}: {}",
                out.stderr
            );
            assert!(out.stderr.contains(name), "{text}: {}", out.stderr);
            assert!(!out.stderr.contains("/TMP"), "{text}: {}", out.stderr);
            assert!(!out.stderr.contains("data dir"), "{text}: {}", out.stderr);
            let out = env.run(["query", "--json", text]);
            let v = failed(&out, 2, "USAGE");
            let message = v["error"]["message"].as_str().unwrap();
            assert!(message.starts_with("invalid SPARQL query: "), "{message}");
            assert!(message.contains(name), "{message}");
        }
        assert_eq!(env.tree(), before);
    }

    /// The Turtle a query prints for `query`, which must parse, hold no raw §9.3 character and
    /// be the same with `--json`.
    fn safe_turtle(env: &Env, query: &str) -> (String, HashSet<Triple>) {
        let out = env.run(["query", query]);
        let text = ok_text(&out).to_string();
        assert_eq!(terminal_unsafe(&text), Vec::<char>::new(), "{text:?}");
        let triples = parse_turtle(&text);
        let json = env.run(["query", "--json", query]);
        assert_eq!(json.stdout, text);
        (text, triples)
    }

    fn construct_ext() -> String {
        format!("CONSTRUCT {{ ?s ?p ?o }} WHERE {{ GRAPH <{EXT}> {{ ?s ?p ?o }} }}")
    }

    /// Row 23: a blank node whose label holds a §9.3 character is still valid Turtle (a label
    /// cannot carry a `\u` escape, so the label has to change, not the escape).
    #[test]
    fn turtle_blank_node_labels_with_unsafe_characters() {
        for c in ['\u{FEFF}', '\u{061C}', '\u{E0041}'] {
            let env = Env::with_extensions(&format!("<urn:x:s> <urn:x:p> _:a{c}b .\n"));
            let (text, triples) = safe_turtle(&env, &construct_ext());
            assert_eq!(triples.len(), 1, "{c:?}: {text}");
            let t = triples.iter().next().unwrap();
            assert_eq!(t.subject, named("urn:x:s").into(), "{c:?}: {text}");
            assert!(matches!(t.object, Term::BlankNode(_)), "{c:?}: {text}");
            // Subject position, and DESCRIBE.
            let env = Env::with_extensions(&format!("_:a{c}b <urn:x:p> <urn:x:o> .\n"));
            let (text, triples) = safe_turtle(&env, &construct_ext());
            assert_eq!(triples.len(), 1, "{c:?}: {text}");
            let out = env.run(["query", "DESCRIBE <urn:x:o>"]);
            let described = ok_text(&out);
            assert_eq!(terminal_unsafe(described), Vec::<char>::new());
            parse_turtle(described);
        }
    }

    /// Row 23: `BNODE("...")` with such a character in the string, and a triple term holding an
    /// IRI that has one, print valid Turtle.
    #[test]
    fn turtle_bnode_function_and_triple_terms_are_valid() {
        let env = Env::absent();
        for c in ['\u{061C}', '\u{FEFF}', '\u{E0041}'] {
            let query = format!(
                "CONSTRUCT {{ <urn:x:s> <urn:x:p> ?b }} WHERE {{ BIND(BNODE(\"a{c}b\") AS ?b) }}"
            );
            let (text, triples) = safe_turtle(&env, &query);
            assert_eq!(triples.len(), 1, "{c:?}: {text}");
        }
        let query = "CONSTRUCT { <http://x/s> <http://x/p> <<( <https://w3id.org/hali#a\u{FEFF}> \
                     <http://x/p> <http://x/o> )>> } WHERE {}";
        let (text, triples) = safe_turtle(&env, query);
        assert_eq!(triples.len(), 1, "{text}");
        let lower = text.to_lowercase();
        assert!(lower.contains("\\ufeff"), "{text}");
        let inner = Triple::new(
            named("https://w3id.org/hali#a\u{FEFF}"),
            named("http://x/p"),
            named("http://x/o"),
        );
        let expected = Triple::new(
            named("http://x/s"),
            named("http://x/p"),
            Term::Triple(Box::new(inner)),
        );
        assert_eq!(triples, [expected].into_iter().collect());
        // A triple term in a subject-less position of every part: predicate-side and datatype.
        let query = "CONSTRUCT { <http://x/s> <http://x/p> <<( <http://x/a> <https://w3id.org/hali#a\u{FEFF}> \
                     \"v\"^^<https://w3id.org/hali#a\u{FEFF}> )>> } WHERE {}";
        let (text, triples) = safe_turtle(&env, query);
        assert_eq!(triples.len(), 1, "{text}");
    }

    /// Row 23: an IRI with a §9.3 character is written whole in every position (subject,
    /// predicate, object, datatype), never as a prefixed name, and the Turtle parses back.
    #[test]
    fn turtle_stand_in_iris_in_every_position() {
        let hostile = "https://w3id.org/hali#a\u{FEFF}";
        let env = Env::with_extensions(
            "<https://w3id.org/hali#a\\uFEFF> <urn:x:p1> <urn:x:o> .\n\
             <urn:x:s> <https://w3id.org/hali#a\\uFEFF> \"plain\" .\n\
             <urn:x:s> <urn:x:p3> \"v\"^^<https://w3id.org/hali#a\\uFEFF> .\n\
             <urn:x:s> <urn:x:p4> <https://w3id.org/hali#a\\uFEFF> .\n",
        );
        let (text, triples) = safe_turtle(&env, &construct_ext());
        let lower = text.to_lowercase();
        assert!(!text.contains("hali:a"), "{text}");
        assert_eq!(
            lower.matches("<https://w3id.org/hali#a\\ufeff>").count(),
            4,
            "{text}"
        );
        let expected: HashSet<Triple> = [
            Triple::new(named(hostile), named("urn:x:p1"), named("urn:x:o")),
            Triple::new(
                named("urn:x:s"),
                named(hostile),
                Literal::new_simple_literal("plain"),
            ),
            Triple::new(
                named("urn:x:s"),
                named("urn:x:p3"),
                Literal::new_typed_literal("v", named(hostile)),
            ),
            Triple::new(named("urn:x:s"), named("urn:x:p4"), named(hostile)),
        ]
        .into_iter()
        .collect();
        assert_eq!(triples, expected);
    }

    /// Row 25 (mutation 6): data that already holds the serializer's stand-in IRIs is not
    /// confused with them.
    #[test]
    fn turtle_stand_in_marker_avoids_the_data() {
        let hostile = "https://w3id.org/hali#a\u{FEFF}";
        let env = Env::with_extensions(
            "<urn:hali-stand-in-0-0> <urn:x:p> <https://w3id.org/hali#a\\uFEFF> .\n\
             <urn:hali-stand-in-0-1> <urn:x:p> <urn:hali-stand-in-0-0> .\n\
             <urn:hali-stand-in-1-0> <urn:x:p> <https://w3id.org/hali#b\\uFEFF> .\n",
        );
        let (text, triples) = safe_turtle(&env, &construct_ext());
        let expected: HashSet<Triple> = [
            Triple::new(
                named("urn:hali-stand-in-0-0"),
                named("urn:x:p"),
                named(hostile),
            ),
            Triple::new(
                named("urn:hali-stand-in-0-1"),
                named("urn:x:p"),
                named("urn:hali-stand-in-0-0"),
            ),
            Triple::new(
                named("urn:hali-stand-in-1-0"),
                named("urn:x:p"),
                named("https://w3id.org/hali#b\u{FEFF}"),
            ),
        ]
        .into_iter()
        .collect();
        assert_eq!(triples, expected, "{text}");
    }

    /// Row 24: nothing to query is a usage error at the CLI, in text and JSON.
    #[test]
    fn empty_text_is_a_usage_error() {
        let env = Env::new();
        let before = env.tree();
        for text in ["", "# only a comment", "PREFIX ex: <x:>"] {
            let out = env.run(["query", text]);
            assert_eq!(out.code, Some(2), "{text:?}: {}", out.stderr);
            assert_eq!(out.stdout, "");
            assert!(
                out.stderr.contains("invalid SPARQL query"),
                "{text:?}: {}",
                out.stderr
            );
            assert!(
                !out.stderr.contains("READ_ONLY_QUERY") && !out.stderr.contains("read-only"),
                "{text:?}: {}",
                out.stderr
            );
            let out = env.run(["query", "--json", text]);
            let v = failed(&out, 2, "USAGE");
            let message = v["error"]["message"].as_str().unwrap();
            assert!(message.starts_with("invalid SPARQL query: "), "{message}");
        }
        assert_eq!(env.tree(), before);
    }

    /// The first cell of the one data row of a text SELECT.
    fn one_cell(env: &Env, query: &str) -> String {
        let out = env.run(["query", query]);
        let text = ok_text(&out);
        let mut lines = text.lines();
        lines.next().expect("a header line");
        let row = lines.next().expect("one row");
        assert_eq!(lines.next(), None, "{text:?}");
        row.to_string()
    }

    /// Row 25 (mutations 1 and 2): literal cells in text mode keep their quotes, backslashes
    /// and datatypes.
    #[test]
    fn text_literal_cells() {
        let env = Env::with_extensions("<urn:x:s> <urn:x:p> \"a\\\\b\\\"c\" .\n");
        let query = format!("SELECT ?o WHERE {{ GRAPH <{EXT}> {{ ?s <urn:x:p> ?o }} }}");
        assert_eq!(one_cell(&env, &query), "\"a\\\\b\\\"c\"");
        let env = Env::absent();
        assert_eq!(
            one_cell(&env, "SELECT ?o WHERE { BIND(5 AS ?o) }"),
            "\"5\"^^xsd:integer"
        );
        assert_eq!(
            one_cell(&env, "SELECT ?o WHERE { BIND(\"v\"^^<urn:x:dt> AS ?o) }"),
            "\"v\"^^<urn:x:dt>"
        );
        assert_eq!(
            one_cell(
                &env,
                "SELECT ?o WHERE { BIND(\"v\"^^<urn:x:d\u{202e}t> AS ?o) }"
            ),
            "\"v\"^^<urn:x:d\\u{202e}t>"
        );
        assert_eq!(
            one_cell(&env, "SELECT ?o WHERE { BIND(\"v\\\\w\"@en AS ?o) }"),
            "\"v\\\\w\"@en"
        );
        // A plain string has no datatype suffix.
        assert_eq!(
            one_cell(&env, "SELECT ?o WHERE { BIND(\"plain\" AS ?o) }"),
            "\"plain\""
        );
    }

    /// Row 25 (mutation 3): blank nodes in text cells.
    #[test]
    fn text_blank_node_cells() {
        let env = Env::with_extensions("<urn:x:s> <urn:x:p> _:a\u{FEFF}b .\n");
        let query = format!("SELECT ?o WHERE {{ GRAPH <{EXT}> {{ ?s <urn:x:p> ?o }} }}");
        let cell = one_cell(&env, &query);
        assert!(cell.starts_with("_:"), "{cell:?}");
        assert_eq!(terminal_unsafe(&cell), Vec::<char>::new(), "{cell:?}");
        let env = Env::absent();
        let cell = one_cell(&env, "SELECT ?b WHERE { BIND(BNODE(\"x\") AS ?b) }");
        assert!(cell.starts_with("_:") && cell.len() > 2, "{cell:?}");
    }

    /// Row 26: the text of a triple-term cell is the N-Triples / RDF 1.2 triple-term form
    /// `<<( s p o )>>`, not `<< s p o >>`, with every part escaped for the terminal.
    #[test]
    fn text_triple_term_cells() {
        let env = Env::absent();
        // The N-Triples / RDF 1.2 triple-term form: `<<( s p o )>>`, escaped inside.
        assert_eq!(
            one_cell(
                &env,
                "SELECT ?t WHERE { BIND(<<( <urn:x:s> <urn:x:p> \"o\" )>> AS ?t) }"
            ),
            "<<( <urn:x:s> <urn:x:p> \"o\" )>>"
        );
        assert_eq!(
            one_cell(
                &env,
                "SELECT ?t WHERE { BIND(<<( <urn:x:s\u{202e}> dcterms:relation 5 )>> AS ?t) }"
            ),
            "<<( <urn:x:s\\u{202e}> dcterms:relation \"5\"^^xsd:integer )>>"
        );
    }

    /// Row 25 (mutation 4): a triple term in W3C JSON results.
    #[test]
    fn json_triple_term() {
        let env = Env::absent();
        let out = env.run([
            "query",
            "--json",
            "SELECT ?t WHERE { BIND(<<( <urn:x:s> <urn:x:p> \"o\" )>> AS ?t) }",
        ]);
        let v = json(ok_text(&out));
        assert_eq!(
            v["results"]["bindings"],
            json!([{"t": {"type": "triple", "value": {
                "subject": {"type": "uri", "value": "urn:x:s"},
                "predicate": {"type": "uri", "value": "urn:x:p"},
                "object": {"type": "literal", "value": "o"},
            }}}])
        );
    }

    /// Row 25 (F5 and mutation 8): `SERVICE ?s` is QUERY_FAILED service at the CLI, and
    /// `SERVICE SILENT` in an ASK prints `true`.
    #[test]
    fn service_variables_and_silent_ask() {
        let env = Env::absent();
        for text in [
            "SELECT * WHERE { BIND(<urn:x:s> AS ?s) SERVICE ?s { ?a ?b ?c } }",
            "SELECT * WHERE { SERVICE ?s { ?a ?b ?c } }",
        ] {
            let out = env.run(["query", "--json", text]);
            let v = failed(&out, 1, "QUERY_FAILED");
            assert_eq!(v["error"]["details"]["reason"], "service", "{text}");
        }
        let out = env.run([
            "query",
            "ASK { SERVICE SILENT <urn:x:nowhere> { ?s ?p ?o } }",
        ]);
        assert_eq!(ok_text(&out), "true\n");
        let out = env.run([
            "query",
            "--json",
            "ASK { SERVICE SILENT <urn:x:nowhere> { ?s ?p ?o } }",
        ]);
        assert_eq!(ok_text(&out), "{\"head\":{},\"boolean\":true}\n");
    }
}
