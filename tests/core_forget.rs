//! Goal tests for `forget` in `core` (#14, spec §2.6, §9.4, §9.8).
//!
//! Assumed API (invented here; the implementer follows it):
//!   `resource::forget(&Context, &mut Store, id: &str, force: bool) -> Result<Mutation<Record>, Error>`
//!       removes, from the resources graph only, every quad whose subject is the Resource, its
//!       checksum node or its catalog record, and every quad whose object is the Resource; the
//!       `a dcterms:MediaType` triple of a non-IANA format node goes only when no remaining
//!       Resource uses that format. One SHACL guard over the removals, one `apply`; no clock and
//!       no id is used. The value is the record as it was (`missing`/`stale` left false); every
//!       change is `Op::Remove`. An id that is no cataloged Resource is `UNKNOWN_RESOURCE`.
//!   `Error::confirmation_required(command: &str, retry: &str, declined: bool) -> Error`
//!       rule `CONFIRMATION_REQUIRED`, exit code 1, `details {"command", "declined"}`,
//!       `suggested_command == retry`.
//!
//! The tests read tests/fixtures/forget: the user Person; R1 (IANA, tagged `ml`); R2 and R3, both
//! with the non-IANA format `text/x-forget`, R3 `dcterms:relation` R2 (an inbound triple of R2);
//! one catalog record each (ids 0211, 0212, 0213). All three files have six bytes and `modified`
//! 2026-10-20T14:02:11Z.

mod goal {
    use hali::core::resource;
    use hali::core::{Clock, Context, Error, FixedClock, IdGen, Op, SeededIdGen};
    use hali::storage::{File, Store};
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    const P: &str = "urn:uuid:0192f0c4-0000-7000-8000-000000000001";
    const R1: &str = "0192f0c4-0000-7000-8000-000000000201";
    const R2: &str = "0192f0c4-0000-7000-8000-000000000202";
    const R3: &str = "0192f0c4-0000-7000-8000-000000000203";
    const C1: &str = "0192f0c4-0000-7000-8000-000000000211";
    const C2: &str = "0192f0c4-0000-7000-8000-000000000212";
    const C3: &str = "0192f0c4-0000-7000-8000-000000000213";
    const FORMAT: &str = "https://w3id.org/hali/media-types/text/x-forget";
    const SHA: &str = "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03";
    const XSD: &str = "http://www.w3.org/2001/XMLSchema#";

    fn ctx() -> Context {
        Context::new(
            "en",
            Box::new(FixedClock::new("2026-10-24T09:30:00Z")),
            Box::new(SeededIdGen::new(1)),
        )
    }

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    fn load() -> Store {
        hali::storage::load(&fixture("forget")).unwrap()
    }

    fn resources_ttl(store: &Store) -> String {
        hali::storage::to_turtle(store, File::Resources)
    }

    fn vocabulary_ttl(store: &Store) -> String {
        hali::storage::to_turtle(store, File::Vocabulary)
    }

    /// The triples of a Turtle text, as the N-Triples strings `changes` uses.
    fn triples(turtle: &str) -> BTreeSet<String> {
        oxttl::TurtleParser::new()
            .for_slice(turtle.as_bytes())
            .map(|t| format!("{} .", t.unwrap()))
            .collect()
    }

    fn urn(id: &str) -> String {
        format!("urn:uuid:{id}")
    }

    /// Everything a forget of R2 removes, in the order `changes` must list it. The order is the
    /// plain byte order of the whole triple: the checksum node (`#` sorts before `>`), the
    /// Resource, R3's inbound relation, the catalog record.
    fn r2_removals() -> Vec<String> {
        let r2 = urn(R2);
        let c2 = urn(C2);
        let r3 = urn(R3);
        vec![
            format!(
                "<{r2}#sha256> <http://spdx.org/rdf/terms#algorithm> <http://spdx.org/rdf/terms#checksumAlgorithm_sha256> ."
            ),
            format!(
                "<{r2}#sha256> <http://spdx.org/rdf/terms#checksumValue> \"{SHA}\"^^<{XSD}hexBinary> ."
            ),
            format!("<{r2}> <http://purl.org/dc/terms/format> <{FORMAT}> ."),
            format!(
                "<{r2}> <http://purl.org/dc/terms/modified> \"2026-10-20T14:02:11Z\"^^<{XSD}dateTime> ."
            ),
            format!("<{r2}> <http://purl.org/dc/terms/title> \"two doc\" ."),
            format!("<{r2}> <http://spdx.org/rdf/terms#checksum> <{r2}#sha256> ."),
            format!(
                "<{r2}> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <https://w3id.org/hali#Resource> ."
            ),
            format!("<{r2}> <http://www.w3.org/ns/dcat#byteSize> \"6\"^^<{XSD}integer> ."),
            format!(
                "<{r2}> <http://www.w3.org/ns/dcat#downloadURL> <file:///fixtures/forget/two.txt> ."
            ),
            format!("<{r3}> <http://purl.org/dc/terms/relation> <{r2}> ."),
            format!(
                "<{c2}> <http://purl.org/dc/terms/issued> \"2026-10-21T08:00:00Z\"^^<{XSD}dateTime> ."
            ),
            format!(
                "<{c2}> <http://purl.org/dc/terms/modified> \"2026-10-21T08:00:00Z\"^^<{XSD}dateTime> ."
            ),
            format!(
                "<{c2}> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.w3.org/ns/dcat#CatalogRecord> ."
            ),
            format!("<{c2}> <http://www.w3.org/ns/prov#wasAttributedTo> <{P}> ."),
            format!("<{c2}> <http://xmlns.com/foaf/0.1/primaryTopic> <{r2}> ."),
        ]
    }

    fn format_node() -> String {
        format!(
            "<{FORMAT}> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://purl.org/dc/terms/MediaType> ."
        )
    }

    // ---- criterion 1: what forget removes and what it keeps -----------------------------------

    #[test]
    fn forget_removes_resource_record_checksum_and_inbound() {
        let mut store = load();
        let before = triples(&resources_ttl(&store));
        let forgotten = resource::forget(&ctx(), &mut store, R2, true).unwrap();

        // The value is the record as it was.
        let record = &forgotten.value;
        assert_eq!(record.id, R2);
        assert_eq!(record.title, "two doc");
        assert_eq!(record.path, "/fixtures/forget/two.txt");
        assert_eq!(record.media_type, "text/x-forget");
        assert_eq!(record.byte_size, 6);
        assert_eq!(record.modified, "2026-10-20T14:02:11Z");
        assert!(record.tags.is_empty());

        // Nothing about R2, its checksum node or its catalog record is left, and nothing
        // points at R2: the inbound `R3 dcterms:relation R2` went too.
        let text = resources_ttl(&store);
        for gone in [R2, C2] {
            assert!(
                !text.contains(gone),
                "{gone} still in resources.ttl:\n{text}"
            );
        }
        let after = triples(&text);
        let removed: BTreeSet<String> = before.difference(&after).cloned().collect();
        assert_eq!(removed, r2_removals().into_iter().collect::<BTreeSet<_>>());
        assert!(
            after.is_subset(&before),
            "forget added a triple: {:?}",
            after.difference(&before).collect::<Vec<_>>()
        );

        // The tagged IANA Resource: the record carries its tag as it was.
        let mut store = load();
        let r1 = resource::forget(&ctx(), &mut store, R1, false).unwrap();
        assert_eq!(r1.value.media_type, "text/plain");
        let tags: Vec<&str> = r1.value.tags.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(tags, ["ml"]);
    }

    #[test]
    fn forget_changes_are_sorted_removes() {
        let mut store = load();
        let forgotten = resource::forget(&ctx(), &mut store, R2, true).unwrap();
        assert!(forgotten.changes.iter().all(|c| c.op == Op::Remove));
        let listed: Vec<&str> = forgotten
            .changes
            .iter()
            .map(|c| c.triple.as_str())
            .collect();
        assert_eq!(listed, r2_removals());
        // No `dcterms:modified` bump: nothing is added, and no triple is listed twice.
        assert_eq!(
            forgotten.changes.iter().collect::<BTreeSet<_>>().len(),
            forgotten.changes.len()
        );
    }

    #[test]
    fn forget_keeps_person_and_other_resources() {
        for (id, record) in [(R1, C1), (R2, C2), (R3, C3)] {
            let mut store = load();
            let vocabulary = vocabulary_ttl(&store);
            let before = triples(&resources_ttl(&store));
            resource::forget(&ctx(), &mut store, id, true).unwrap();
            let text = resources_ttl(&store);
            let after = triples(&text);

            // Exactly the triples that mention the Resource (or its checksum node, which
            // extends its IRI) or its catalog record are gone; the format node stays, as
            // another Resource still uses it (R1 has none).
            let own = [urn(id), urn(record)];
            let expected: BTreeSet<String> = before
                .iter()
                .filter(|t| own.iter().any(|o| t.contains(o.as_str())))
                .cloned()
                .collect();
            assert!(!expected.is_empty());
            let removed: BTreeSet<String> = before.difference(&after).cloned().collect();
            assert_eq!(removed, expected, "{id}");

            // The Person and the other Resources are intact.
            assert!(
                after.contains(&format!(
                    "<{P}> <http://xmlns.com/foaf/0.1/name> \"nate\" ."
                )),
                "{id}: the Person is gone:\n{text}"
            );
            for other in [R1, R2, R3].into_iter().filter(|o| *o != id) {
                assert!(text.contains(other), "{id}: {other} was removed too");
            }
            assert!(after.contains(&format_node()), "{id}: the format node went");

            // Concepts live in vocabulary.ttl, which forget does not touch.
            assert_eq!(vocabulary_ttl(&store), vocabulary);
        }
    }

    #[test]
    fn forget_drops_orphan_format_only() {
        // R3 first: R2 still uses the format, so the format node stays, in the store and out
        // of the changes.
        let mut store = load();
        let first = resource::forget(&ctx(), &mut store, R3, false).unwrap();
        assert!(!first.changes.iter().any(|c| c.triple == format_node()));
        assert!(triples(&resources_ttl(&store)).contains(&format_node()));
        // Then R2, the last user: the node goes with it.
        let last = resource::forget(&ctx(), &mut store, R2, false).unwrap();
        assert!(
            last.changes
                .iter()
                .any(|c| c.op == Op::Remove && c.triple == format_node()),
            "{:?}",
            last.changes
        );
        assert!(!triples(&resources_ttl(&store)).contains(&format_node()));

        // The other order: R2 keeps the node for R3, and R3 then drops it.
        let mut store = load();
        let first = resource::forget(&ctx(), &mut store, R2, true).unwrap();
        assert!(!first.changes.iter().any(|c| c.triple == format_node()));
        let last = resource::forget(&ctx(), &mut store, R3, false).unwrap();
        assert!(last.changes.iter().any(|c| c.triple == format_node()));

        // R1 is IANA: it has no format node to drop, and R2 and R3 keep theirs.
        let mut store = load();
        let r1 = resource::forget(&ctx(), &mut store, R1, false).unwrap();
        assert!(!r1.changes.iter().any(|c| c.triple == format_node()));
        assert!(triples(&resources_ttl(&store)).contains(&format_node()));
    }

    #[test]
    fn forget_unknown_is_unknown_resource() {
        let mut store = load();
        let before = resources_ttl(&store);
        for unknown in [
            "0192f0c4-0000-7000-8000-00000000ffff", // not cataloged
            "0192f0c4-0000-7000-8000-000000000001", // the Person
            C2,                                     // a catalog record
            "no-such-id",
        ] {
            let err: Error = resource::forget(&ctx(), &mut store, unknown, false).unwrap_err();
            assert_eq!(err.rule, "UNKNOWN_RESOURCE", "{unknown}");
            assert_eq!(err.exit_code(), 1);
            assert_eq!(err.details["ref"], unknown);
        }
        assert_eq!(resources_ttl(&store), before);

        // A Resource that was forgotten is unknown, not a silent success.
        resource::forget(&ctx(), &mut store, R1, false).unwrap();
        let err = resource::forget(&ctx(), &mut store, R1, false).unwrap_err();
        assert_eq!(err.rule, "UNKNOWN_RESOURCE");
    }

    #[test]
    fn forget_guard_sees_removed_quads() {
        // extensions.ttl still types R1 as a hali:Resource: without the Resource's own triples
        // that does not conform, so the forget is refused and nothing changes.
        let dir = tempfile::tempdir().unwrap();
        for name in ["vocabulary.ttl", "resources.ttl"] {
            std::fs::copy(fixture("forget").join(name), dir.path().join(name)).unwrap();
        }
        std::fs::write(
            dir.path().join("extensions.ttl"),
            format!(
                "@prefix hali: <https://w3id.org/hali#> .\n<urn:uuid:{R1}> a hali:Resource .\n"
            ),
        )
        .unwrap();
        let mut store = hali::storage::load(dir.path()).unwrap();
        let before = resources_ttl(&store);
        let err = resource::forget(&ctx(), &mut store, R1, false).unwrap_err();
        assert_eq!(err.rule, "SHACL_VIOLATION");
        assert_eq!(err.exit_code(), 1);
        let violations = err.details["violations"].as_array().unwrap();
        assert!(
            violations
                .iter()
                .any(|v| v["focus_node"] == format!("<urn:uuid:{R1}>")),
            "{violations:?}"
        );
        assert_eq!(resources_ttl(&store), before, "nothing was removed");
    }

    /// The fixture with `edit` applied to its resources.ttl, loaded from a temp dir.
    fn load_edited(edit: impl Fn(String) -> String) -> Store {
        let dir = tempfile::tempdir().unwrap();
        std::fs::copy(
            fixture("forget").join("vocabulary.ttl"),
            dir.path().join("vocabulary.ttl"),
        )
        .unwrap();
        let text = std::fs::read_to_string(fixture("forget").join("resources.ttl")).unwrap();
        std::fs::write(dir.path().join("resources.ttl"), edit(text)).unwrap();
        hali::storage::load(dir.path()).unwrap()
    }

    fn about(ts: &BTreeSet<String>, subject: &str) -> BTreeSet<String> {
        let prefix = format!("<{subject}>");
        ts.iter()
            .filter(|t| t.starts_with(&prefix))
            .cloned()
            .collect()
    }

    #[test]
    fn forget_keeps_a_shared_checksum_node() {
        // R1 also links R2's checksum node: forgetting R1 removes its own node only.
        let link = format!(
            "<{}> <http://spdx.org/rdf/terms#checksum> <{}#sha256> .\n",
            urn(R1),
            urn(R2)
        );
        let mut store = load_edited(|t| t + &link);
        let r2_node = format!("{}#sha256", urn(R2));
        let before = triples(&resources_ttl(&store));
        assert_eq!(about(&before, &r2_node).len(), 2);
        let m = resource::forget(&ctx(), &mut store, R1, false).unwrap();
        let after = triples(&resources_ttl(&store));
        assert_eq!(about(&after, &r2_node), about(&before, &r2_node));
        assert!(about(&after, &format!("{}#sha256", urn(R1))).is_empty());
        assert!(m.changes.iter().all(|c| c.op == Op::Remove));
        assert_eq!(after.len() + m.changes.len(), before.len());
    }

    #[test]
    fn forget_leaves_a_checksum_pointing_at_another_resource() {
        // R1's only checksum link points at R2 itself: R2 stays completely intact.
        let own = format!("spdx:checksum <{}#sha256>", urn(R1));
        let mut store = load_edited(|t| {
            let edited = t.replacen(&own, &format!("spdx:checksum <{}>", urn(R2)), 1);
            assert_ne!(edited, t);
            edited
        });
        let before = triples(&resources_ttl(&store));
        let m = resource::forget(&ctx(), &mut store, R1, false).unwrap();
        let after = triples(&resources_ttl(&store));
        for s in [urn(R2), format!("{}#sha256", urn(R2))] {
            assert_eq!(about(&after, &s), about(&before, &s), "{s}");
        }
        // R1's own node is no longer linked from R1, so it is left too; the guard is happy.
        assert!(!about(&after, &format!("{}#sha256", urn(R1))).is_empty());
        let removed: BTreeSet<String> = m.changes.iter().map(|c| c.triple.clone()).collect();
        let expected: BTreeSet<String> = about(&before, &urn(R1))
            .union(&about(&before, &urn(C1)))
            .cloned()
            .collect();
        assert_eq!(removed, expected, "nothing else is removed");
    }

    #[test]
    fn forget_removes_every_catalog_record_of_the_resource() {
        let c1b = "0192f0c4-0000-7000-8000-0000000002ff";
        let extra = format!(
            "<{}> a dcat:CatalogRecord ;\n\tdcterms:issued \"2026-10-22T08:00:00Z\"^^xsd:dateTime ;\n\tdcterms:modified \"2026-10-22T08:00:00Z\"^^xsd:dateTime ;\n\tprov:wasAttributedTo <{P}> ;\n\tfoaf:primaryTopic <{}> .\n",
            urn(c1b),
            urn(R1)
        );
        let mut store = load_edited(|t| t + &extra);
        resource::forget(&ctx(), &mut store, R1, false).unwrap();
        let text = resources_ttl(&store);
        for id in [C1, c1b, R1] {
            assert!(!text.contains(&urn(id)), "{id} remains:\n{text}");
        }
        assert!(text.contains(&urn(C2)) && text.contains(&urn(R2)));
    }

    #[test]
    fn forget_needs_no_clock_or_ids() {
        struct NoClock;
        impl Clock for NoClock {
            fn now(&self) -> String {
                panic!("forget read the clock")
            }
        }
        struct NoIds;
        impl IdGen for NoIds {
            fn next_id(&self) -> String {
                panic!("forget minted an id")
            }
        }
        let ctx = Context::new("en", Box::new(NoClock), Box::new(NoIds));
        let mut store = load();
        resource::forget(&ctx, &mut store, R1, false).unwrap();
    }

    // ---- criterion 3: the confirmation rule -----------------------------------------------------

    #[test]
    fn confirmation_required_error() {
        let retry = format!("hali forget {R1} --yes");
        let err = Error::confirmation_required("forget", &retry, false);
        assert_eq!(err.rule, "CONFIRMATION_REQUIRED");
        assert_eq!(err.exit_code(), 1);
        assert_eq!(
            err.message,
            "forget needs confirmation, but stdin or stderr is not a terminal; nothing was written"
        );
        assert_eq!(
            *err.details,
            serde_json::json!({"command": "forget", "declined": false})
        );
        assert_eq!(
            err.suggested_fix.as_deref(),
            Some(
                "Run it again with --yes to confirm, or with --dry-run to see what it would change."
            )
        );
        assert_eq!(err.suggested_command.as_deref(), Some(retry.as_str()));

        // Declining at the prompt raises the same rule.
        let err = Error::confirmation_required("forget", &retry, true);
        assert_eq!(err.rule, "CONFIRMATION_REQUIRED");
        assert_eq!(err.exit_code(), 1);
        assert_eq!(err.message, "forget was not confirmed; nothing was written");
        assert_eq!(
            *err.details,
            serde_json::json!({"command": "forget", "declined": true})
        );
        assert_eq!(err.suggested_command.as_deref(), Some(retry.as_str()));
    }
}
