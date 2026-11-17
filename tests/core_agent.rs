//! Goal tests for agent identity in `core` (#13, spec §2.5, §9.6).
//!
//! Assumed API (invented here; the implementer follows it).
//!
//! `hali::core::config::agent_name(flag: Option<&str>, env: Option<&std::ffi::OsStr>)
//!     -> Result<Option<String>, hali::core::Error>`
//!   Resolves the acting agent's name. `flag` is `--agent`, `env` is `HALI_AGENT` as read with
//!   `var_os`. The flag wins when present; otherwise the env value; an env value that is empty
//!   or only whitespace counts as unset (`Ok(None)`). A name is trimmed. A flag that is empty
//!   or whitespace-only, a flag or env value with a control character, and an env value that
//!   is not UTF-8 are a usage error (`USAGE`, exit code 2). Neither given: `Ok(None)`.
//!
//! `hali::core::Context::with_agent(self, Option<String>) -> Context` sets the (already
//!   resolved and trimmed) agent name; a context made by `Context::new` has none.
//!
//! `resource::add` (via `add_all`) reads the agent from the context: with a name, the new
//!   Catalog record is `prov:wasAttributedTo` the `prov:SoftwareAgent` whose `foaf:name` equals
//!   the name exactly (the first by IRI when several do), minted when absent as `a
//!   prov:SoftwareAgent ; foaf:name "NAME" ; prov:actedOnBehalfOf <user Person>`, the Person
//!   being minted first when absent. Id order: Resource, record, Person (first time only),
//!   SoftwareAgent (first time only). Without a name the record goes to the Person, as today,
//!   and no SoftwareAgent is minted. The `user_name` argument still names the Person.
//!
//! `SeededIdGen::new(1)` yields R1, C1, P, A (the fourth id) in order.

#[cfg(feature = "goal")]
mod goal {
    use hali::core::config::agent_name;
    use hali::core::resource::{self, FileFacts};
    use hali::core::{Context, FixedClock, SeededIdGen};
    use hali::storage::{File, Store};
    use oxrdf::Term;
    use std::collections::BTreeSet;
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    const R1: &str = "018bcfe5-6801-710a-adec-89025cc1beeb";
    const C1: &str = "018bcfe5-6802-7893-a2ee-fb32555e71c1";
    const P: &str = "018bcfe5-6803-71bb-94d8-d101b5b9c34d";
    const A: &str = "018bcfe5-6804-7099-ac6c-d7363ca585e7";

    const NOW: &str = "2026-10-24T09:30:00Z";
    const XSD: &str = "http://www.w3.org/2001/XMLSchema#";
    const SHA: &str = "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03";
    const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
    const PROV: &str = "http://www.w3.org/ns/prov#";
    const NAME: &str = "http://xmlns.com/foaf/0.1/name";

    fn ctx(seed: u64, agent: Option<&str>) -> Context {
        Context::new(
            "en",
            Box::new(FixedClock::new(NOW)),
            Box::new(SeededIdGen::new(seed)),
        )
        .with_agent(agent.map(str::to_string))
    }

    fn ids(seed: u64) -> Vec<String> {
        let c = ctx(seed, None);
        (0..6).map(|_| c.next_id()).collect()
    }

    fn empty_store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
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

    fn triples_of(turtle: &str) -> BTreeSet<String> {
        oxttl::TurtleParser::new()
            .for_slice(turtle.as_bytes())
            .map(|t| format!("{} .", t.expect("resources.ttl is Turtle")))
            .collect()
    }

    fn triples(store: &Store) -> BTreeSet<String> {
        triples_of(&hali::storage::to_turtle(store, File::Resources))
    }

    fn nt(s: &str, p: &str, o: &str) -> String {
        format!("<{s}> <{p}> {o} .")
    }

    fn urn(id: &str) -> String {
        format!("urn:uuid:{id}")
    }

    fn dt(v: &str) -> String {
        format!("\"{v}\"^^<{XSD}dateTime>")
    }

    /// The triples `add` writes for a first `/data/notes.txt` into an empty store, the record
    /// attributed to `attributed` and the Person (`people`) and agent (`agents`: id, name,
    /// acts on behalf of) nodes as given.
    fn expected(
        r: &str,
        c: &str,
        attributed: &str,
        people: &[(&str, &str)],
        agents: &[(&str, &str, &str)],
    ) -> BTreeSet<String> {
        let r = urn(r);
        let c = urn(c);
        let sum = format!("{r}#sha256");
        let mut set = BTreeSet::from([
            nt(&r, RDF_TYPE, "<https://w3id.org/hali#Resource>"),
            nt(&r, "http://purl.org/dc/terms/title", "\"notes\""),
            nt(
                &r,
                "http://www.w3.org/ns/dcat#downloadURL",
                "<file:///data/notes.txt>",
            ),
            nt(
                &r,
                "http://www.w3.org/ns/dcat#mediaType",
                "<http://www.iana.org/assignments/media-types/text/plain>",
            ),
            nt(
                &r,
                "http://www.w3.org/ns/dcat#byteSize",
                &format!("\"6\"^^<{XSD}integer>"),
            ),
            nt(
                &r,
                "http://purl.org/dc/terms/modified",
                &dt("2026-10-20T14:02:11Z"),
            ),
            nt(
                &r,
                "http://spdx.org/rdf/terms#checksum",
                &format!("<{sum}>"),
            ),
            nt(
                &sum,
                "http://spdx.org/rdf/terms#algorithm",
                "<http://spdx.org/rdf/terms#checksumAlgorithm_sha256>",
            ),
            nt(
                &sum,
                "http://spdx.org/rdf/terms#checksumValue",
                &format!("\"{SHA}\"^^<{XSD}hexBinary>"),
            ),
            nt(&c, RDF_TYPE, "<http://www.w3.org/ns/dcat#CatalogRecord>"),
            nt(
                &c,
                "http://xmlns.com/foaf/0.1/primaryTopic",
                &format!("<{r}>"),
            ),
            nt(&c, "http://purl.org/dc/terms/issued", &dt(NOW)),
            nt(&c, "http://purl.org/dc/terms/modified", &dt(NOW)),
            nt(
                &c,
                &format!("{PROV}wasAttributedTo"),
                &format!("<{}>", urn(attributed)),
            ),
        ]);
        for (id, name) in people {
            set.insert(nt(&urn(id), RDF_TYPE, &format!("<{PROV}Person>")));
            set.insert(nt(&urn(id), NAME, &format!("\"{name}\"")));
        }
        for (id, name, on_behalf_of) in agents {
            set.insert(nt(&urn(id), RDF_TYPE, &format!("<{PROV}SoftwareAgent>")));
            set.insert(nt(&urn(id), NAME, &format!("\"{name}\"")));
            set.insert(nt(
                &urn(id),
                &format!("{PROV}actedOnBehalfOf"),
                &format!("<{}>", urn(on_behalf_of)),
            ));
        }
        set
    }

    fn count_typed(set: &BTreeSet<String>, class: &str) -> usize {
        set.iter()
            .filter(|t| t.contains(&format!("<{RDF_TYPE}> <{PROV}{class}> .")))
            .count()
    }

    // ---- row 1: the record is attributed to the SoftwareAgent ------------------------------

    #[test]
    fn add_attributes_to_agent() {
        let (mut store, _dir) = empty_store();
        let out = resource::add(
            &ctx(1, Some("claude")),
            &mut store,
            &facts("/data/notes.txt"),
            None,
            "nate",
        )
        .unwrap();
        assert!(!out.value.existing);
        assert_eq!(out.value.record.id, R1);

        let want = expected(R1, C1, A, &[(P, "nate")], &[(A, "claude", P)]);
        assert_eq!(triples(&store), want);
        // The changes are exactly those triples.
        let changed: BTreeSet<String> = out.changes.iter().map(|c| c.triple.clone()).collect();
        assert_eq!(changed, want);
        assert_eq!(out.changes.len(), want.len());
    }

    // ---- rows 2 and 9: the name rules ------------------------------------------------------

    fn os(s: &str) -> Option<OsString> {
        Some(OsString::from(s))
    }

    fn resolve(flag: Option<&str>, env: Option<&OsString>) -> Result<Option<String>, u8> {
        agent_name(flag, env.map(|e| e.as_os_str())).map_err(|e| e.exit_code())
    }

    #[test]
    fn agent_name_rules() {
        // Neither: the user is the Agent.
        assert_eq!(resolve(None, None), Ok(None));
        // The flag alone, the env alone, the flag over the env.
        assert_eq!(resolve(Some("claude"), None), Ok(Some("claude".into())));
        assert_eq!(
            resolve(None, os("codex").as_ref()),
            Ok(Some("codex".into()))
        );
        assert_eq!(
            resolve(Some("claude"), os("codex").as_ref()),
            Ok(Some("claude".into()))
        );
        // Names are trimmed, and case is kept.
        assert_eq!(
            resolve(Some("  Claude \t"), None),
            Ok(Some("Claude".into()))
        );
        assert_eq!(
            resolve(None, os(" codex ").as_ref()),
            Ok(Some("codex".into()))
        );
        // An empty or whitespace-only env counts as unset; the flag still wins over it.
        assert_eq!(resolve(None, os("").as_ref()), Ok(None));
        assert_eq!(resolve(None, os("  ").as_ref()), Ok(None));
        assert_eq!(resolve(None, os(" \t ").as_ref()), Ok(None));
        assert_eq!(
            resolve(Some("claude"), os("  ").as_ref()),
            Ok(Some("claude".into()))
        );
        // A name with inner spaces and non-ASCII letters is fine.
        assert_eq!(
            resolve(Some("my agent \u{e9}"), None),
            Ok(Some("my agent \u{e9}".into()))
        );
        // `hali` is not special.
        assert_eq!(resolve(Some("hali"), None), Ok(Some("hali".into())));

        // Usage errors (exit 2): an empty or blank flag, control characters, non-UTF-8.
        assert_eq!(resolve(Some(""), None), Err(2));
        assert_eq!(resolve(Some("   "), None), Err(2));
        assert_eq!(resolve(Some(""), os("codex").as_ref()), Err(2));
        assert_eq!(resolve(Some("a\nb"), None), Err(2));
        assert_eq!(resolve(Some("a\u{7f}b"), None), Err(2));
        assert_eq!(resolve(Some("a\u{0}b"), None), Err(2));
        assert_eq!(resolve(None, os("a\tb").as_ref()), Err(2));
        assert_eq!(resolve(None, os("a\u{1b}[0m").as_ref()), Err(2));
        let bad = OsString::from_vec(vec![b'a', 0xff, b'b']);
        assert_eq!(resolve(None, Some(&bad)), Err(2));
        // A valid flag does not excuse an invalid env.
        assert_eq!(resolve(Some("claude"), os("a\nb").as_ref()), Err(2));
        assert_eq!(resolve(Some("claude"), Some(&bad)), Err(2));
        // Trimming comes before the control-character check.
        assert_eq!(resolve(Some("claude\n"), None), Ok(Some("claude".into())));
        assert_eq!(
            resolve(None, os("claude\n").as_ref()),
            Ok(Some("claude".into()))
        );
        assert_eq!(
            resolve(Some("claude"), os("\tcodex\r\n").as_ref()),
            Ok(Some("claude".into()))
        );
    }

    // ---- rows 13 and 16: how the lookup picks an Agent -------------------------------------------

    fn store_with(turtle: &str) -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("resources.ttl"), turtle).unwrap();
        (hali::storage::load(dir.path()).unwrap(), dir)
    }

    #[test]
    fn first_by_iri_among_duplicates() {
        let hi = "ffffffff-ffff-7fff-bfff-ffffffffffff";
        let lo = "00000000-0000-7000-8000-000000000000";
        let turtle = format!(
            "<urn:uuid:{hi}> <{RDF_TYPE}> <{PROV}SoftwareAgent> ; <{NAME}> \"dup\" .\n\
             <urn:uuid:{lo}> <{RDF_TYPE}> <{PROV}SoftwareAgent> ; <{NAME}> \"dup\" .\n"
        );
        let (mut store, _dir) = store_with(&turtle);
        let want = ids(1); // Resource, record, Person
        resource::add(
            &ctx(1, Some("dup")),
            &mut store,
            &facts("/data/notes.txt"),
            None,
            "nate",
        )
        .unwrap();
        let after = triples(&store);
        assert_eq!(count_typed(&after, "SoftwareAgent"), 2, "no Agent minted");
        assert!(after.contains(&nt(
            &urn(&want[1]),
            &format!("{PROV}wasAttributedTo"),
            &format!("<{}>", urn(lo))
        )));
    }

    #[test]
    fn person_with_the_same_name_is_not_reused() {
        let (mut store, _dir) = empty_store();
        resource::add(
            &ctx(1, None),
            &mut store,
            &facts("/data/notes.txt"),
            None,
            "nate",
        )
        .unwrap();
        let want = ids(2); // Resource, record, SoftwareAgent
        resource::add(
            &ctx(2, Some("nate")),
            &mut store,
            &facts("/data/other.txt"),
            None,
            "nate",
        )
        .unwrap();
        let after = triples(&store);
        assert_eq!(count_typed(&after, "SoftwareAgent"), 1);
        assert_eq!(count_typed(&after, "Person"), 1);
        assert!(after.contains(&nt(
            &urn(&want[1]),
            &format!("{PROV}wasAttributedTo"),
            &format!("<{}>", urn(&want[2]))
        )));
        assert!(after.contains(&nt(
            &urn(&want[2]),
            &format!("{PROV}actedOnBehalfOf"),
            &format!("<{}>", urn(P))
        )));
    }

    // ---- row 4: the same name reuses the Agent ---------------------------------------------

    #[test]
    fn same_name_reuses() {
        let (mut store, _dir) = empty_store();
        resource::add(
            &ctx(1, Some("claude")),
            &mut store,
            &facts("/data/notes.txt"),
            None,
            "nate",
        )
        .unwrap();

        // A second run names it with spaces around: the name resolves to the same text.
        let name = agent_name(Some("  claude "), None).unwrap();
        assert_eq!(name.as_deref(), Some("claude"));
        let second = ctx(2, name.as_deref());
        let want_ids = ids(2); // Resource, record: no Person and no SoftwareAgent to mint
        let before = triples(&store);
        let out =
            resource::add(&second, &mut store, &facts("/data/other.txt"), None, "nate").unwrap();
        assert_eq!(out.value.record.id, want_ids[0]);

        let after = triples(&store);
        assert_eq!(count_typed(&after, "SoftwareAgent"), 1);
        assert_eq!(count_typed(&after, "Person"), 1);
        let attributed = nt(
            &urn(&want_ids[1]),
            &format!("{PROV}wasAttributedTo"),
            &format!("<{}>", urn(A)),
        );
        assert!(after.contains(&attributed), "{attributed}");
        // Only the new Resource and record triples were added: nothing mentions a third id.
        let added: BTreeSet<_> = after.difference(&before).cloned().collect();
        for t in &added {
            assert!(
                t.contains(&want_ids[0]) || t.contains(&want_ids[1]),
                "unexpected new triple: {t}"
            );
        }
        let changed: BTreeSet<String> = out.changes.iter().map(|c| c.triple.clone()).collect();
        assert_eq!(changed, added);
    }

    // ---- row 5: a different name mints a second Agent; the first run mints the Person ------

    #[test]
    fn new_name_mints_with_person() {
        let (mut store, _dir) = empty_store();
        resource::add(
            &ctx(1, Some("claude")),
            &mut store,
            &facts("/data/notes.txt"),
            None,
            "nate",
        )
        .unwrap();
        // Id order: Resource, record, Person, SoftwareAgent.
        assert_eq!(
            triples(&store),
            expected(R1, C1, A, &[(P, "nate")], &[(A, "claude", P)])
        );

        // A different name (case matters) mints a second Agent acting for the same Person.
        let want_ids = ids(2); // Resource, record, SoftwareAgent
        let out = resource::add(
            &ctx(2, Some("Claude")),
            &mut store,
            &facts("/data/other.txt"),
            None,
            "nate",
        )
        .unwrap();
        assert_eq!(out.value.record.id, want_ids[0]);
        let after = triples(&store);
        assert_eq!(count_typed(&after, "SoftwareAgent"), 2);
        assert_eq!(count_typed(&after, "Person"), 1);
        let second_agent = &want_ids[2];
        for t in [
            nt(
                &urn(second_agent),
                RDF_TYPE,
                &format!("<{PROV}SoftwareAgent>"),
            ),
            nt(&urn(second_agent), NAME, "\"Claude\""),
            nt(
                &urn(second_agent),
                &format!("{PROV}actedOnBehalfOf"),
                &format!("<{}>", urn(P)),
            ),
            nt(
                &urn(&want_ids[1]),
                &format!("{PROV}wasAttributedTo"),
                &format!("<{}>", urn(second_agent)),
            ),
        ] {
            assert!(after.contains(&t), "missing: {t}");
        }
        // The first record is still attributed to the first Agent.
        assert!(after.contains(&nt(
            &urn(C1),
            &format!("{PROV}wasAttributedTo"),
            &format!("<{}>", urn(A))
        )));
    }

    // ---- row 10: the name is one inert literal ---------------------------------------------

    #[test]
    fn name_is_inert_literal() {
        let names = [
            "x\" ; a <urn:evil:Resource> . #",
            "a\\\"b\\",
            "<urn:x> ] [ ) ( } {",
            "SELECT ?s WHERE { ?s ?p ?o } # \" ; DROP",
            "'''\"\"\"''' @en ^^<urn:t>",
            "\u{e9}t\u{e9} \u{1f600} \u{202e}",
            "claude\\u0041",
        ];
        for name in names {
            let (mut store, dir) = empty_store();
            let out = resource::add(
                &ctx(1, Some(name)),
                &mut store,
                &facts("/data/notes.txt"),
                None,
                "nate",
            )
            .unwrap();
            let got = triples(&store);
            let want = expected(R1, C1, A, &[(P, "nate")], &[(A, "claude", P)]);
            // Everything but the name literal is as for an ordinary agent.
            let strip = |set: &BTreeSet<String>| -> BTreeSet<String> {
                set.iter()
                    .filter(|t| !t.starts_with(&format!("<{}> <{NAME}>", urn(A))))
                    .cloned()
                    .collect()
            };
            assert_eq!(strip(&got), strip(&want), "name {name:?}");
            assert_eq!(got.len(), want.len(), "no extra triples for {name:?}");
            assert_eq!(out.changes.len(), want.len());

            // The literal is a simple string holding exactly the name.
            let ttl = hali::storage::to_turtle(&store, File::Resources);
            let literals: Vec<_> = oxttl::TurtleParser::new()
                .for_slice(ttl.as_bytes())
                .map(|t| t.unwrap())
                .filter(|t| {
                    t.predicate.as_str() == NAME && t.subject.to_string() == format!("<{}>", urn(A))
                })
                .collect();
            assert_eq!(literals.len(), 1, "name {name:?}");
            let Term::Literal(lit) = &literals[0].object else {
                panic!("the name is not a literal: {name:?}");
            };
            assert_eq!(lit.value(), name);
            assert_eq!(lit.datatype().as_str(), format!("{XSD}string"));
            assert!(lit.language().is_none());

            // Reload and re-serialize: byte-identical; and the lookup finds the agent again.
            std::fs::write(dir.path().join("resources.ttl"), &ttl).unwrap();
            let mut reloaded = hali::storage::load(dir.path()).unwrap();
            assert_eq!(hali::storage::to_turtle(&reloaded, File::Resources), ttl);
            resource::add(
                &ctx(2, Some(name)),
                &mut reloaded,
                &facts("/data/other.txt"),
                None,
                "nate",
            )
            .unwrap();
            let after = triples(&reloaded);
            assert_eq!(count_typed(&after, "SoftwareAgent"), 1, "name {name:?}");
            assert_eq!(count_typed(&after, "Person"), 1, "name {name:?}");
        }
    }
}
