//! Goal tests for `core` of `hali new` (#25, spec §8, §9.1, §9.6).
//!
//! Assumed API (invented here; the implementer follows it).
//!
//! In `hali::core::resource`:
//!
//! ```text
//! pub struct NewFile {
//!     pub title: String,          // as given: `new_resource` normalises it (`normalise_title`)
//!     pub types: Vec<String>,     // Type references, as `add --type`
//!     pub tags: Vec<String>,      // Concept references, as `add --tag`
//!     pub dir_real: String,       // the real path of `files.dir`, UTF-8, no trailing `/`
//!     pub user_name: String,      // names the user Person when it is minted
//! }
//! pub struct Created {
//!     pub record: Record,         // `missing: false`, `stale: false`, `editable: true`
//!     pub path: String,           // `<dir_real>/<name>.md`
//!     pub content: String,        // exactly `# <title>\n`
//!     pub mtime: i64,             // `now` in seconds since the Unix epoch
//! }
//! pub fn new_resource(
//!     ctx: &Context, store: &mut Store, new: &NewFile,
//!     taken: impl Fn(&str) -> bool,   // gets a candidate full path; true when anything is there
//! ) -> Result<Mutation<Created>, Error>
//! ```
//!
//! `new_resource` is pure (no filesystem). It resolves the Types then the tags, normalises the
//! title, picks the first free name `<slug>.md`, `<slug>-2.md`, `-3`, ... (a name is taken when
//! `taken` says so or any Resource in the store records that path, a Missing one included),
//! mints ids in the order Resource, record, Person (first time), SoftwareAgent (first time, when
//! the context has an agent), runs the SHACL guard and applies the change. It writes exactly
//! what `add` writes for a file of `text/markdown`, byte size and SHA-256 of `content`, plus
//! `R dcterms:created now` and `R prov:wasAttributedTo <Agent>` (the Agent being the
//! SoftwareAgent when one acts, else the Person; the record is attributed to the same one).
//! Resource `modified`, `created`, record `issued` and `modified` are all `ctx.now()`.
//!
//! In `hali::core::Error`: `files_dir_unusable(path: Option<&str>, reason: &str) -> Error`,
//! rule `FILES_DIR_UNUSABLE`, exit code 3, `details {"path": <string|null>, "reason": reason}`.

#[cfg(feature = "goal")]
mod goal {
    use hali::core::resource::{self, NewFile};
    use hali::core::{Context, Error, FixedClock, SeededIdGen};
    use hali::storage::{File, Store};
    use std::cell::RefCell;
    use std::collections::BTreeSet;

    const R1: &str = "018bcfe5-6801-710a-adec-89025cc1beeb";
    const C1: &str = "018bcfe5-6802-7893-a2ee-fb32555e71c1";
    const P: &str = "018bcfe5-6803-71bb-94d8-d101b5b9c34d";
    const A: &str = "018bcfe5-6804-7099-ac6c-d7363ca585e7";

    const NOW: &str = "2026-10-24T09:30:00Z";
    const NOW_SECS: i64 = 1_792_834_200;
    const XSD: &str = "http://www.w3.org/2001/XMLSchema#";
    /// sha256 of `# Weekly review\n`.
    const SHA_WEEKLY: &str = "7a74edee5cc8f550445d1596e91acc3bf238a13123810cdfd8387dcfe34cbd0c";
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
        (0..10).map(|_| c.next_id()).collect()
    }

    fn empty_store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        (hali::storage::load(dir.path()).unwrap(), dir)
    }

    fn fixture_store(name: &str) -> Store {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        hali::storage::load(&dir).unwrap()
    }

    fn nf(title: &str) -> NewFile {
        NewFile {
            title: title.to_string(),
            types: Vec::new(),
            tags: Vec::new(),
            dir_real: "/d".to_string(),
            user_name: "nate".to_string(),
        }
    }

    fn triples(store: &Store) -> BTreeSet<String> {
        oxttl::TurtleParser::new()
            .for_slice(hali::storage::to_turtle(store, File::Resources).as_bytes())
            .map(|t| format!("{} .", t.expect("resources.ttl is Turtle")))
            .collect()
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

    /// The triples of a first `new` of the file `url` (a `file://` URL) with the given title,
    /// size and checksum, into an empty store: what `add` writes, plus `created` and the
    /// Resource's attribution. `agent` is the SoftwareAgent's (id, name) when one acts.
    fn expected(
        url: &str,
        title: &str,
        size: usize,
        sha: &str,
        agent: Option<(&str, &str)>,
    ) -> BTreeSet<String> {
        let r = urn(R1);
        let c = urn(C1);
        let sum = format!("{r}#sha256");
        let who = urn(agent.map_or(P, |(id, _)| id));
        let mut set = BTreeSet::from([
            nt(&r, RDF_TYPE, "<https://w3id.org/hali#Resource>"),
            nt(
                &r,
                "http://purl.org/dc/terms/title",
                &format!("\"{title}\""),
            ),
            nt(
                &r,
                "http://www.w3.org/ns/dcat#downloadURL",
                &format!("<{url}>"),
            ),
            nt(
                &r,
                "http://www.w3.org/ns/dcat#mediaType",
                "<http://www.iana.org/assignments/media-types/text/markdown>",
            ),
            nt(
                &r,
                "http://www.w3.org/ns/dcat#byteSize",
                &format!("\"{size}\"^^<{XSD}integer>"),
            ),
            nt(&r, "http://purl.org/dc/terms/modified", &dt(NOW)),
            nt(&r, "http://purl.org/dc/terms/created", &dt(NOW)),
            nt(&r, &format!("{PROV}wasAttributedTo"), &format!("<{who}>")),
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
                &format!("\"{sha}\"^^<{XSD}hexBinary>"),
            ),
            nt(&c, RDF_TYPE, "<http://www.w3.org/ns/dcat#CatalogRecord>"),
            nt(
                &c,
                "http://xmlns.com/foaf/0.1/primaryTopic",
                &format!("<{r}>"),
            ),
            nt(&c, "http://purl.org/dc/terms/issued", &dt(NOW)),
            nt(&c, "http://purl.org/dc/terms/modified", &dt(NOW)),
            nt(&c, &format!("{PROV}wasAttributedTo"), &format!("<{who}>")),
            nt(&urn(P), RDF_TYPE, &format!("<{PROV}Person>")),
            nt(&urn(P), NAME, "\"nate\""),
        ]);
        if let Some((id, name)) = agent {
            set.insert(nt(&urn(id), RDF_TYPE, &format!("<{PROV}SoftwareAgent>")));
            set.insert(nt(&urn(id), NAME, &format!("\"{name}\"")));
            set.insert(nt(
                &urn(id),
                &format!("{PROV}actedOnBehalfOf"),
                &format!("<{}>", urn(P)),
            ));
        }
        set
    }

    fn name_of(path: &str) -> &str {
        path.strip_prefix("/d/").expect("inside the directory")
    }

    /// The file name `new` picks for `title` in `/d` when nothing is taken.
    fn picked(title: &str) -> String {
        let (mut store, _dir) = empty_store();
        let out = resource::new_resource(&ctx(1, None), &mut store, &nf(title), |_| false)
            .unwrap_or_else(|e| panic!("{title:?}: {}", e.message));
        name_of(&out.value.path).to_string()
    }

    // ---- row 1 and 5: what a first `new` writes -----------------------------------------------------

    #[test]
    fn creates_markdown_record() {
        let (mut store, _dir) = empty_store();
        let out =
            resource::new_resource(&ctx(1, None), &mut store, &nf("Weekly review"), |_| false)
                .unwrap();
        let created = &out.value;
        assert_eq!(created.path, "/d/weekly-review.md");
        assert_eq!(created.content, "# Weekly review\n");
        assert_eq!(created.mtime, NOW_SECS);
        let r = &created.record;
        assert_eq!(r.id, R1);
        assert_eq!(r.path, "/d/weekly-review.md");
        assert_eq!(r.title, "Weekly review");
        assert_eq!(r.media_type, "text/markdown");
        assert_eq!(r.byte_size, 16);
        assert_eq!(r.modified, NOW);
        assert!(r.editable);
        assert!(!r.missing);
        assert!(!r.stale);
        assert!(r.types.is_empty() && r.tags.is_empty());
    }

    #[test]
    fn triples_exact() {
        let url = "file:///d/weekly-review.md";
        // The user is the Agent.
        let (mut store, _dir) = empty_store();
        let out =
            resource::new_resource(&ctx(1, None), &mut store, &nf("Weekly review"), |_| false)
                .unwrap();
        let want = expected(url, "Weekly review", 16, SHA_WEEKLY, None);
        assert_eq!(triples(&store), want);
        // The changes are exactly those triples, all additions.
        let changed: BTreeSet<String> = out.changes.iter().map(|c| c.triple.clone()).collect();
        assert_eq!(changed, want);
        assert_eq!(out.changes.len(), want.len());
        assert!(out.changes.iter().all(|c| c.op == hali::core::Op::Add));

        // An acting agent: the Resource and the record are both attributed to it; ids are
        // minted Resource, record, Person, SoftwareAgent.
        let (mut store, _dir) = empty_store();
        let out = resource::new_resource(
            &ctx(1, Some("claude")),
            &mut store,
            &nf("Weekly review"),
            |_| false,
        )
        .unwrap();
        assert_eq!(out.value.record.id, R1);
        let want = expected(url, "Weekly review", 16, SHA_WEEKLY, Some((A, "claude")));
        assert_eq!(triples(&store), want);
        assert_eq!(out.changes.len(), want.len());
    }

    #[test]
    fn second_new_reuses_person_and_agent() {
        let (mut store, _dir) = empty_store();
        let c = ctx(1, Some("claude"));
        resource::new_resource(&c, &mut store, &nf("One"), |_| false).unwrap();
        let second = resource::new_resource(&c, &mut store, &nf("Two"), |_| false).unwrap();
        let all = ids(1);
        // R1 C1 P A, then the second Resource and record: no Person, no Agent.
        assert_eq!(second.value.record.id, all[4]);
        let t = triples(&store);
        let count = |class: &str| {
            t.iter()
                .filter(|l| l.contains(&format!("<{RDF_TYPE}> <{PROV}{class}> .")))
                .count()
        };
        assert_eq!(count("Person"), 1);
        assert_eq!(count("SoftwareAgent"), 1);
        assert!(t.contains(&nt(
            &urn(&all[4]),
            &format!("{PROV}wasAttributedTo"),
            &format!("<{}>", urn(A))
        )));
        assert!(t.contains(&nt(
            &urn(&all[5]),
            &format!("{PROV}wasAttributedTo"),
            &format!("<{}>", urn(A))
        )));
    }

    #[test]
    fn types_and_tags_are_resolved() {
        let mut store = fixture_store("store");
        let mut new = nf("Typed");
        new.types = vec!["Paper".to_string()];
        new.tags = vec!["ml".to_string()];
        let out = resource::new_resource(&ctx(1, None), &mut store, &new, |_| false).unwrap();
        let r = &out.value.record;
        assert_eq!(r.types, ["Paper"]);
        assert_eq!(r.tags.len(), 1);
        assert_eq!(
            (r.tags[0].id.as_str(), r.tags[0].label.as_str()),
            ("ml", "machine learning")
        );
        let changed: Vec<&str> = out.changes.iter().map(|c| c.triple.as_str()).collect();
        assert!(
            changed.contains(&nt(&urn(R1), RDF_TYPE, "<https://w3id.org/hali#Paper>").as_str())
        );
        assert!(
            changed.contains(
                &nt(
                    &urn(R1),
                    "http://purl.org/dc/terms/subject",
                    "<https://w3id.org/hali#ml>"
                )
                .as_str()
            )
        );
    }

    // ---- row 3: the slug ------------------------------------------------------------------------------

    #[test]
    fn slug_rules() {
        let cases: Vec<(String, String)> = vec![
            ("Weekly review".into(), "weekly-review.md".into()),
            ("  Hello,   World!! ".into(), "hello-world.md".into()),
            ("UPPER Case".into(), "upper-case.md".into()),
            // Unicode letters and digits are kept and lowercased; nothing is folded to ASCII.
            ("\u{c9}t\u{e9} 2026".into(), "\u{e9}t\u{e9}-2026.md".into()),
            (
                "\u{65e5}\u{672c}\u{8a9e} notes".into(),
                "\u{65e5}\u{672c}\u{8a9e}-notes.md".into(),
            ),
            ("Stra\u{df}e".into(), "stra\u{df}e.md".into()),
            // Traversal and separators.
            ("../../etc/x".into(), "etc-x.md".into()),
            ("a/b\\c".into(), "a-b-c.md".into()),
            ("..".into(), "untitled.md".into()),
            (".hidden".into(), "hidden.md".into()),
            ("#x".into(), "x.md".into()),
            // Nothing left of the title.
            ("---".into(), "untitled.md".into()),
            ("!!!".into(), "untitled.md".into()),
            ("\u{1f600}".into(), "untitled.md".into()),
            // The 128-byte cap, at a character boundary, with no trailing `-`.
            ("a".repeat(200), format!("{}.md", "a".repeat(128))),
            ("a".repeat(128), format!("{}.md", "a".repeat(128))),
            (
                format!("{}\u{e9}", "a".repeat(127)),
                format!("{}.md", "a".repeat(127)),
            ),
            (
                format!("{} b", "a".repeat(127)),
                format!("{}.md", "a".repeat(127)),
            ),
            (
                format!("{}\u{e9}", "a".repeat(126)),
                format!("{}\u{e9}.md", "a".repeat(126)),
            ),
            (
                "\u{65e5}".repeat(50),
                format!("{}.md", "\u{65e5}".repeat(42)),
            ),
        ];
        for (title, want) in cases {
            assert_eq!(picked(&title), want, "title {title:?}");
        }
    }

    #[test]
    fn the_title_is_normalised_but_not_escaped() {
        let (mut store, _dir) = empty_store();
        let out =
            resource::new_resource(&ctx(1, None), &mut store, &nf("  #x  "), |_| false).unwrap();
        assert_eq!(out.value.record.title, "#x");
        assert_eq!(out.value.content, "# #x\n");
        assert_eq!(out.value.record.byte_size, 5);
        assert_eq!(out.value.path, "/d/x.md");
    }

    #[test]
    fn bad_titles_are_usage_errors_and_change_nothing() {
        for bad in ["", "   ", "a\nb", "tab\there", "bell\u{7}"] {
            let (mut store, _dir) = empty_store();
            let err = resource::new_resource(&ctx(1, None), &mut store, &nf(bad), |_| false)
                .map(|_| ())
                .unwrap_err();
            assert_eq!(err.rule, "USAGE", "{bad:?}");
            assert_eq!(err.exit_code(), 2, "{bad:?}");
            assert!(triples(&store).is_empty(), "{bad:?}");
        }
    }

    // ---- row 4: taken names ---------------------------------------------------------------------------

    #[test]
    fn taken_names_skip() {
        // The caller's lookup (lstat of anything there) is asked with full candidate paths, in
        // order.
        let (mut store, _dir) = empty_store();
        let asked = RefCell::new(Vec::new());
        let out = resource::new_resource(&ctx(1, None), &mut store, &nf("Same"), |p| {
            asked.borrow_mut().push(p.to_string());
            p == "/d/same.md" || p == "/d/same-2.md"
        })
        .unwrap();
        assert_eq!(out.value.path, "/d/same-3.md");
        assert_eq!(
            *asked.borrow(),
            ["/d/same.md", "/d/same-2.md", "/d/same-3.md"]
        );
        assert_eq!(out.value.content, "# Same\n");

        // A path any Resource records is taken, though nothing is on disk (a Missing one).
        let (mut store, _dir) = empty_store();
        let first =
            resource::new_resource(&ctx(1, None), &mut store, &nf("Same"), |_| false).unwrap();
        assert_eq!(first.value.path, "/d/same.md");
        let second =
            resource::new_resource(&ctx(2, None), &mut store, &nf("Same"), |_| false).unwrap();
        assert_eq!(second.value.path, "/d/same-2.md");
        let third = resource::new_resource(&ctx(3, None), &mut store, &nf("Same"), |p| {
            p == "/d/same-3.md"
        })
        .unwrap();
        assert_eq!(third.value.path, "/d/same-4.md");

        // The cap applies to the slug; the suffix comes after it.
        let (mut store, _dir) = empty_store();
        let long = "a".repeat(200);
        let want_first = format!("/d/{}.md", "a".repeat(128));
        let out =
            resource::new_resource(&ctx(1, None), &mut store, &nf(&long), |p| p == want_first)
                .unwrap();
        assert_eq!(out.value.path, format!("/d/{}-2.md", "a".repeat(128)));
    }

    // ---- row 7: errors change nothing ---------------------------------------------------------------------

    #[test]
    fn unknown_refs_change_nothing() {
        let mut store = fixture_store("store");
        let before = triples(&store);
        let mut new = nf("Nope");
        new.types = vec!["Papr".to_string()];
        let err = resource::new_resource(&ctx(1, None), &mut store, &new, |_| false)
            .map(|_| ())
            .unwrap_err();
        assert_eq!(err.rule, "UNKNOWN_TYPE");
        assert_eq!(err.exit_code(), 1);

        let mut new = nf("Nope");
        new.types = vec!["Paper".to_string()];
        new.tags = vec!["machin".to_string()];
        let err = resource::new_resource(&ctx(1, None), &mut store, &new, |_| false)
            .map(|_| ())
            .unwrap_err();
        assert_eq!(err.rule, "UNKNOWN_CONCEPT");
        assert_eq!(err.exit_code(), 1);
        assert_eq!(triples(&store), before);
    }

    // ---- row 11: SHACL ----------------------------------------------------------------------------------------

    #[test]
    fn shacl_failure_changes_nothing() {
        let mut store = fixture_store("shacl/new-resource-shape");
        let before = triples(&store);
        let err = resource::new_resource(&ctx(1, None), &mut store, &nf("x"), |_| false)
            .map(|_| ())
            .unwrap_err();
        assert_eq!(err.rule, "SHACL_VIOLATION");
        assert_eq!(err.exit_code(), 1);
        assert_eq!(triples(&store), before);
    }

    // ---- row 12: the error ----------------------------------------------------------------------------------------

    #[test]
    fn files_dir_unusable_error() {
        for reason in [
            "unresolved",
            "relative",
            "not_a_directory",
            "own_dir",
            "cannot_create",
        ] {
            let path = (reason != "unresolved").then_some("/some/dir");
            let e: Error = Error::files_dir_unusable(path, reason);
            assert_eq!(e.rule, "FILES_DIR_UNUSABLE", "{reason}");
            assert_eq!(e.exit_code(), 3, "{reason}");
            assert_eq!(e.details["reason"], reason);
            assert_eq!(
                e.details["path"],
                path.map_or(serde_json::Value::Null, |p| serde_json::json!(p))
            );
            assert!(!e.message.is_empty());
            assert!(e.suggested_fix.is_some(), "{reason}");
        }
    }

    // ---- row 21 and 23: the slug is only letters, digits and `-` ----------------------------------------------

    fn stem(title: &str) -> String {
        let name = picked(title);
        name.strip_suffix(".md")
            .unwrap_or_else(|| panic!("{title:?}: {name}"))
            .to_string()
    }

    #[test]
    fn the_slug_has_only_alphanumerics_and_hyphens() {
        let many_dotted = "\u{130}".repeat(100);
        let titles = [
            "\u{130}STANBUL",
            "\u{130}",
            "\u{130}\u{130}\u{130}",
            "I\u{130} x",
            "x \u{130}",
            "e\u{301}",
            "Re\u{301}sume\u{301}",
            "\u{301}x\u{301}",
            "a\u{307}b",
            "\u{1e9e}",
            many_dotted.as_str(),
        ];
        for title in titles {
            let s = stem(title);
            assert!(!s.is_empty(), "{title:?}");
            assert!(
                s.chars().all(|c| c.is_alphanumeric() || c == '-'),
                "{title:?} gives {s:?}"
            );
            assert!(!s.starts_with('-') && !s.ends_with('-'), "{title:?}: {s:?}");
            assert!(s.len() <= 128, "{title:?}: {} bytes", s.len());
        }
        // The dotted capital I keeps its letter and loses the combining dot.
        assert!(stem("\u{130}STANBUL").starts_with('i'));
        assert!(stem("\u{130}STANBUL").contains("stanbul"));
    }

    #[test]
    fn the_combining_dot_is_a_separator_with_exact_slugs() {
        // Lowercasing U+0130 gives `i` and U+0307; the mark is a separator, never dropped.
        assert_eq!(stem("\u{130}STANBUL"), "i-stanbul");
        assert_eq!(stem("x.\u{130}.x"), "x-i-x");
        assert_eq!(stem("a  b"), "a-b");
        for title in ["\u{130}STANBUL", "x.\u{130}.x", "a  b", "I\u{130} x"] {
            assert!(!stem(title).contains("--"), "{title:?}: {}", stem(title));
        }
    }

    #[test]
    fn a_title_of_only_combining_marks_is_untitled() {
        assert_eq!(picked("\u{301}"), "untitled.md");
        assert_eq!(picked("\u{301}\u{307} \u{301}"), "untitled.md");
        assert_eq!(picked("\u{301}-\u{307}"), "untitled.md");
    }

    #[test]
    fn nfd_titles_drop_the_combining_accent_and_split_the_word() {
        // Pinned: a combining mark is not alphanumeric, so it is a separator like punctuation
        // (the accent is dropped). NFC input keeps its precomposed letter; nothing is normalised.
        assert_eq!(picked("Re\u{301}sume\u{301}"), "re-sume.md");
        assert_eq!(picked("R\u{e9}sum\u{e9}"), "r\u{e9}sum\u{e9}.md");
        assert_eq!(picked("e\u{301}"), "e.md");
    }

    #[test]
    fn sharp_s_and_final_sigma_are_pinned() {
        // `char::to_lowercase` per character, no case folding, no context: the sharp s stays,
        // the capital sharp s lowercases to it, and a final sigma becomes the medial one.
        assert_eq!(picked("Stra\u{df}e"), "stra\u{df}e.md");
        assert_eq!(picked("\u{1e9e}"), "\u{df}.md");
        assert_eq!(picked("STRASSE"), "strasse.md");
        assert_eq!(
            picked("\u{39f}\u{394}\u{39f}\u{3a3}"),
            "\u{3bf}\u{3b4}\u{3bf}\u{3c3}.md"
        );
    }
}
