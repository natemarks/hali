//! Goal tests for `relink` and move detection in `core` (#21, spec §6, §9.1 **Relink:**, §9.5).
//!
//! Assumed API (invented here; the implementer follows it).
//!
//! Module `hali::core::relink` (new):
//!   `struct Counts { relinked, ambiguous, unmatched: usize }`, `Serialize`, in that key order;
//!       always exact.
//!   `struct Relinked { resource: Record, from: String }`, `Serialize`, in that key order: the
//!       Resource after the change (its `path` is the new one) and the path it was recorded at.
//!   `struct Ambiguous { resource: Record, candidates: Vec<String> }`, `Serialize`: a Missing
//!       Resource that was not relinked, and the files (at most 10, in byte order of the path)
//!       that have its size and hash. Never empty.
//!   `struct Searched { counts: Counts, relinked: Vec<Relinked>, ambiguous: Vec<Ambiguous>,
//!       unmatched: Vec<Record>, truncated: bool }`, `Serialize`, in that key order. Each of the
//!       three lists is in id order and holds at most 100 entries; `truncated` is true when one
//!       was cut. The records are as `refresh` returns them (`missing` and `stale` left false:
//!       the CLI observes them).
//!   `search(&Context, &mut Store, roots: &[String], walk, stat, hash)
//!       -> Result<Mutation<Searched>, Error>`
//!       `roots`: real, absolute Scan roots; `walk: impl FnMut(&str) -> Vec<Walked>` (the entries
//!       of one root, called once per root in argument order); `stat: impl Fn(&str) ->
//!       Option<(u64, i64)>` (as `refresh`; `None` is Missing); `hash: impl FnMut(&str, u64) ->
//!       Option<Hashed>` (the real path and the cap: the file is read at most `cap + 1` bytes;
//!       `None` when it cannot be hashed within the cap: it vanished, cannot be read, is not a
//!       regular file or holds more than `cap` bytes). `core` reads no file.
//!   `struct Explicit { resource: Record, from: String, refreshed: bool }`, `Serialize`, in that
//!       key order: the Resource after the change, the path it was recorded at, and whether the
//!       relink also refreshed its size, mtime and hash (`--force` on different content).
//!   `explicit(&Context, &mut Store, id: &str, path: &str, hashed: &Hashed, force: bool)
//!       -> Result<Mutation<Explicit>, Error>`
//!       `id` is a bare UUID (`UNKNOWN_RESOURCE` if no Resource has it, details `ref`); `path` is
//!       the real path of the file and `hashed` what was read from it. Not Missing is not
//!       required: any Resource can be relinked.
//!
//! What `search` does:
//!   1. A Missing Resource is a cataloged one that `stat` finds no regular file for (`stat` is
//!      called once per Resource, on its recorded path). Ones recorded as empty (byte size 0)
//!      are left out of matching (spec: empty files are excluded).
//!   2. Each root is walked once. A file is a candidate unless its size is 0 or its path is
//!      the recorded path of a cataloged Resource; a path found under several roots counts once.
//!   3. Size first: a candidate is only hashed if some Missing Resource has its size, once per
//!      path however many Resources it could match, with the cap = that size. A candidate
//!      matches a Resource when `hash` gives the same byte size and SHA-256 as the record.
//!   4. One-to-one: a Resource is relinked only when exactly one candidate matches it and no
//!      other Missing Resource matches that candidate. A Resource with matches that are not
//!      one-to-one is ambiguous (its matching files are its candidates); one with no match is
//!      unmatched.
//!   5. A relink swaps `dcat:downloadURL`, sets `dcterms:modified` to the mtime `hash` read and
//!      the catalog record's `dcterms:modified` to `ctx.now()`; nothing else changes. All
//!      relinks are one mutation with one SHACL guard; on an error the store is untouched.
//!
//! `explicit` makes the same change for one Resource. If `hashed.sha256` differs from the
//! record it is `HASH_MISMATCH` (exit 1, details `resource`, `path`, `expected`, `actual`;
//! `suggested_command` `hali relink <id> <path> --force`), unless `force`, which also replaces
//! `dcat:byteSize` and the checksum value, and sets `refreshed`.
//!
//! Module `hali::core::resource` gains
//!   `check_moves(&Store, files: &[FileFacts], stat: impl Fn(&str) -> Option<(u64, i64)>)
//!       -> Result<(), Error>`
//!       the CLI calls it before `add_all` unless `--new`. For the first file, in argument order,
//!       whose path no Resource records, whose size is above 0 and that has the byte size and
//!       SHA-256 of a Missing Resource (`stat` finds no regular file), it is `POSSIBLE_MOVE`
//!       (exit 1, details `{"path", "resources": [ids sorted]}`).
//!
//! `hali::core::config::Config` gains `files_dir: Option<String>`, from `[files] dir`; a value
//! of the wrong type is `CONFIG_INVALID`.
//!
//! The tests read tests/fixtures/refresh (see tests/core_refresh.rs: R1-R5 with catalog records
//! C1-C5, all six bytes with the SHA-256 of "hello\n"; here only R4, gone.txt, is Missing unless
//! a test says otherwise) and stores they generate.

mod goal {
    use hali::core::relink::{self, Searched};
    use hali::core::resource::{self, FileFacts, Hashed};
    use hali::core::{Context, Error, FixedClock, Mutation, Op, SeededIdGen, config};
    use hali::storage::walk::Walked;
    use hali::storage::{File, Store};
    use std::cell::RefCell;
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};
    use std::time::SystemTime;

    const R1: &str = "0192f0c4-0000-7000-8000-000000000901";
    const R4: &str = "0192f0c4-0000-7000-8000-000000000904";
    const R5: &str = "0192f0c4-0000-7000-8000-000000000905";
    const C4: &str = "0192f0c4-0000-7000-8000-000000000914";
    const NOW: &str = "2026-10-24T09:30:00Z";
    const OLD: &str = "2026-10-20T14:02:11Z";
    const OLD_RECORD: &str = "2026-10-21T08:00:00Z";
    const NEW: &str = "2026-10-22T10:00:00Z";
    const XSD: &str = "http://www.w3.org/2001/XMLSchema#";
    const SHA_HELLO: &str = "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03";
    const SHA_WORLD: &str = "a948904f2f0f479b8f8197694b30184b0d2ed1c1cd2a1ec0fb85d299a192a447";
    const SHA_UPPER: &str = "3b09aeb6f5f5336beb205d7f720371bc927cd46c21922e334d47ba264acb5ba4";
    const SHA_EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    const SAME: &str = "/fixtures/refresh/same.txt";
    const GONE: &str = "/fixtures/refresh/gone.txt";
    const SNEAKY: &str = "/fixtures/refresh/sneaky.txt";
    const PINNED: i64 = 1_792_504_931;

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

    fn refresh_store() -> Store {
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

    fn sha(i: usize) -> String {
        format!("{i:064x}")
    }

    fn hashed(size: u64, sha256: &str) -> Hashed {
        Hashed {
            byte_size: size,
            modified: NEW.to_string(),
            sha256: sha256.to_string(),
        }
    }

    fn file(path: &str, size: u64) -> Walked {
        Walked::File {
            path: path.to_string(),
            byte_size: size,
            mtime: SystemTime::UNIX_EPOCH,
        }
    }

    // ---- generated stores -------------------------------------------------------------------

    struct Gen {
        n: u32,
        path: String,
        size: u64,
        sha: String,
    }

    fn rid(n: u32) -> String {
        format!("0192f0c4-0000-7000-8000-{:012}", 1000 + n)
    }

    fn cid(n: u32) -> String {
        format!("0192f0c4-0000-7000-8000-{:012}", 5000 + n)
    }

    fn resources_for(gens: &[Gen]) -> String {
        let mut out = String::from(
            "# generated by hali; edit with the CLI\n\
             @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .\n\
             @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .\n\
             @prefix dcterms: <http://purl.org/dc/terms/> .\n\
             @prefix dcat: <http://www.w3.org/ns/dcat#> .\n\
             @prefix prov: <http://www.w3.org/ns/prov#> .\n\
             @prefix foaf: <http://xmlns.com/foaf/0.1/> .\n\
             @prefix spdx: <http://spdx.org/rdf/terms#> .\n\
             @prefix hali: <https://w3id.org/hali#> .\n\n\
             <urn:uuid:0192f0c4-0000-7000-8000-000000000001> a prov:Person ;\n\
             \tfoaf:name \"nate\" .\n",
        );
        for g in gens {
            let (r, c) = (rid(g.n), cid(g.n));
            out.push_str(&format!(
                "<urn:uuid:{r}#sha256> spdx:algorithm spdx:checksumAlgorithm_sha256 ;\n\
                 \tspdx:checksumValue \"{sha}\"^^xsd:hexBinary .\n\
                 <urn:uuid:{r}> a hali:Resource ;\n\
                 \tdcterms:modified \"{OLD}\"^^xsd:dateTime ;\n\
                 \tdcterms:title \"r{n}\" ;\n\
                 \tspdx:checksum <urn:uuid:{r}#sha256> ;\n\
                 \tdcat:byteSize {size} ;\n\
                 \tdcat:downloadURL <file://{path}> ;\n\
                 \tdcat:mediaType <http://www.iana.org/assignments/media-types/text/plain> .\n\
                 <urn:uuid:{c}> a dcat:CatalogRecord ;\n\
                 \tdcterms:issued \"{OLD_RECORD}\"^^xsd:dateTime ;\n\
                 \tdcterms:modified \"{OLD_RECORD}\"^^xsd:dateTime ;\n\
                 \tprov:wasAttributedTo <urn:uuid:0192f0c4-0000-7000-8000-000000000001> ;\n\
                 \tfoaf:primaryTopic <urn:uuid:{r}> .\n",
                sha = g.sha,
                n = g.n,
                size = g.size,
                path = g.path,
            ));
        }
        out
    }

    /// A store of the generated Resources (and `extensions.ttl` if given).
    fn generated(gens: &[Gen], extensions: Option<&str>) -> Store {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("resources.ttl"), resources_for(gens)).unwrap();
        if let Some(text) = extensions {
            std::fs::write(dir.path().join("extensions.ttl"), text).unwrap();
        }
        hali::storage::load(dir.path()).unwrap()
    }

    fn make(n: u32, size: u64, sha: &str) -> Gen {
        Gen {
            n,
            path: format!("/gen/old/r{n}.txt"),
            size,
            sha: sha.to_string(),
        }
    }

    /// A shape that every Resource's `dcat:downloadURL` must be one of `urls`.
    fn urls_stay(urls: &[&str]) -> String {
        let list: Vec<String> = urls.iter().map(|u| format!("<{u}>")).collect();
        format!(
            "@prefix hali: <https://w3id.org/hali#> .\n\
             @prefix dcat: <http://www.w3.org/ns/dcat#> .\n\
             @prefix sh: <http://www.w3.org/ns/shacl#> .\n\
             @prefix ex: <http://example.org/shapes#> .\n\n\
             ex:UrlsStay a sh:NodeShape ;\n\
             \tsh:targetClass hali:Resource ;\n\
             \tsh:property ex:UrlRule .\n\
             ex:UrlRule sh:path dcat:downloadURL ;\n\
             \tsh:in ( {} ) ;\n\
             \tsh:message \"The file moved\"@en .\n",
            list.join(" ")
        )
    }

    // ---- running `search` with logging fakes ------------------------------------------------

    #[derive(Default)]
    struct Calls {
        walked: Vec<String>,
        stats: Vec<String>,
        hashed: Vec<(String, u64)>,
    }

    /// `tree`: for each root, the (path, size) of the files its walk yields.
    type Tree<'a> = Vec<(&'a str, Vec<(String, u64)>)>;

    fn search<S, H>(
        store: &mut Store,
        roots: &[&str],
        tree: &Tree<'_>,
        stat: S,
        hash: H,
    ) -> (Result<Mutation<Searched>, Error>, Calls)
    where
        S: Fn(&str) -> Option<(u64, i64)>,
        H: Fn(&str, u64) -> Option<Hashed>,
    {
        let calls = RefCell::new(Calls::default());
        let roots: Vec<String> = roots.iter().map(|r| r.to_string()).collect();
        let result = relink::search(
            &ctx(),
            store,
            &roots,
            |root: &str| {
                calls.borrow_mut().walked.push(root.to_string());
                tree.iter()
                    .filter(|(r, _)| *r == root)
                    .flat_map(|(_, files)| files.iter().map(|(p, s)| file(p, *s)))
                    .collect()
            },
            |path: &str| {
                calls.borrow_mut().stats.push(path.to_string());
                stat(path)
            },
            |path: &str, cap: u64| {
                calls.borrow_mut().hashed.push((path.to_string(), cap));
                hash(path, cap)
            },
        );
        (result, calls.into_inner())
    }

    /// `stat` for the refresh fixture where the files named in `missing` are gone.
    fn all_but(missing: &'static [&'static str]) -> impl Fn(&str) -> Option<(u64, i64)> {
        move |path: &str| (!missing.contains(&path)).then_some((6, PINNED))
    }

    /// Every file of six bytes hashes to "hello\n".
    fn hello(_: &str, _: u64) -> Option<Hashed> {
        Some(hashed(6, SHA_HELLO))
    }

    fn tree_of<'a>(root: &'a str, files: &[(&str, u64)]) -> Tree<'a> {
        vec![(
            root,
            files.iter().map(|(p, s)| (p.to_string(), *s)).collect(),
        )]
    }

    fn ids(list: &[hali::core::resource::Record]) -> Vec<&str> {
        list.iter().map(|r| r.id.as_str()).collect()
    }

    fn change_set(m: &Mutation<impl Sized>) -> BTreeSet<(Op, String)> {
        m.changes.iter().map(|c| (c.op, c.triple.clone())).collect()
    }

    fn quad(subject: &str, predicate: &str, object: &str) -> String {
        format!("<urn:uuid:{subject}> <{predicate}> {object} .")
    }

    fn datetime(value: &str) -> String {
        format!("\"{value}\"^^<{XSD}dateTime>")
    }

    const MODIFIED: &str = "http://purl.org/dc/terms/modified";
    const DOWNLOAD: &str = "http://www.w3.org/ns/dcat#downloadURL";
    const BYTE_SIZE: &str = "http://www.w3.org/ns/dcat#byteSize";

    /// The six changes of relinking R4 from `GONE` to `to`, the file having mtime `NEW`.
    fn relink_changes(to: &str) -> Vec<(Op, String)> {
        vec![
            (Op::Add, quad(R4, MODIFIED, &datetime(NEW))),
            (Op::Add, quad(R4, DOWNLOAD, &format!("<file://{to}>"))),
            (Op::Add, quad(C4, MODIFIED, &datetime(NOW))),
            (Op::Remove, quad(R4, MODIFIED, &datetime(OLD))),
            (Op::Remove, quad(R4, DOWNLOAD, &format!("<file://{GONE}>"))),
            (Op::Remove, quad(C4, MODIFIED, &datetime(OLD_RECORD))),
        ]
    }

    // ---- 1, 2: a unique match is relinked, and only that changes ---------------------------------

    #[test]
    fn unique_match_relinks() {
        let mut store = refresh_store();
        let tree = tree_of("/new", &[("/new/moved.txt", 6), ("/new/other.txt", 99)]);
        let (result, calls) = search(&mut store, &["/new"], &tree, all_but(&[GONE]), hello);
        let m = result.unwrap();
        let s = &m.value;
        assert_eq!(
            (s.counts.relinked, s.counts.ambiguous, s.counts.unmatched),
            (1, 0, 0)
        );
        assert_eq!(s.relinked.len(), 1);
        assert_eq!(s.relinked[0].from, GONE);
        assert_eq!(s.relinked[0].resource.id, R4);
        assert_eq!(s.relinked[0].resource.path, "/new/moved.txt");
        assert!(s.ambiguous.is_empty() && s.unmatched.is_empty() && !s.truncated);
        let ttl = resources_ttl(&store);
        assert!(ttl.contains("<file:///new/moved.txt>"), "{ttl}");
        assert!(!ttl.contains("gone.txt"), "{ttl}");
        assert_eq!(calls.walked, ["/new"]);
        // `stat` once per cataloged Resource, on its recorded path, wherever the roots are.
        let mut stats = calls.stats.clone();
        stats.sort();
        assert_eq!(stats.len(), 5, "{stats:?}");
        assert_eq!(stats.iter().collect::<BTreeSet<_>>().len(), 5, "{stats:?}");
        assert!(stats.iter().all(|p| p.starts_with("/fixtures/refresh/")));
        assert_eq!(calls.hashed, [("/new/moved.txt".to_string(), 6)]);
    }

    #[test]
    fn relink_writes_exactly_these_triples() {
        let mut store = refresh_store();
        let before = triples(&resources_ttl(&store));
        let tree = tree_of("/new", &[("/new/moved.txt", 6)]);
        let (result, _) = search(&mut store, &["/new"], &tree, all_but(&[GONE]), hello);
        let m = result.unwrap();
        // Adds before removes, then byte order; the swap, the new mtime and the record's bump.
        let got: Vec<(Op, String)> = m.changes.iter().map(|c| (c.op, c.triple.clone())).collect();
        assert_eq!(got, relink_changes("/new/moved.txt"));
        // Nothing else is touched: the title, media type, size, checksum and the rest stay.
        let mut want = before.clone();
        for (op, t) in relink_changes("/new/moved.txt") {
            match op {
                Op::Add => want.insert(t),
                Op::Remove => want.remove(&t),
            };
        }
        assert_eq!(triples(&resources_ttl(&store)), want);
    }

    // ---- 3, 4: several matches and one-to-one ------------------------------------------------

    #[test]
    fn several_candidates_are_ambiguous() {
        let mut store = refresh_store();
        let before = resources_ttl(&store);
        let tree = tree_of("/new", &[("/new/b.txt", 6), ("/new/a.txt", 6)]);
        let (result, calls) = search(&mut store, &["/new"], &tree, all_but(&[GONE]), hello);
        let m = result.unwrap();
        let s = &m.value;
        assert_eq!(
            (s.counts.relinked, s.counts.ambiguous, s.counts.unmatched),
            (0, 1, 0)
        );
        assert!(s.relinked.is_empty() && s.unmatched.is_empty());
        assert_eq!(s.ambiguous[0].resource.id, R4);
        assert_eq!(s.ambiguous[0].candidates, ["/new/a.txt", "/new/b.txt"]);
        assert!(m.changes.is_empty());
        assert_eq!(resources_ttl(&store), before, "nothing changes");
        assert_eq!(calls.hashed.len(), 2);
    }

    #[test]
    fn candidates_are_capped_at_ten() {
        let mut store = refresh_store();
        let files: Vec<(String, u64)> = (0..12).map(|i| (format!("/new/f{i:02}.txt"), 6)).collect();
        let tree: Tree = vec![("/new", files)];
        let (result, _) = search(&mut store, &["/new"], &tree, all_but(&[GONE]), hello);
        let s = result.unwrap().value;
        assert_eq!(s.counts.ambiguous, 1);
        let want: Vec<String> = (0..10).map(|i| format!("/new/f{i:02}.txt")).collect();
        assert_eq!(s.ambiguous[0].candidates, want);
    }

    #[test]
    fn two_missing_resources_and_one_file_are_both_ambiguous() {
        // R4 and R5 have the same size and content: one file cannot be told apart.
        let mut store = refresh_store();
        let before = resources_ttl(&store);
        let tree = tree_of("/new", &[("/new/moved.txt", 6)]);
        let (result, calls) = search(
            &mut store,
            &["/new"],
            &tree,
            all_but(&[GONE, SNEAKY]),
            hello,
        );
        let m = result.unwrap();
        let s = &m.value;
        assert_eq!(
            (s.counts.relinked, s.counts.ambiguous, s.counts.unmatched),
            (0, 2, 0)
        );
        assert_eq!(
            s.ambiguous
                .iter()
                .map(|a| a.resource.id.as_str())
                .collect::<Vec<_>>(),
            [R4, R5]
        );
        for a in &s.ambiguous {
            assert_eq!(a.candidates, ["/new/moved.txt"]);
        }
        assert!(m.changes.is_empty());
        assert_eq!(resources_ttl(&store), before);
        assert_eq!(
            calls.hashed.len(),
            1,
            "a file is hashed once: {:?}",
            calls.hashed
        );
    }

    #[test]
    fn two_missing_resources_with_their_own_files_are_both_relinked() {
        let mut store = generated(&[make(1, 6, SHA_HELLO), make(2, 12, SHA_WORLD)], None);
        let tree = tree_of("/new", &[("/new/y.txt", 12), ("/new/x.txt", 6)]);
        let (result, _) = search(
            &mut store,
            &["/new"],
            &tree,
            |_: &str| None,
            |_: &str, cap: u64| {
                Some(match cap {
                    6 => hashed(6, SHA_HELLO),
                    _ => hashed(12, SHA_WORLD),
                })
            },
        );
        let m = result.unwrap();
        let s = &m.value;
        assert_eq!(s.counts.relinked, 2);
        let listed: Vec<&str> = s.relinked.iter().map(|r| r.resource.id.as_str()).collect();
        assert_eq!(listed, [rid(1).as_str(), rid(2).as_str()]);
        assert_eq!(s.relinked[0].resource.path, "/new/x.txt");
        assert_eq!(s.relinked[1].resource.path, "/new/y.txt");
        assert_eq!(m.changes.len(), 12);
    }

    #[test]
    fn a_hash_that_differs_is_no_match() {
        // Same size, other content; and a file that cannot be hashed within the cap.
        for answer in [Some(hashed(6, SHA_UPPER)), Some(hashed(6, SHA_WORLD)), None] {
            let mut store = refresh_store();
            let before = resources_ttl(&store);
            let tree = tree_of("/new", &[("/new/moved.txt", 6)]);
            let (result, calls) = search(
                &mut store,
                &["/new"],
                &tree,
                all_but(&[GONE]),
                |_: &str, _: u64| answer.clone(),
            );
            let m = result.unwrap();
            let s = &m.value;
            assert_eq!(
                (s.counts.relinked, s.counts.ambiguous, s.counts.unmatched),
                (0, 0, 1),
                "{answer:?}"
            );
            assert_eq!(ids(&s.unmatched), [R4]);
            assert!(m.changes.is_empty());
            assert_eq!(resources_ttl(&store), before);
            assert_eq!(calls.hashed.len(), 1);
        }
    }

    #[test]
    fn a_file_that_grew_while_hashed_is_no_match() {
        // `hash` reports what it read: more bytes than the record is not a match.
        let mut store = refresh_store();
        let tree = tree_of("/new", &[("/new/moved.txt", 6)]);
        let (result, _) = search(
            &mut store,
            &["/new"],
            &tree,
            all_but(&[GONE]),
            |_: &str, _: u64| Some(hashed(7, SHA_HELLO)),
        );
        let s = result.unwrap().value;
        assert_eq!((s.counts.relinked, s.counts.unmatched), (0, 1));
    }

    // ---- 5, 6: size first, capped read ---------------------------------------------------------

    #[test]
    fn only_files_with_a_missing_size_are_hashed_and_with_that_cap() {
        let mut store = refresh_store();
        let tree = tree_of(
            "/new",
            &[
                ("/new/five.txt", 5),
                ("/new/six-a.txt", 6),
                ("/new/seven.txt", 7),
                ("/new/six-b.txt", 6),
                ("/new/empty.txt", 0),
                ("/new/huge.bin", 1 << 40),
            ],
        );
        let (result, calls) = search(&mut store, &["/new"], &tree, all_but(&[GONE]), hello);
        assert!(result.is_ok());
        let mut hashed_paths = calls.hashed.clone();
        hashed_paths.sort();
        assert_eq!(
            hashed_paths,
            [
                ("/new/six-a.txt".to_string(), 6),
                ("/new/six-b.txt".to_string(), 6)
            ],
            "size first; the cap is the recorded size"
        );
    }

    #[test]
    fn cataloged_paths_are_not_candidates() {
        // same.txt is cataloged (and has R4's content): it cannot be where R4 went.
        let mut store = refresh_store();
        let tree = tree_of(
            "/fixtures/refresh",
            &[(SAME, 6), ("/fixtures/refresh/moved.txt", 6)],
        );
        let (result, calls) = search(
            &mut store,
            &["/fixtures/refresh"],
            &tree,
            all_but(&[GONE]),
            hello,
        );
        let s = result.unwrap().value;
        assert_eq!(s.counts.relinked, 1);
        assert_eq!(s.relinked[0].resource.path, "/fixtures/refresh/moved.txt");
        assert!(
            calls.hashed.iter().all(|(p, _)| p != SAME),
            "{:?}",
            calls.hashed
        );
    }

    #[test]
    fn a_path_under_two_roots_counts_once() {
        let mut store = refresh_store();
        let tree: Tree = vec![
            ("/new", vec![("/new/sub/moved.txt".to_string(), 6)]),
            ("/new/sub", vec![("/new/sub/moved.txt".to_string(), 6)]),
        ];
        let (result, calls) = search(
            &mut store,
            &["/new", "/new/sub"],
            &tree,
            all_but(&[GONE]),
            hello,
        );
        let s = result.unwrap().value;
        assert_eq!((s.counts.relinked, s.counts.ambiguous), (1, 0));
        assert_eq!(calls.walked, ["/new", "/new/sub"]);
        assert_eq!(calls.hashed.len(), 1);
    }

    #[test]
    fn nothing_missing_changes_nothing() {
        let mut store = refresh_store();
        let before = resources_ttl(&store);
        let tree = tree_of("/new", &[("/new/moved.txt", 6)]);
        let (result, calls) = search(&mut store, &["/new"], &tree, all_but(&[]), hello);
        let m = result.unwrap();
        let c = &m.value.counts;
        assert_eq!((c.relinked, c.ambiguous, c.unmatched), (0, 0, 0));
        assert!(m.changes.is_empty());
        assert!(calls.hashed.is_empty());
        assert_eq!(resources_ttl(&store), before);
    }

    // ---- 7: one mutation, one SHACL check ------------------------------------------------------

    #[test]
    fn one_shacl_check_over_every_relink() {
        // The shape allows only the recorded URLs, so each relink violates it. A check per
        // Resource would stop at the first and list one violation; one check on the whole
        // mutation lists both. The store is untouched.
        let ext = urls_stay(&["file:///gen/old/r1.txt", "file:///gen/old/r2.txt"]);
        let mut store = generated(&[make(1, 6, SHA_HELLO), make(2, 12, SHA_WORLD)], Some(&ext));
        let before = resources_ttl(&store);
        let tree = tree_of("/new", &[("/new/x.txt", 6), ("/new/y.txt", 12)]);
        let (result, _) = search(
            &mut store,
            &["/new"],
            &tree,
            |_: &str| None,
            |_: &str, cap: u64| {
                Some(match cap {
                    6 => hashed(6, SHA_HELLO),
                    _ => hashed(12, SHA_WORLD),
                })
            },
        );
        let err = result.expect_err("the extension shape is violated");
        assert_eq!(err.rule, "SHACL_VIOLATION");
        assert_eq!(err.exit_code(), 1);
        assert_eq!(err.details["total"], 2);
        let focus: BTreeSet<&str> = err.details["violations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["focus_node"].as_str().unwrap())
            .collect();
        assert_eq!(focus.len(), 2, "both Resources are listed: {focus:?}");
        assert_eq!(resources_ttl(&store), before, "the store is untouched");
    }

    // ---- 8: caps ---------------------------------------------------------------------------------

    #[test]
    fn lists_are_capped_at_100_and_counts_stay_exact() {
        let gens: Vec<Gen> = (1..=101u32)
            .map(|n| make(n, u64::from(n), &sha(n as usize)))
            .collect();
        let mut store = generated(&gens, None);
        let files: Vec<(String, u64)> = (1..=101u64)
            .map(|n| (format!("/new/f{n:03}.txt"), n))
            .collect();
        let tree: Tree = vec![("/new", files)];
        let (result, _) = search(
            &mut store,
            &["/new"],
            &tree,
            |_: &str| None,
            |_: &str, cap: u64| Some(hashed(cap, &sha(cap as usize))),
        );
        let m = result.unwrap();
        let s = &m.value;
        assert_eq!(s.counts.relinked, 101);
        assert_eq!(s.relinked.len(), 100);
        assert!(s.truncated);
        let listed: Vec<String> = s.relinked.iter().map(|r| r.resource.id.clone()).collect();
        let mut sorted = listed.clone();
        sorted.sort();
        assert_eq!(listed, sorted, "id order");
        assert_eq!(listed[0], rid(1));
        assert_eq!(listed[99], rid(100));
        assert_eq!(
            m.changes.len(),
            101 * 6,
            "every relink is written, listed or not"
        );
    }

    // ---- 24: empty files ---------------------------------------------------------------------

    #[test]
    fn empty_files_are_excluded_from_matching() {
        let mut store = generated(&[make(1, 0, SHA_EMPTY)], None);
        let before = resources_ttl(&store);
        let tree = tree_of("/new", &[("/new/e1.txt", 0), ("/new/e2.txt", 0)]);
        let (result, calls) = search(
            &mut store,
            &["/new"],
            &tree,
            |_: &str| None,
            |_: &str, _: u64| Some(hashed(0, SHA_EMPTY)),
        );
        let m = result.unwrap();
        let s = &m.value;
        assert_eq!((s.counts.relinked, s.counts.ambiguous), (0, 0));
        assert!(s.relinked.is_empty() && s.ambiguous.is_empty());
        assert!(calls.hashed.is_empty(), "{:?}", calls.hashed);
        assert!(m.changes.is_empty());
        assert_eq!(resources_ttl(&store), before);
    }

    #[test]
    fn an_empty_file_is_never_a_possible_move() {
        let store = generated(&[make(1, 0, SHA_EMPTY)], None);
        let new = FileFacts {
            path: "/new/empty.txt".to_string(),
            byte_size: 0,
            modified: OLD.to_string(),
            sha256: SHA_EMPTY.to_string(),
            media_type: "text/plain".to_string(),
        };
        assert!(resource::check_moves(&store, &[new], |_: &str| None).is_ok());
    }

    // ---- explicit relink -----------------------------------------------------------------------

    fn explicit(
        store: &mut Store,
        id: &str,
        path: &str,
        h: &Hashed,
        force: bool,
    ) -> Result<Mutation<relink::Explicit>, Error> {
        relink::explicit(&ctx(), store, id, path, h, force)
    }

    #[test]
    fn explicit_relink_of_the_same_content() {
        let mut store = refresh_store();
        let m = explicit(
            &mut store,
            R4,
            "/new/moved.txt",
            &hashed(6, SHA_HELLO),
            false,
        )
        .unwrap();
        assert_eq!(m.value.from, GONE);
        assert_eq!(m.value.resource.id, R4);
        assert_eq!(m.value.resource.path, "/new/moved.txt");
        assert!(!m.value.refreshed);
        let got: Vec<(Op, String)> = m.changes.iter().map(|c| (c.op, c.triple.clone())).collect();
        assert_eq!(got, relink_changes("/new/moved.txt"));
    }

    #[test]
    fn explicit_relink_of_other_content_is_a_hash_mismatch() {
        // Another size, and the same size with other content: both refused, nothing written.
        for other in [hashed(12, SHA_WORLD), hashed(6, SHA_UPPER)] {
            let mut store = refresh_store();
            let before = resources_ttl(&store);
            let err = explicit(&mut store, R4, "/new/moved.txt", &other, false)
                .expect_err("different content");
            assert_eq!(err.rule, "HASH_MISMATCH");
            assert_eq!(err.exit_code(), 1);
            assert_eq!(err.details["resource"], R4);
            assert_eq!(err.details["path"], "/new/moved.txt");
            assert_eq!(err.details["expected"], SHA_HELLO);
            assert_eq!(err.details["actual"], other.sha256.as_str());
            assert_eq!(
                err.suggested_command.as_deref(),
                Some(format!("hali relink {R4} '/new/moved.txt' --force").as_str())
            );
            assert!(
                err.suggested_fix
                    .as_deref()
                    .is_some_and(|f| f.contains("--force"))
            );
            assert_eq!(resources_ttl(&store), before);
        }
    }

    #[test]
    fn explicit_force_refreshes_the_hash() {
        let mut store = refresh_store();
        let m = explicit(
            &mut store,
            R4,
            "/new/moved.txt",
            &hashed(12, SHA_WORLD),
            true,
        )
        .unwrap();
        assert!(m.value.refreshed);
        assert_eq!(m.value.resource.byte_size, 12);
        assert_eq!(m.value.resource.path, "/new/moved.txt");
        let int = |n: u32| format!("\"{n}\"^^<{XSD}integer>");
        let checksum = |sha: &str| {
            format!(
                "<urn:uuid:{R4}#sha256> <http://spdx.org/rdf/terms#checksumValue> \"{sha}\"^^<{XSD}hexBinary> ."
            )
        };
        let mut want: BTreeSet<(Op, String)> =
            relink_changes("/new/moved.txt").into_iter().collect();
        want.insert((Op::Add, quad(R4, BYTE_SIZE, &int(12))));
        want.insert((Op::Remove, quad(R4, BYTE_SIZE, &int(6))));
        want.insert((Op::Add, checksum(SHA_WORLD)));
        want.insert((Op::Remove, checksum(SHA_HELLO)));
        assert_eq!(change_set(&m), want);
    }

    #[test]
    fn explicit_force_with_the_same_content_refreshes_nothing() {
        let mut store = refresh_store();
        let m = explicit(
            &mut store,
            R4,
            "/new/moved.txt",
            &hashed(6, SHA_HELLO),
            true,
        )
        .unwrap();
        assert!(!m.value.refreshed);
        assert_eq!(m.changes.len(), 6);
    }

    #[test]
    fn explicit_relink_of_a_resource_that_is_not_missing() {
        let mut store = refresh_store();
        let m = explicit(
            &mut store,
            R1,
            "/new/copy.txt",
            &hashed(6, SHA_HELLO),
            false,
        )
        .unwrap();
        assert_eq!(m.value.from, SAME);
        assert_eq!(m.value.resource.path, "/new/copy.txt");
    }

    #[test]
    fn explicit_relink_of_an_unknown_resource() {
        let mut store = refresh_store();
        let err = explicit(
            &mut store,
            "0192f0c4-0000-7000-8000-000000000999",
            "/new/moved.txt",
            &hashed(6, SHA_HELLO),
            false,
        )
        .expect_err("no such Resource");
        assert_eq!(err.rule, "UNKNOWN_RESOURCE");
        assert_eq!(err.exit_code(), 1);
        assert_eq!(err.details["ref"], "0192f0c4-0000-7000-8000-000000000999");
    }

    #[test]
    fn explicit_relink_is_one_checked_mutation() {
        let ext = urls_stay(&["file:///gen/old/r1.txt"]);
        let mut store = generated(&[make(1, 6, SHA_HELLO)], Some(&ext));
        let before = resources_ttl(&store);
        let err = explicit(
            &mut store,
            &rid(1),
            "/new/x.txt",
            &hashed(6, SHA_HELLO),
            false,
        )
        .expect_err("the extension shape is violated");
        assert_eq!(err.rule, "SHACL_VIOLATION");
        assert_eq!(err.exit_code(), 1);
        assert_eq!(resources_ttl(&store), before, "the store is untouched");
    }

    // ---- POSSIBLE_MOVE: check_moves ------------------------------------------------------------

    fn new_file(path: &str, size: u64, sha256: &str) -> FileFacts {
        FileFacts {
            path: path.to_string(),
            byte_size: size,
            modified: OLD.to_string(),
            sha256: sha256.to_string(),
            media_type: "text/plain".to_string(),
        }
    }

    #[test]
    fn a_new_path_with_the_content_of_a_missing_resource_is_a_possible_move() {
        let store = refresh_store();
        let err = resource::check_moves(
            &store,
            &[new_file("/new/moved.txt", 6, SHA_HELLO)],
            all_but(&[GONE]),
        )
        .expect_err("R4's content");
        assert_eq!(err.rule, "POSSIBLE_MOVE");
        assert_eq!(err.exit_code(), 1);
        assert_eq!(err.details["path"], "/new/moved.txt");
        assert_eq!(err.details["resources"], serde_json::json!([R4]));
        assert_eq!(
            err.suggested_command.as_deref(),
            Some(format!("hali relink {R4} '/new/moved.txt'").as_str())
        );
        assert!(
            err.suggested_fix
                .as_deref()
                .is_some_and(|f| f.contains("relink"))
        );
        assert!(
            err.suggested_fix
                .as_deref()
                .is_some_and(|f| f.contains("--new"))
        );
        assert!(err.message.contains("/new/moved.txt") && err.message.contains(R4));
    }

    #[test]
    fn several_missing_resources_are_listed_sorted() {
        let store = refresh_store();
        let err = resource::check_moves(
            &store,
            &[new_file("/new/moved.txt", 6, SHA_HELLO)],
            all_but(&[SNEAKY, GONE]),
        )
        .expect_err("R4's and R5's content");
        assert_eq!(err.rule, "POSSIBLE_MOVE");
        assert_eq!(err.details["resources"], serde_json::json!([R4, R5]));
        assert_eq!(err.suggested_command.as_deref(), Some("hali relink"));
    }

    #[test]
    fn the_first_offender_in_argument_order_is_reported() {
        let store = refresh_store();
        let err = resource::check_moves(
            &store,
            &[
                new_file("/new/fine.txt", 6, SHA_WORLD),
                new_file("/new/second.txt", 6, SHA_HELLO),
                new_file("/new/first.txt", 6, SHA_HELLO),
            ],
            all_but(&[GONE]),
        )
        .expect_err("two offenders");
        assert_eq!(err.details["path"], "/new/second.txt");
    }

    #[test]
    fn nothing_that_is_not_a_move_is_refused() {
        let store = refresh_store();
        let missing = all_but(&[GONE]);
        let fine: Vec<(&str, Vec<FileFacts>)> = vec![
            (
                "other content, same size",
                vec![new_file("/new/a.txt", 6, SHA_UPPER)],
            ),
            (
                "same hash, other size",
                vec![new_file("/new/a.txt", 7, SHA_HELLO)],
            ),
            (
                "a cataloged path is a re-add",
                vec![new_file(GONE, 6, SHA_HELLO)],
            ),
            ("no files", vec![]),
        ];
        for (what, files) in fine {
            assert!(
                resource::check_moves(&store, &files, &missing).is_ok(),
                "{what}"
            );
        }
        // Content that only Resources whose files are there have is a duplicate, allowed.
        assert!(
            resource::check_moves(
                &store,
                &[new_file("/new/copy.txt", 6, SHA_HELLO)],
                all_but(&[])
            )
            .is_ok()
        );
    }

    // ---- default roots: the config ---------------------------------------------------------------

    #[test]
    fn files_dir_comes_from_the_config() {
        let path = Path::new("config.toml");
        let c = config::parse("[files]\ndir = \"/docs/hali\"\n", path).unwrap();
        assert_eq!(c.files_dir.as_deref(), Some("/docs/hali"));
        assert_eq!(config::parse("", path).unwrap().files_dir, None);
        assert_eq!(config::parse("[files]\n", path).unwrap().files_dir, None);
        for bad in [
            "[files]\ndir = 5\n",
            "files = 5\n",
            "[files]\ndir = [\"/a\"]\n",
        ] {
            let err = config::parse(bad, path).err().expect(bad);
            assert_eq!(err.rule, "CONFIG_INVALID", "{bad}");
            assert_eq!(err.exit_code(), 3, "{bad}");
        }
    }

    // ---- 25-29: reviewer rows ----------------------------------------------------------------

    // Row 26: an explicit relink never binds a path another Resource records.
    #[test]
    fn explicit_relink_never_binds_a_path_another_resource_records() {
        let mut store = refresh_store();
        let before = resources_ttl(&store);
        // R1 records SAME; R4 is Missing with the same content and asks for it.
        let err = explicit(&mut store, R4, SAME, &hashed(6, SHA_HELLO), false)
            .expect_err("SAME is R1's path");
        assert_eq!(err.rule, "USAGE");
        assert_eq!(err.exit_code(), 2);
        assert_eq!(resources_ttl(&store), before, "nothing is written");
        // Forcing does not lift it.
        let err = explicit(&mut store, R4, SAME, &hashed(6, SHA_HELLO), true)
            .expect_err("SAME is R1's path");
        assert_eq!(err.rule, "USAGE");
        assert_eq!(resources_ttl(&store), before, "nothing is written");
        // A Resource's own path is no other Resource's: that still works.
        let m = explicit(&mut store, R1, SAME, &hashed(6, SHA_HELLO), false).unwrap();
        assert_eq!(m.value.resource.path, SAME);
    }

    // Row 27: caps.
    fn capped_search(n: u32, ambiguous: bool) -> Searched {
        let gens: Vec<Gen> = (1..=n)
            .map(|k| make(k, u64::from(k), &sha(k as usize)))
            .collect();
        let mut store = generated(&gens, None);
        let mut files: Vec<(String, u64)> = Vec::new();
        if ambiguous {
            for k in 1..=u64::from(n) {
                files.push((format!("/new/f{k:03}a.txt"), k));
                files.push((format!("/new/f{k:03}b.txt"), k));
            }
        }
        let tree: Tree = vec![("/new", files)];
        let (result, _) = search(
            &mut store,
            &["/new"],
            &tree,
            |_: &str| None,
            |_: &str, cap: u64| Some(hashed(cap, &sha(cap as usize))),
        );
        result.unwrap().value
    }

    #[test]
    fn the_unmatched_list_is_capped_at_100() {
        let s = capped_search(101, false);
        assert_eq!(s.counts.unmatched, 101);
        assert_eq!(s.unmatched.len(), 100);
        assert_eq!((s.counts.relinked, s.counts.ambiguous), (0, 0));
        assert!(s.truncated, "an unmatched list was cut");
        let s = capped_search(100, false);
        assert_eq!((s.counts.unmatched, s.unmatched.len()), (100, 100));
        assert!(!s.truncated, "exactly 100 is not cut");
    }

    #[test]
    fn the_ambiguous_list_is_capped_at_100() {
        let s = capped_search(101, true);
        assert_eq!(s.counts.ambiguous, 101);
        assert_eq!(s.ambiguous.len(), 100);
        assert_eq!((s.counts.relinked, s.counts.unmatched), (0, 0));
        assert!(s.truncated, "an ambiguous list was cut");
        let s = capped_search(100, true);
        assert_eq!((s.counts.ambiguous, s.ambiguous.len()), (100, 100));
        assert!(!s.truncated, "exactly 100 is not cut");
    }

    #[test]
    fn exactly_100_relinked_is_not_truncated() {
        let gens: Vec<Gen> = (1..=100u32)
            .map(|n| make(n, u64::from(n), &sha(n as usize)))
            .collect();
        let mut store = generated(&gens, None);
        let files: Vec<(String, u64)> = (1..=100u64)
            .map(|n| (format!("/new/f{n:03}.txt"), n))
            .collect();
        let tree: Tree = vec![("/new", files)];
        let (result, _) = search(
            &mut store,
            &["/new"],
            &tree,
            |_: &str| None,
            |_: &str, cap: u64| Some(hashed(cap, &sha(cap as usize))),
        );
        let s = result.unwrap().value;
        assert_eq!((s.counts.relinked, s.relinked.len()), (100, 100));
        assert!(!s.truncated);
    }

    // Row 28: a candidate that cannot be hashed is skipped, not fatal for the others.
    #[test]
    fn an_unreadable_candidate_does_not_stop_the_search() {
        for (bad, good) in [("/new/a.txt", "/new/b.txt"), ("/new/b.txt", "/new/a.txt")] {
            let mut store = refresh_store();
            let tree = tree_of("/new", &[("/new/a.txt", 6), ("/new/b.txt", 6)]);
            let (result, calls) = search(
                &mut store,
                &["/new"],
                &tree,
                all_but(&[GONE]),
                |path: &str, _: u64| (path != bad).then(|| hashed(6, SHA_HELLO)),
            );
            let s = result.unwrap().value;
            assert_eq!(calls.hashed.len(), 2, "both are tried ({bad} unreadable)");
            assert_eq!((s.counts.relinked, s.counts.ambiguous), (1, 0), "{bad}");
            assert_eq!(s.relinked[0].resource.path, good, "{bad}");
        }
    }

    // Row 29: check_moves looks at the file system only for Resources that could be the move.
    #[test]
    fn check_moves_stats_only_resources_with_the_content() {
        let gens: Vec<Gen> = (1..=3u32)
            .map(|n| make(n, u64::from(n) + 10, &sha(n as usize)))
            .collect();
        let store = generated(&gens, None);
        let stats = RefCell::new(Vec::<String>::new());
        let stat = |path: &str| {
            stats.borrow_mut().push(path.to_string());
            None
        };
        // Other content, and the size of one but the hash of another: nothing is stat-ed.
        resource::check_moves(
            &store,
            &[
                new_file("/new/a.txt", 12, &sha(99)),
                new_file("/new/b.txt", 12, &sha(1)),
            ],
            stat,
        )
        .unwrap();
        assert!(stats.borrow().is_empty(), "{:?}", stats.borrow());
        // The content of R2 only: R2 alone is stat-ed.
        let err = resource::check_moves(&store, &[new_file("/new/c.txt", 12, &sha(2))], stat)
            .expect_err("R2's content");
        assert_eq!(err.rule, "POSSIBLE_MOVE");
        assert_eq!(*stats.borrow(), ["/gen/old/r2.txt"]);
    }
}
