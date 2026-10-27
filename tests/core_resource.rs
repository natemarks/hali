//! Goal tests for `core::resource::{add, edit_title}` (#10, spec §2.1, §2.5).
//!
//! Assumed API (the file facts are injected, so core never touches the filesystem):
//!   `hali::core::resource::FileFacts { path: String, byte_size: u64, modified: String,
//!        sha256: String, media_type: String }`
//!       `path` is the real path (UTF-8, not yet percent-encoded), `modified` is
//!       `YYYY-MM-DDTHH:MM:SSZ`, `sha256` is lowercase hex, `media_type` is `type/subtype`.
//!   `resource::add(&Context, &mut Store, &FileFacts, title: Option<&str>, user_name: &str)
//!        -> Result<Mutation<Added>, hali::core::Error>`
//!   `Added { record: Record, existing: bool }`
//!   `Record { id: String (bare UUID), path: String, title: String, media_type: String,
//!        byte_size: u64, modified: String, .. }`
//!   `resource::edit_title(&Context, &mut Store, id: &str, title: &str)
//!        -> Result<Mutation<Record>, hali::core::Error>`
//!   Ids are minted in the order Resource, catalog record, user Person (first time only).
//!
//! Expected triples are written out here from spec §2.1 and §2.5. Oxigraph stores
//! `xsd:nonNegativeInteger` as `xsd:integer`, so `dcat:byteSize` is an `xsd:integer`.

mod goal {
    use hali::core::resource::{self, FileFacts};
    use hali::core::{Context, FixedClock, Op, SeededIdGen};
    use hali::storage::{File, Store};
    use std::collections::BTreeSet;

    // SeededIdGen::new(1) yields these ids in order.
    const R1: &str = "018bcfe5-6801-710a-adec-89025cc1beeb";
    const C1: &str = "018bcfe5-6802-7893-a2ee-fb32555e71c1";
    const P: &str = "018bcfe5-6803-71bb-94d8-d101b5b9c34d";
    const R2: &str = "018bcfe5-6804-7099-ac6c-d7363ca585e7";
    const C2: &str = "018bcfe5-6805-7917-98de-357e3da8cb43";

    const NOW: &str = "2026-10-24T09:30:00Z";
    const XSD: &str = "http://www.w3.org/2001/XMLSchema#";
    const SHA: &str = "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03";

    fn ctx_at(now: &str) -> Context {
        Context::new(
            "en",
            Box::new(FixedClock::new(now)),
            Box::new(SeededIdGen::new(1)),
        )
    }

    fn empty_store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        (hali::storage::load(dir.path()).unwrap(), dir)
    }

    fn facts(path: &str, media_type: &str) -> FileFacts {
        FileFacts {
            path: path.to_string(),
            byte_size: 6,
            modified: "2026-10-20T14:02:11Z".to_string(),
            sha256: SHA.to_string(),
            media_type: media_type.to_string(),
        }
    }

    /// Every triple of resources.ttl as canonical N-Triples.
    fn triples(store: &Store) -> BTreeSet<String> {
        let ttl = hali::storage::to_turtle(store, File::Resources);
        oxttl::TurtleParser::new()
            .for_slice(ttl.as_bytes())
            .map(|t| format!("{} .", t.expect("resources.ttl is Turtle")))
            .collect()
    }

    fn nt(subject: &str, predicate: &str, object: &str) -> String {
        format!("<{subject}> <{predicate}> {object} .")
    }

    fn iri(s: &str) -> String {
        format!("<{s}>")
    }

    fn dt(v: &str) -> String {
        format!("\"{v}\"^^<{XSD}dateTime>")
    }

    const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";

    /// The triples `add` writes for the first file, with the user named `user`.
    fn first_add_triples(
        url: &str,
        title: &str,
        media: BTreeSet<String>,
        user: &str,
    ) -> BTreeSet<String> {
        let r = format!("urn:uuid:{R1}");
        let sum = format!("urn:uuid:{R1}#sha256");
        let c = format!("urn:uuid:{C1}");
        let p = format!("urn:uuid:{P}");
        let mut want = BTreeSet::new();
        for t in [
            nt(&r, RDF_TYPE, "<https://w3id.org/hali#Resource>"),
            nt(
                &r,
                "http://purl.org/dc/terms/title",
                &format!("\"{title}\""),
            ),
            nt(&r, "http://www.w3.org/ns/dcat#downloadURL", &iri(url)),
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
            nt(&r, "http://spdx.org/rdf/terms#checksum", &iri(&sum)),
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
            nt(&c, "http://xmlns.com/foaf/0.1/primaryTopic", &iri(&r)),
            nt(&c, "http://purl.org/dc/terms/issued", &dt(NOW)),
            nt(&c, "http://purl.org/dc/terms/modified", &dt(NOW)),
            nt(&c, "http://www.w3.org/ns/prov#wasAttributedTo", &iri(&p)),
            nt(&p, RDF_TYPE, "<http://www.w3.org/ns/prov#Person>"),
            nt(&p, "http://xmlns.com/foaf/0.1/name", &format!("\"{user}\"")),
        ] {
            want.insert(t);
        }
        want.extend(media);
        want
    }

    fn iana_plain() -> BTreeSet<String> {
        BTreeSet::from([nt(
            &format!("urn:uuid:{R1}"),
            "http://www.w3.org/ns/dcat#mediaType",
            "<http://www.iana.org/assignments/media-types/text/plain>",
        )])
    }

    #[test]
    fn add_writes_resource_triples() {
        let (mut store, _dir) = empty_store();
        let f = facts("/data/my notes.txt", "text/plain");
        let out = resource::add(&ctx_at(NOW), &mut store, &f, None, "nate").unwrap();

        // The record: the bare UUIDv7, the default title (file name without extension).
        let record = &out.value.record;
        assert!(!out.value.existing);
        assert_eq!(record.id, R1);
        assert_eq!(record.title, "my notes");
        assert_eq!(record.path, "/data/my notes.txt");
        assert_eq!(record.media_type, "text/plain");
        assert_eq!(record.byte_size, 6);
        assert_eq!(record.modified, "2026-10-20T14:02:11Z");

        // The graph: downloadURL is `file://` + the real path, percent-encoded.
        let want = first_add_triples(
            "file:///data/my%20notes.txt",
            "my notes",
            iana_plain(),
            "nate",
        );
        assert_eq!(triples(&store), want);

        // The changes are exactly those triples, as sorted `add`s.
        assert!(out.changes.iter().all(|c| c.op == Op::Add));
        let changed: BTreeSet<String> = out.changes.iter().map(|c| c.triple.clone()).collect();
        assert_eq!(changed, want);
        assert_eq!(out.changes.len(), want.len());
        let mut sorted = out.changes.clone();
        sorted.sort();
        assert_eq!(out.changes, sorted);
    }

    #[test]
    fn add_title_override() {
        let (mut store, _dir) = empty_store();
        let f = facts("/data/my notes.txt", "text/plain");
        let out = resource::add(&ctx_at(NOW), &mut store, &f, Some("Field Notes"), "nate").unwrap();
        assert_eq!(out.value.record.title, "Field Notes");
        let want = first_add_triples(
            "file:///data/my%20notes.txt",
            "Field Notes",
            iana_plain(),
            "nate",
        );
        assert_eq!(triples(&store), want);
    }

    #[test]
    fn x_type_uses_dcterms_format() {
        let (mut store, _dir) = empty_store();
        let f = facts("/data/run.sh", "application/x-shellscript");
        let out = resource::add(&ctx_at(NOW), &mut store, &f, None, "nate").unwrap();
        assert_eq!(out.value.record.media_type, "application/x-shellscript");
        let format_iri = "https://w3id.org/hali/media-types/application/x-shellscript";
        let media = BTreeSet::from([
            nt(
                &format!("urn:uuid:{R1}"),
                "http://purl.org/dc/terms/format",
                &iri(format_iri),
            ),
            nt(format_iri, RDF_TYPE, "<http://purl.org/dc/terms/MediaType>"),
        ]);
        let want = first_add_triples("file:///data/run.sh", "run", media, "nate");
        assert_eq!(triples(&store), want);
        assert!(
            !triples(&store)
                .iter()
                .any(|t| t.contains("<http://www.w3.org/ns/dcat#mediaType>")),
            "an x- type has no dcat:mediaType"
        );
    }

    #[test]
    fn add_writes_catalog_record_and_mints_user_once() {
        let (mut store, _dir) = empty_store();
        let ctx = ctx_at(NOW);
        let first = resource::add(
            &ctx,
            &mut store,
            &facts("/data/a.txt", "text/plain"),
            None,
            "first",
        )
        .unwrap();
        assert_eq!(first.value.record.id, R1);

        // The second add mints Resource then catalog record, and no Person; the name given
        // now is ignored because the Person's name is fixed once minted.
        let second = resource::add(
            &ctx,
            &mut store,
            &facts("/data/b.txt", "text/plain"),
            None,
            "second",
        )
        .unwrap();
        assert_eq!(second.value.record.id, R2);
        assert!(
            !second
                .changes
                .iter()
                .any(|c| c.triple.contains("prov#Person") || c.triple.contains("foaf/0.1/name")),
            "{:?}",
            second.changes
        );

        let all = triples(&store);
        let persons: Vec<&String> = all.iter().filter(|t| t.contains("prov#Person")).collect();
        assert_eq!(persons.len(), 1, "{persons:?}");
        assert!(all.contains(&nt(
            &format!("urn:uuid:{P}"),
            "http://xmlns.com/foaf/0.1/name",
            "\"first\""
        )));
        assert!(!all.iter().any(|t| t.contains("\"second\"")));
        for (c, r) in [(C1, R1), (C2, R2)] {
            let c = format!("urn:uuid:{c}");
            let r = format!("urn:uuid:{r}");
            assert!(all.contains(&nt(
                &c,
                RDF_TYPE,
                "<http://www.w3.org/ns/dcat#CatalogRecord>"
            )));
            assert!(all.contains(&nt(&c, "http://xmlns.com/foaf/0.1/primaryTopic", &iri(&r))));
            assert!(all.contains(&nt(&c, "http://purl.org/dc/terms/issued", &dt(NOW))));
            assert!(all.contains(&nt(&c, "http://purl.org/dc/terms/modified", &dt(NOW))));
            assert!(all.contains(&nt(
                &c,
                "http://www.w3.org/ns/prov#wasAttributedTo",
                &iri(&format!("urn:uuid:{P}"))
            )));
        }
    }

    #[test]
    fn re_add_returns_existing_no_changes() {
        let (mut store, _dir) = empty_store();
        let f = facts("/data/a.txt", "text/plain");
        let first = resource::add(&ctx_at(NOW), &mut store, &f, None, "nate").unwrap();
        let before = hali::storage::to_turtle(&store, File::Resources);

        // A later clock, another title and another user change nothing.
        let again = resource::add(
            &ctx_at("2026-10-30T00:00:00Z"),
            &mut store,
            &f,
            Some("Other title"),
            "someone else",
        )
        .unwrap();
        assert!(again.value.existing);
        assert!(again.changes.is_empty(), "{:?}", again.changes);
        assert_eq!(again.value.record.id, first.value.record.id);
        assert_eq!(again.value.record.title, "a");
        assert_eq!(hali::storage::to_turtle(&store, File::Resources), before);
    }

    #[test]
    fn edit_title_replaces_title_and_bumps_catalog_modified() {
        let (mut store, _dir) = empty_store();
        let f = facts("/data/a.txt", "text/plain");
        resource::add(&ctx_at(NOW), &mut store, &f, None, "nate").unwrap();

        let later = "2026-10-25T10:00:00Z";
        let out = resource::edit_title(&ctx_at(later), &mut store, R1, "A better title").unwrap();
        assert_eq!(out.value.title, "A better title");
        assert_eq!(out.value.id, R1);

        let r = format!("urn:uuid:{R1}");
        let c = format!("urn:uuid:{C1}");
        let want: BTreeSet<(Op, String)> = BTreeSet::from([
            (
                Op::Add,
                nt(&r, "http://purl.org/dc/terms/title", "\"A better title\""),
            ),
            (
                Op::Add,
                nt(&c, "http://purl.org/dc/terms/modified", &dt(later)),
            ),
            (
                Op::Remove,
                nt(&r, "http://purl.org/dc/terms/title", "\"a\""),
            ),
            (
                Op::Remove,
                nt(&c, "http://purl.org/dc/terms/modified", &dt(NOW)),
            ),
        ]);
        let got: BTreeSet<(Op, String)> = out
            .changes
            .iter()
            .map(|c| (c.op, c.triple.clone()))
            .collect();
        assert_eq!(got, want);
        let mut sorted = out.changes.clone();
        sorted.sort();
        assert_eq!(out.changes, sorted);

        // The graph has exactly one title and the catalog record's issued is untouched.
        let all = triples(&store);
        let titles = all
            .iter()
            .filter(|t| t.contains("<http://purl.org/dc/terms/title>"))
            .count();
        assert_eq!(titles, 1);
        assert!(all.contains(&nt(&c, "http://purl.org/dc/terms/issued", &dt(NOW))));
        assert!(all.contains(&nt(&c, "http://purl.org/dc/terms/modified", &dt(later))));

        // The same title is a no-op: no changes, nothing bumped.
        let before = hali::storage::to_turtle(&store, File::Resources);
        let same = resource::edit_title(
            &ctx_at("2026-11-01T00:00:00Z"),
            &mut store,
            R1,
            "A better title",
        )
        .unwrap();
        assert!(same.changes.is_empty(), "{:?}", same.changes);
        assert_eq!(hali::storage::to_turtle(&store, File::Resources), before);
    }

    #[test]
    fn edit_title_of_an_unknown_resource_is_unknown_resource() {
        let (mut store, _dir) = empty_store();
        let err = resource::edit_title(&ctx_at(NOW), &mut store, R1, "x").unwrap_err();
        assert_eq!(err.rule, "UNKNOWN_RESOURCE");
        assert_eq!(err.exit_code(), 1);
        assert_eq!(err.details["ref"], R1);
    }
}
