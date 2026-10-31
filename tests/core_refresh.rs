//! Goal tests for `refresh` in `core` (#20, spec §6, §9.1 **Refresh**, §9.5).
//!
//! Assumed API (invented here; the implementer follows it). Module `hali::core::resource`:
//!   `enum Target { All, Ids(Vec<String>) }`
//!       `All`: every cataloged Resource in id order. `Ids`: bare UUIDs, already resolved by the
//!       caller, in argument order; one that is no cataloged Resource is `UNKNOWN_RESOURCE`
//!       (details `ref` = the id given). A Resource named twice counts once (its first place).
//!   `struct Hashed { byte_size: u64, modified: String, sha256: String }`
//!       what `hash` read: the size, the mtime as `YYYY-MM-DDTHH:MM:SSZ` (as `FileFacts`) and
//!       the lowercase hex SHA-256. `Clone + Debug + PartialEq`.
//!   `enum Seen { Missing, Hashed(Hashed) }`  `Clone + Debug + PartialEq`.
//!   `struct Refreshed { refreshed: Vec<Record>, missing: Vec<Record>, unchanged: usize }`
//!       `Clone + Debug + PartialEq + Serialize`, serialised in that key order. The records are
//!       as `forget` returns them (`missing` and `stale` left false; the CLI observes them),
//!       after the change.
//!   `refresh(&Context, &mut Store, &Target, stat, hash) -> Result<Mutation<Refreshed>, Error>`
//!       `stat: impl Fn(&str) -> Option<(u64, i64)>` (size, mtime in whole seconds since the
//!       epoch, of the regular file at the recorded path, as `storage::stat`) and
//!       `hash: impl FnMut(&str) -> Result<Seen, Error>` (the path is the recorded real path,
//!       as `Record::path`). `core` reads no file: it only calls these.
//!       Every Resource is resolved first (an unknown id is an error before `stat` or `hash`
//!       is called). Then, per Resource in order: `stat` once; `None` is Missing and `hash` is
//!       never called for it. For `All`, a Resource is hashed only if it is Stale (size, or
//!       `rfc3339(mtime)`, differs from the record); for `Ids`, always. A `hash` that answers
//!       `Seen::Missing` (the file went after the stat) is Missing; its `Err` aborts the whole
//!       refresh with that error. A Resource whose triples already match what was hashed is
//!       unchanged (hashed or not). The changes of all Resources form one mutation: per
//!       changed Resource the `dcat:byteSize`, `dcterms:modified` and the checksum node's
//!       `spdx:checksumValue` that differ are replaced, any `spdx:checksum` link of the
//!       Resource other than the one to `<id>#sha256` is replaced by that link (the other node
//!       is not touched), and the catalog record's `dcterms:modified` becomes `ctx.now()`. One
//!       SHACL guard, one `apply`; on any error the store is untouched. No clock-free path
//!       mints an id.
//!
//! The tests read tests/fixtures/refresh: the user Person; five Resources R1-R5 (ids ...0901 to
//! ...0905, catalog records C1-C5 ...0911 to ...0915), each recorded as six bytes, sha256 of
//! "hello\n" and `modified` 2026-10-20T14:02:11Z (epoch second 1792504931), the catalog records
//! 2026-10-21T08:00:00Z. The injected `stat` and `hash` fakes stand for the files:
//!   same.txt    6 bytes, 1792504931              matches the record
//!   edited.txt  12 bytes, 1792663200 (2026-10-22T10:00:00Z), sha256 of "hello world\n"
//!   touched.txt 6 bytes, 1792663200, sha256 of "hello\n"      (mtime only)
//!   gone.txt    no regular file
//!   sneaky.txt  6 bytes, 1792504931, sha256 of "HELLO\n"      (same size and mtime: not Stale)

#[cfg(feature = "goal")]
mod goal {
    use hali::core::resource::{self, Hashed, Refreshed, Seen, Target};
    use hali::core::{Context, Error, FixedClock, Mutation, Op, SeededIdGen};
    use hali::storage::{File, Store};
    use std::cell::RefCell;
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    const R1: &str = "0192f0c4-0000-7000-8000-000000000901";
    const R2: &str = "0192f0c4-0000-7000-8000-000000000902";
    const R3: &str = "0192f0c4-0000-7000-8000-000000000903";
    const R4: &str = "0192f0c4-0000-7000-8000-000000000904";
    const R5: &str = "0192f0c4-0000-7000-8000-000000000905";
    const C1: &str = "0192f0c4-0000-7000-8000-000000000911";
    const C2: &str = "0192f0c4-0000-7000-8000-000000000912";
    const C3: &str = "0192f0c4-0000-7000-8000-000000000913";
    const C5: &str = "0192f0c4-0000-7000-8000-000000000915";
    const GONE: &str = "0192f0c4-0000-7000-8000-000000000999";
    const XSD: &str = "http://www.w3.org/2001/XMLSchema#";
    const NOW: &str = "2026-10-24T09:30:00Z";
    const OLD: &str = "2026-10-20T14:02:11Z";
    const OLD_RECORD: &str = "2026-10-21T08:00:00Z";
    const NEW: &str = "2026-10-22T10:00:00Z";
    const SHA_HELLO: &str = "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03";
    const SHA_WORLD: &str = "a948904f2f0f479b8f8197694b30184b0d2ed1c1cd2a1ec0fb85d299a192a447";
    const SHA_UPPER: &str = "3b09aeb6f5f5336beb205d7f720371bc927cd46c21922e334d47ba264acb5ba4";
    const SAME: &str = "/fixtures/refresh/same.txt";
    const EDITED: &str = "/fixtures/refresh/edited.txt";
    const TOUCHED: &str = "/fixtures/refresh/touched.txt";
    const GONE_PATH: &str = "/fixtures/refresh/gone.txt";
    const SNEAKY: &str = "/fixtures/refresh/sneaky.txt";

    type Calls = (Vec<String>, Vec<String>);

    /// One Context per seeded id sequence (refresh mints no id; the seed is never consumed).
    fn ctx() -> Context {
        Context::new(
            "en",
            Box::new(FixedClock::new(NOW)),
            Box::new(SeededIdGen::new(1)),
        )
    }

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    fn load() -> Store {
        hali::storage::load(&fixture("refresh")).unwrap()
    }

    fn resources_ttl(store: &Store) -> String {
        hali::storage::to_turtle(store, File::Resources)
    }

    fn triples(turtle: &str) -> BTreeSet<String> {
        oxttl::TurtleParser::new()
            .for_slice(turtle.as_bytes())
            .map(|t| format!("{} .", t.unwrap()))
            .collect()
    }

    fn stat_of(path: &str) -> Option<(u64, i64)> {
        match path {
            SAME | SNEAKY => Some((6, 1_792_504_931)),
            EDITED => Some((12, 1_792_663_200)),
            TOUCHED => Some((6, 1_792_663_200)),
            _ => None,
        }
    }

    fn hashed(byte_size: u64, modified: &str, sha256: &str) -> Seen {
        Seen::Hashed(Hashed {
            byte_size,
            modified: modified.to_string(),
            sha256: sha256.to_string(),
        })
    }

    fn hash_of(path: &str) -> Result<Seen, Error> {
        Ok(match path {
            SAME => hashed(6, OLD, SHA_HELLO),
            EDITED => hashed(12, NEW, SHA_WORLD),
            TOUCHED => hashed(6, NEW, SHA_HELLO),
            SNEAKY => hashed(6, OLD, SHA_UPPER),
            _ => Seen::Missing,
        })
    }

    /// Runs `refresh` with logging fakes; returns the result and the paths `stat` and `hash`
    /// were called with, in call order.
    fn go(
        store: &mut Store,
        target: &Target,
        stat: fn(&str) -> Option<(u64, i64)>,
        hash: fn(&str) -> Result<Seen, Error>,
    ) -> (Result<Mutation<Refreshed>, Error>, Calls) {
        let stats = RefCell::new(Vec::new());
        let hashes = RefCell::new(Vec::new());
        let result = resource::refresh(
            &ctx(),
            store,
            target,
            |p: &str| {
                stats.borrow_mut().push(p.to_string());
                stat(p)
            },
            |p: &str| {
                hashes.borrow_mut().push(p.to_string());
                hash(p)
            },
        );
        (result, (stats.into_inner(), hashes.into_inner()))
    }

    fn run(store: &mut Store, target: &Target) -> (Mutation<Refreshed>, Calls) {
        let (result, calls) = go(store, target, stat_of, hash_of);
        (result.unwrap(), calls)
    }

    fn ids(list: &[&str]) -> Target {
        Target::Ids(list.iter().map(|s| s.to_string()).collect())
    }

    fn dt(value: &str) -> String {
        format!("\"{value}\"^^<{XSD}dateTime>")
    }

    fn modified(id: &str, value: &str) -> String {
        format!(
            "<urn:uuid:{id}> <http://purl.org/dc/terms/modified> {} .",
            dt(value)
        )
    }

    fn size(id: &str, n: u64) -> String {
        format!("<urn:uuid:{id}> <http://www.w3.org/ns/dcat#byteSize> \"{n}\"^^<{XSD}integer> .")
    }

    fn sum(id: &str, hex: &str) -> String {
        format!(
            "<urn:uuid:{id}#sha256> <http://spdx.org/rdf/terms#checksumValue> \"{hex}\"^^<{XSD}hexBinary> ."
        )
    }

    /// The (op, triple) pairs of the changes.
    fn pairs(m: &Mutation<Refreshed>) -> Vec<(Op, String)> {
        m.changes.iter().map(|c| (c.op, c.triple.clone())).collect()
    }

    /// Expected changes: the adds then the removes, each sorted by the triple's plain byte
    /// order (the order of `Change`).
    fn expected(mut adds: Vec<String>, mut removes: Vec<String>) -> Vec<(Op, String)> {
        adds.sort();
        removes.sort();
        adds.into_iter()
            .map(|t| (Op::Add, t))
            .chain(removes.into_iter().map(|t| (Op::Remove, t)))
            .collect()
    }

    /// The store after is the store before, minus the removes, plus the adds.
    fn assert_applied(before: &BTreeSet<String>, store: &Store, m: &Mutation<Refreshed>) {
        let mut want = before.clone();
        for c in &m.changes {
            match c.op {
                Op::Add => want.insert(c.triple.clone()),
                Op::Remove => want.remove(&c.triple),
            };
        }
        assert_eq!(triples(&resources_ttl(store)), want);
    }

    fn assert_sorted(m: &Mutation<Refreshed>) {
        assert!(
            m.changes.windows(2).all(|w| w[0] < w[1]),
            "changes sorted, without duplicates: {:?}",
            m.changes
        );
    }

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    // ---- criterion 2: changed content ---------------------------------------------------------

    #[test]
    fn refreshes_changed_content() {
        let mut store = load();
        let before = triples(&resources_ttl(&store));
        let (m, (stats, hashes)) = run(&mut store, &ids(&[R2]));

        let r = &m.value;
        assert_eq!(r.refreshed.len(), 1);
        assert_eq!(r.refreshed[0].id, R2);
        assert_eq!(r.refreshed[0].path, EDITED);
        assert_eq!(r.refreshed[0].title, "edited");
        assert_eq!(r.refreshed[0].byte_size, 12);
        assert_eq!(r.refreshed[0].modified, NEW);
        assert!(r.missing.is_empty());
        assert_eq!(r.unchanged, 0);
        assert_eq!((stats, hashes), (strings(&[EDITED]), strings(&[EDITED])));

        // Exactly 4 adds and 4 removes: size, mtime and checksum replaced, the catalog
        // record's `modified` set to now. The algorithm triple stays.
        assert_eq!(
            pairs(&m),
            expected(
                vec![
                    size(R2, 12),
                    modified(R2, NEW),
                    sum(R2, SHA_WORLD),
                    modified(C2, NOW)
                ],
                vec![
                    size(R2, 6),
                    modified(R2, OLD),
                    sum(R2, SHA_HELLO),
                    modified(C2, OLD_RECORD)
                ],
            )
        );
        assert_sorted(&m);
        assert_applied(&before, &store, &m);
        // The other Resources and their records are as they were.
        let text = resources_ttl(&store);
        for kept in [R1, R3, R4, R5] {
            assert!(text.contains(kept), "{kept}");
        }
        assert!(!text.contains(SHA_UPPER));
    }

    // ---- criterion 3: mtime only ---------------------------------------------------------------

    #[test]
    fn touched_changes_only_modified() {
        let mut store = load();
        let before = triples(&resources_ttl(&store));
        let (m, (_, hashes)) = run(&mut store, &ids(&[R3]));
        assert_eq!(hashes, strings(&[TOUCHED]));
        assert_eq!(m.value.refreshed.len(), 1);
        assert_eq!(m.value.refreshed[0].id, R3);
        assert_eq!(m.value.refreshed[0].byte_size, 6);
        assert_eq!(m.value.refreshed[0].modified, NEW);
        assert_eq!(m.value.unchanged, 0);
        assert_eq!(
            pairs(&m),
            expected(
                vec![modified(R3, NEW), modified(C3, NOW)],
                vec![modified(R3, OLD), modified(C3, OLD_RECORD)],
            )
        );
        assert_applied(&before, &store, &m);
    }

    // ---- criterion 4: unchanged is a no-op -----------------------------------------------------

    #[test]
    fn unchanged_is_noop() {
        let mut store = load();
        let before = resources_ttl(&store);
        let (m, (stats, hashes)) = run(&mut store, &ids(&[R1]));
        assert_eq!(m.changes, []);
        assert!(m.value.refreshed.is_empty());
        assert!(m.value.missing.is_empty());
        assert_eq!(m.value.unchanged, 1);
        // A named Resource is hashed even though it is unchanged.
        assert_eq!((stats, hashes), (strings(&[SAME]), strings(&[SAME])));
        assert_eq!(resources_ttl(&store), before);
        // The catalog record is not touched when nothing changes.
        assert!(resources_ttl(&store).contains(&format!("<urn:uuid:{C1}>")));
        assert_eq!(
            serde_json::to_string(&m.value).unwrap(),
            r#"{"refreshed":[],"missing":[],"unchanged":1}"#
        );
    }

    // ---- criterion 5: which Resources are hashed -----------------------------------------------

    #[test]
    fn all_hashes_only_stale() {
        let mut store = load();
        let before = triples(&resources_ttl(&store));
        let (m, (stats, hashes)) = run(&mut store, &Target::All);

        // Every Resource is statted once, in id order; only the Stale ones are hashed.
        assert_eq!(
            stats,
            strings(&[SAME, EDITED, TOUCHED, GONE_PATH, SNEAKY]),
            "stat once each, in id order"
        );
        assert_eq!(hashes, strings(&[EDITED, TOUCHED]));

        let refreshed: Vec<&str> = m.value.refreshed.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(refreshed, [R2, R3]);
        let missing: Vec<&str> = m.value.missing.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(missing, [R4]);
        assert_eq!(m.value.unchanged, 2, "R1 and R5");
        assert_eq!(
            pairs(&m),
            expected(
                vec![
                    size(R2, 12),
                    modified(R2, NEW),
                    sum(R2, SHA_WORLD),
                    modified(C2, NOW),
                    modified(R3, NEW),
                    modified(C3, NOW),
                ],
                vec![
                    size(R2, 6),
                    modified(R2, OLD),
                    sum(R2, SHA_HELLO),
                    modified(C2, OLD_RECORD),
                    modified(R3, OLD),
                    modified(C3, OLD_RECORD),
                ],
            )
        );
        assert_sorted(&m);
        assert_applied(&before, &store, &m);
        assert!(
            !resources_ttl(&store).contains(SHA_UPPER),
            "R5 is not Stale, so --all never read it"
        );
    }

    #[test]
    fn named_ref_always_hashes() {
        let mut store = load();
        let before = triples(&resources_ttl(&store));
        // R5 is not Stale (same size and mtime) but named: it is hashed, and its different
        // content is recorded. R1 is named and unchanged. Argument order, not id order.
        let (m, (stats, hashes)) = run(&mut store, &ids(&[R5, R1]));
        assert_eq!(hashes, strings(&[SNEAKY, SAME]));
        assert_eq!(stats, strings(&[SNEAKY, SAME]));
        assert_eq!(m.value.refreshed.len(), 1);
        assert_eq!(m.value.refreshed[0].id, R5);
        assert_eq!(m.value.refreshed[0].byte_size, 6);
        assert_eq!(m.value.refreshed[0].modified, OLD);
        assert_eq!(m.value.unchanged, 1);
        assert_eq!(
            pairs(&m),
            expected(
                vec![sum(R5, SHA_UPPER), modified(C5, NOW)],
                vec![sum(R5, SHA_HELLO), modified(C5, OLD_RECORD)],
            )
        );
        assert_applied(&before, &store, &m);
    }

    #[test]
    fn named_twice_counts_once() {
        let mut store = load();
        let (m, (stats, hashes)) = run(&mut store, &ids(&[R2, R2, R1, R2]));
        assert_eq!(hashes, strings(&[EDITED, SAME]));
        assert_eq!(stats, strings(&[EDITED, SAME]));
        assert_eq!(m.value.refreshed.len(), 1);
        assert_eq!(m.value.unchanged, 1);
        assert_eq!(m.changes.len(), 8);
    }

    // ---- criterion 7: missing is never hashed --------------------------------------------------

    #[test]
    fn missing_never_hashed() {
        let mut store = load();
        let before = resources_ttl(&store);
        let (m, (stats, hashes)) = run(&mut store, &ids(&[R4]));
        assert_eq!(stats, strings(&[GONE_PATH]));
        assert_eq!(
            hashes,
            Vec::<String>::new(),
            "no regular file: never opened"
        );
        assert!(m.value.refreshed.is_empty());
        assert_eq!(m.value.missing.len(), 1);
        assert_eq!(m.value.missing[0].id, R4);
        assert_eq!(m.value.missing[0].byte_size, 6, "the record as it is");
        assert_eq!(m.value.unchanged, 0);
        assert_eq!(m.changes, []);
        assert_eq!(resources_ttl(&store), before);
    }

    #[test]
    fn vanished_after_stat_is_missing() {
        // `stat` saw a regular file; by the time `hash` looked it had gone.
        let mut store = load();
        let before = resources_ttl(&store);
        let (result, (_, hashes)) = go(&mut store, &ids(&[R2]), stat_of, |_| Ok(Seen::Missing));
        let m = result.unwrap();
        assert_eq!(hashes, strings(&[EDITED]));
        assert!(m.value.refreshed.is_empty());
        assert_eq!(m.value.missing.len(), 1);
        assert_eq!(m.value.missing[0].id, R2);
        assert_eq!(m.value.unchanged, 0);
        assert_eq!(m.changes, []);
        assert_eq!(resources_ttl(&store), before);
    }

    // ---- criterion 8: errors abort, nothing is written -----------------------------------------

    #[test]
    fn unknown_ref_before_hashing() {
        for target in [ids(&[R2, GONE]), ids(&[GONE, R2])] {
            let mut store = load();
            let before = resources_ttl(&store);
            let (result, (stats, hashes)) = go(&mut store, &target, stat_of, hash_of);
            let err = result.unwrap_err();
            assert_eq!(err.rule, "UNKNOWN_RESOURCE");
            assert_eq!(err.exit_code(), 1);
            assert_eq!(err.details["ref"], GONE);
            assert_eq!(
                stats,
                Vec::<String>::new(),
                "refs resolve before any file is read"
            );
            assert_eq!(hashes, Vec::<String>::new());
            assert_eq!(resources_ttl(&store), before);
        }
    }

    fn unreadable_touched(path: &str) -> Result<Seen, Error> {
        if path == TOUCHED {
            Err(Error::file_unreadable(path))
        } else {
            hash_of(path)
        }
    }

    #[test]
    fn hash_error_aborts_all() {
        for target in [ids(&[R2, R3]), Target::All] {
            let mut store = load();
            let before = resources_ttl(&store);
            let (result, (_, hashes)) = go(&mut store, &target, stat_of, unreadable_touched);
            let err = result.unwrap_err();
            assert_eq!(err.rule, "FILE_UNREADABLE");
            assert_eq!(err.exit_code(), 1);
            assert_eq!(err.details["path"], TOUCHED);
            // R2 was hashed first and fine; its change is not kept.
            assert_eq!(hashes, strings(&[EDITED, TOUCHED]));
            assert_eq!(resources_ttl(&store), before, "nothing written");
        }

        // The first error in processing order aborts: nothing after it is hashed.
        let mut store = load();
        let (result, (_, hashes)) = go(&mut store, &ids(&[R3, R2]), stat_of, unreadable_touched);
        assert_eq!(result.unwrap_err().rule, "FILE_UNREADABLE");
        assert_eq!(hashes, strings(&[TOUCHED]));

        // NOT_A_REGULAR_FILE passes through unchanged too.
        let mut store = load();
        let before = resources_ttl(&store);
        let (result, _) = go(&mut store, &ids(&[R2]), stat_of, |p| {
            Err(Error::not_a_regular_file(p, "other"))
        });
        let err = result.unwrap_err();
        assert_eq!(err.rule, "NOT_A_REGULAR_FILE");
        assert_eq!(err.details["kind"], "other");
        assert_eq!(resources_ttl(&store), before);
    }

    #[test]
    fn shacl_guard() {
        // An extension shape that lets a Resource be at most six bytes: the store conforms as
        // it is, and the refresh of R2 (twelve bytes) would not, so it is refused.
        let dir = tempfile::tempdir().unwrap();
        std::fs::copy(
            fixture("refresh").join("resources.ttl"),
            dir.path().join("resources.ttl"),
        )
        .unwrap();
        std::fs::write(
            dir.path().join("extensions.ttl"),
            "@prefix hali: <https://w3id.org/hali#> .\n\
             @prefix dcat: <http://www.w3.org/ns/dcat#> .\n\
             @prefix sh: <http://www.w3.org/ns/shacl#> .\n\
             @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .\n\
             @prefix ex: <http://example.org/shapes#> .\n\
             ex:Small a sh:NodeShape ;\n\
             \tsh:targetClass hali:Resource ;\n\
             \tsh:property ex:SmallSize .\n\
             ex:SmallSize sh:path dcat:byteSize ;\n\
             \tsh:maxInclusive 6 ;\n\
             \tsh:message \"Resources stay small\"@en .\n",
        )
        .unwrap();
        let mut store = hali::storage::load(dir.path()).unwrap();
        let before = resources_ttl(&store);

        // R3 (six bytes) refreshes fine under the shape.
        let (result, _) = go(&mut store, &ids(&[R3]), stat_of, hash_of);
        assert!(result.is_ok(), "{:?}", result.err());

        let mut store = hali::storage::load(dir.path()).unwrap();
        let (result, (_, hashes)) = go(&mut store, &ids(&[R2]), stat_of, hash_of);
        let err = result.unwrap_err();
        assert_eq!(err.rule, "SHACL_VIOLATION");
        assert_eq!(err.exit_code(), 1);
        assert_eq!(hashes, strings(&[EDITED]));
        assert_eq!(
            resources_ttl(&store),
            before,
            "a refused refresh changes nothing"
        );

        // All-or-nothing across Resources: R3's valid change is not kept either.
        let mut store = hali::storage::load(dir.path()).unwrap();
        let (result, _) = go(&mut store, &Target::All, stat_of, hash_of);
        assert_eq!(result.unwrap_err().rule, "SHACL_VIOLATION");
        assert_eq!(resources_ttl(&store), before);
    }

    // ---- criterion 14: a foreign checksum link ---------------------------------------------------

    #[test]
    fn foreign_checksum_link_replaced() {
        // R2 links its checksum to a node of someone else's (`urn:example:other`), not to its
        // own `<id>#sha256`. The refresh replaces the link, leaves the other node alone, and
        // records the new checksum on its own node.
        let text = std::fs::read_to_string(fixture("refresh").join("resources.ttl")).unwrap();
        let own = format!("spdx:checksum <urn:uuid:{R2}#sha256> ;");
        assert!(text.contains(&own));
        let text = text.replace(&own, "spdx:checksum <urn:example:other> ;")
            + "<urn:example:other> spdx:algorithm spdx:checksumAlgorithm_sha256 ;\n\
               \tspdx:checksumValue \"00000000000000000000000000000000000000000000000000000000000000ff\"^^xsd:hexBinary .\n";
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("resources.ttl"), text).unwrap();
        let mut store = hali::storage::load(dir.path()).unwrap();
        let before = triples(&resources_ttl(&store));

        let (m, _) = run(&mut store, &ids(&[R2]));
        assert_eq!(m.value.refreshed.len(), 1);
        let link =
            |to: &str| format!("<urn:uuid:{R2}> <http://spdx.org/rdf/terms#checksum> <{to}> .");
        assert_eq!(
            pairs(&m),
            expected(
                vec![
                    size(R2, 12),
                    modified(R2, NEW),
                    sum(R2, SHA_WORLD),
                    modified(C2, NOW),
                    link(&format!("urn:uuid:{R2}#sha256")),
                ],
                vec![
                    size(R2, 6),
                    modified(R2, OLD),
                    sum(R2, SHA_HELLO),
                    modified(C2, OLD_RECORD),
                    link("urn:example:other"),
                ],
            )
        );
        assert_applied(&before, &store, &m);
        // The foreign node is untouched.
        let after = triples(&resources_ttl(&store));
        for t in before
            .iter()
            .filter(|t| t.starts_with("<urn:example:other>"))
        {
            assert!(after.contains(t), "{t}");
        }
    }

    // ---- criterion 10: empty store; no ids ---------------------------------------------------------

    #[test]
    fn all_on_empty_store() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = hali::storage::load(dir.path()).unwrap();
        let (result, (stats, hashes)) = go(&mut store, &Target::All, stat_of, hash_of);
        let m = result.unwrap();
        assert_eq!(m.changes, []);
        assert_eq!(
            serde_json::to_string(&m.value).unwrap(),
            r#"{"refreshed":[],"missing":[],"unchanged":0}"#
        );
        assert_eq!((stats, hashes), (Vec::new(), Vec::new()));
    }

    #[test]
    fn mints_no_id() {
        let c = ctx();
        let mut store = load();
        resource::refresh(&c, &mut store, &Target::All, stat_of, hash_of).unwrap();
        assert_eq!(c.next_id(), ctx().next_id(), "refresh consumed an id");
    }

    #[test]
    fn serialises_in_contract_order() {
        let mut store = load();
        let (m, _) = run(&mut store, &Target::All);
        let json = serde_json::to_string(&m.value).unwrap();
        assert!(json.starts_with(r#"{"refreshed":[{"id":""#), "{json}");
        assert!(
            json.ends_with(r#"],"unchanged":2}"#) && json.contains(r#"],"missing":[{"id":""#),
            "{json}"
        );
    }
}
