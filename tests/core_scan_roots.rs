//! Goal tests for scanning several remembered roots as one run, in `core` (#23, spec §7,
//! §9.1 **Scan:**).
//!
//! Assumed API (invented here; the implementer follows it), in `hali::core::scan`:
//!   `struct RootScan { pub options: ScanOptions, pub entries: Vec<Walked> }`: one root, its
//!       own settings (`options.max_size`, `options.media_types`) and its walk.
//!   `scan_roots(&Context, &mut Store, roots: Vec<RootScan>, unavailable: &[String], prune:
//!       Prune, stat, inspect, glob_type, progress, user_name) -> Result<Mutation<Scanned>,
//!       Error>`
//!       with `stat`, `inspect`, `glob_type`, `progress` and `user_name` as in `scan_with`.
//!       `unavailable` are the real paths of remembered roots that could not be walked.
//!
//! What `scan_roots` does (`scan_with` of one root is `scan_roots` of one `RootScan` and no
//! unavailable root):
//!   1. Each root's walk is filtered with that root's own options.
//!   2. It is one run: one set of changes, one SHACL check, ids minted in byte order of the
//!      paths of all the files registered, whatever the order of the roots. A cataloged
//!      Resource under several roots is `stat`ed and counted once; a file walked under several
//!      roots (nested roots) is registered once.
//!   3. Move matching is one-to-one across all the roots (a Missing Resource under one root
//!      matches a new file under another).
//!   4. Prune is decided after the moves, over the Missing Resources under any scanned root.
//!      A Resource under an unavailable root is never pruned.
//!
//! `Scanned` is the same as for one root (counts, lists, `truncated`); the CLI adds `roots`
//! and `unavailable` to what it prints.

#[cfg(feature = "goal")]
mod goal {
    use hali::core::resource::{self, FileFacts, parse_media_pattern};
    use hali::core::scan::{self, Prune, RootScan, ScanOptions, Scanned, Unhashable};
    use hali::core::{Context, Error, FixedClock, Mutation, SeededIdGen};
    use hali::storage::walk::Walked;
    use hali::storage::{File, Store};
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};
    use std::time::SystemTime;

    const NOW: &str = "2026-10-24T09:30:00Z";
    const SETUP_NOW: &str = "2026-10-21T08:00:00Z";
    const OLD: &str = "2026-10-20T14:02:11Z";
    const NEW: &str = "2026-10-22T10:00:00Z";
    /// The mtime (seconds) that matches `OLD`.
    const PINNED: i64 = 1_792_504_931;

    fn ctx() -> Context {
        Context::new(
            "en",
            Box::new(FixedClock::new(NOW)),
            Box::new(SeededIdGen::new(1)),
        )
    }

    /// A context for building the store a scan starts from: other ids and another clock.
    fn setup_ctx() -> Context {
        Context::new(
            "en",
            Box::new(FixedClock::new(SETUP_NOW)),
            Box::new(SeededIdGen::new(100)),
        )
    }

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    fn empty_store() -> Store {
        hali::storage::load(Path::new("/nonexistent/hali-core-scan-roots")).unwrap()
    }

    fn resources_ttl(store: &Store) -> String {
        hali::storage::to_turtle(store, File::Resources)
    }

    fn sha(i: usize) -> String {
        format!("{i:064x}")
    }

    fn facts(path: &str, size: u64, sha_index: usize, modified: &str) -> FileFacts {
        FileFacts {
            path: path.to_string(),
            byte_size: size,
            modified: modified.to_string(),
            sha256: sha(sha_index),
            media_type: "text/plain".to_string(),
        }
    }

    fn file(path: &str, size: u64) -> Walked {
        Walked::File {
            path: path.to_string(),
            byte_size: size,
            mtime: SystemTime::UNIX_EPOCH,
        }
    }

    fn opts(root: &str, max_size: u64, patterns: &[&str]) -> ScanOptions {
        ScanOptions {
            root: root.to_string(),
            max_size,
            media_types: patterns
                .iter()
                .map(|p| parse_media_pattern(p).unwrap())
                .collect(),
        }
    }

    fn root(root: &str, entries: Vec<Walked>) -> RootScan {
        RootScan {
            options: opts(root, 1 << 30, &[]),
            entries,
        }
    }

    /// Catalogs `(path, byte size, sha(i))` text files, mtime `OLD`; returns their ids.
    fn catalog(store: &mut Store, items: &[(&str, u64, usize)]) -> Vec<String> {
        let files: Vec<FileFacts> = items
            .iter()
            .map(|(p, size, i)| facts(p, *size, *i, OLD))
            .collect();
        resource::add_all(&setup_ctx(), store, &files, None, &[], &[], "tester")
            .unwrap()
            .value
            .added
            .into_iter()
            .map(|r| r.id)
            .collect()
    }

    /// `stat` that sees exactly these files, as recorded (not Stale).
    fn stat_in(present: &'static [(&'static str, u64)]) -> impl Fn(&str) -> Option<(u64, i64)> {
        move |p| {
            present
                .iter()
                .find(|(q, _)| *q == p)
                .map(|(_, size)| (*size, PINNED))
        }
    }

    /// Hashing: the new files by path, `(size, sha index)`; anything else is gone.
    fn inspect_in(
        known: &'static [(&'static str, u64, usize)],
    ) -> impl Fn(&str) -> Result<FileFacts, Unhashable> {
        move |p| {
            known
                .iter()
                .find(|(q, _, _)| *q == p)
                .map(|(q, size, i)| facts(q, *size, *i, NEW))
                .ok_or(Unhashable::Missing)
        }
    }

    type Run = (Result<Mutation<Scanned>, Error>, Vec<String>, Vec<String>);

    /// Runs `scan_roots` with logging fakes: the result, the paths `stat` and `inspect` saw.
    fn run(
        store: &mut Store,
        roots: Vec<RootScan>,
        unavailable: &[&str],
        prune: Prune,
        stat: impl Fn(&str) -> Option<(u64, i64)>,
        inspect: impl Fn(&str) -> Result<FileFacts, Unhashable>,
    ) -> Run {
        let stats = RefCell::new(Vec::new());
        let inspects = RefCell::new(Vec::new());
        let unavailable: Vec<String> = unavailable.iter().map(|s| s.to_string()).collect();
        let result = scan::scan_roots(
            &ctx(),
            store,
            roots,
            &unavailable,
            prune,
            |p: &str| {
                stats.borrow_mut().push(p.to_string());
                stat(p)
            },
            |p: &str| {
                inspects.borrow_mut().push(p.to_string());
                inspect(p)
            },
            |_: &str| None,
            |_: usize, _: usize| {},
            "tester",
        );
        (result, stats.into_inner(), inspects.into_inner())
    }

    fn paths(records: &[resource::Record]) -> Vec<&str> {
        records.iter().map(|r| r.path.as_str()).collect()
    }

    fn ids(records: &[resource::Record]) -> Vec<&str> {
        records.iter().map(|r| r.id.as_str()).collect()
    }

    /// added, refreshed, relinked, ambiguous, unchanged, missing, pruned, skipped.
    fn tally(c: &scan::Counts) -> [usize; 8] {
        [
            c.added,
            c.refreshed,
            c.relinked,
            c.ambiguous,
            c.unchanged,
            c.missing,
            c.pruned,
            c.skipped,
        ]
    }

    fn moves(scanned: &Scanned) -> Vec<(&str, &str)> {
        scanned
            .relinked
            .iter()
            .map(|r| (r.from.as_str(), r.resource.path.as_str()))
            .collect()
    }

    // ---- row 5: every root's own settings ------------------------------------------------------

    /// Row 5: One root through `scan_roots` is exactly `scan_with`: the same store, the same
    /// changes, the same outcome.
    #[test]
    fn one_root_is_scan_with() {
        let build = || {
            let mut store = empty_store();
            catalog(
                &mut store,
                &[
                    ("/r/gone.txt", 5, 5),
                    ("/r/old/m.txt", 6, 6),
                    ("/r/k.txt", 7, 7),
                ],
            );
            store
        };
        let entries = || {
            vec![
                file("/r/k.txt", 7),
                file("/r/new/m.txt", 6),
                file("/r/n.txt", 9),
            ]
        };
        const PRESENT: &[(&str, u64)] = &[("/r/k.txt", 7)];
        const KNOWN: &[(&str, u64, usize)] = &[("/r/new/m.txt", 6, 6), ("/r/n.txt", 9, 9)];

        let mut a = build();
        let (result, _, _) = run(
            &mut a,
            vec![root("/r", entries())],
            &[],
            Prune::On,
            stat_in(PRESENT),
            inspect_in(KNOWN),
        );
        let got = result.unwrap();

        let mut b = build();
        let want = scan::scan_with(
            &ctx(),
            &mut b,
            &opts("/r", 1 << 30, &[]),
            Prune::On,
            entries(),
            stat_in(PRESENT),
            inspect_in(KNOWN),
            |_: &str| None,
            |_: usize, _: usize| {},
            "tester",
        )
        .unwrap();
        assert_eq!(got, want);
        assert_eq!(resources_ttl(&a), resources_ttl(&b));
        assert_eq!(tally(&got.value.counts), [1, 0, 1, 0, 1, 0, 1, 0]);
    }

    /// Row 5: The size limit and the media type filter of each root apply to that root's files
    /// only.
    #[test]
    fn each_root_uses_its_own_options() {
        let mut store = empty_store();
        let (result, _, inspects) = run(
            &mut store,
            vec![
                RootScan {
                    options: opts("/a", 5, &[]),
                    entries: vec![file("/a/big.txt", 10), file("/a/small.txt", 4)],
                },
                RootScan {
                    options: opts("/b", 100, &[]),
                    entries: vec![file("/b/big.txt", 10)],
                },
                RootScan {
                    options: opts("/c", 100, &["application/pdf"]),
                    entries: vec![file("/c/note.txt", 3)],
                },
            ],
            &[],
            Prune::Off,
            |_| None,
            inspect_in(&[
                ("/a/small.txt", 4, 1),
                ("/b/big.txt", 10, 2),
                ("/c/note.txt", 3, 3),
            ]),
        );
        let m = result.unwrap();
        let v = &m.value;
        assert_eq!(paths(&v.added), ["/a/small.txt", "/b/big.txt"]);
        assert_eq!(
            v.skipped
                .iter()
                .map(|s| (s.path.as_str(), s.reason))
                .collect::<Vec<_>>(),
            [("/a/big.txt", "too_large")],
            "/a/big.txt is over /a's limit, though /b's would take it; /c/note.txt is dropped"
        );
        assert_eq!(tally(&v.counts), [2, 0, 0, 0, 0, 0, 0, 1]);
        assert!(
            !inspects.contains(&"/a/big.txt".to_string()),
            "a too large file is never opened: {inspects:?}"
        );
    }

    // ---- row 6: moves and prune across roots -------------------------------------------------

    /// Row 6: A file that moved from one root to another is relinked, not forgotten, whatever
    /// the order of the roots; the Resource that really vanished is pruned.
    #[test]
    fn cross_root_move_is_relinked_not_pruned() {
        let build = || {
            let mut store = empty_store();
            let made = catalog(
                &mut store,
                &[
                    ("/a/x.txt", 5, 5),
                    ("/a/y.txt", 6, 6),
                    ("/b/keep.txt", 7, 7),
                ],
            );
            (store, made)
        };
        const PRESENT: &[(&str, u64)] = &[("/b/keep.txt", 7)];
        const KNOWN: &[(&str, u64, usize)] = &[("/b/x.txt", 5, 5)];
        let roots = |flip: bool| {
            let a = root("/a", vec![]);
            let b = root("/b", vec![file("/b/keep.txt", 7), file("/b/x.txt", 5)]);
            if flip { vec![b, a] } else { vec![a, b] }
        };
        let mut outputs = Vec::new();
        for flip in [false, true] {
            let (mut store, made) = build();
            let (result, _, _) = run(
                &mut store,
                roots(flip),
                &[],
                Prune::On,
                stat_in(PRESENT),
                inspect_in(KNOWN),
            );
            let m = result.unwrap();
            let v = &m.value;
            assert_eq!(moves(v), [("/a/x.txt", "/b/x.txt")], "flip {flip}");
            assert_eq!(v.relinked[0].resource.id, made[0]);
            assert_eq!(
                ids(&v.pruned),
                [made[1].as_str()],
                "flip {flip}: y vanished"
            );
            assert!(v.missing.is_empty(), "flip {flip}: {:?}", v.missing);
            assert!(v.added.is_empty() && v.ambiguous.is_empty());
            assert_eq!(tally(&v.counts), [0, 0, 1, 0, 1, 0, 1, 0], "flip {flip}");
            let ttl = resources_ttl(&store);
            assert!(ttl.contains("file:///b/x.txt"), "flip {flip}");
            assert!(!ttl.contains("file:///a/x.txt"), "flip {flip}");
            assert!(!ttl.contains("file:///a/y.txt"), "flip {flip}");
            outputs.push((ttl, m.changes));
        }
        assert_eq!(
            outputs[0], outputs[1],
            "the order of the roots does not matter"
        );
    }

    /// Row 6: Moves match one-to-one across all the roots: two Missing Resources with two new
    /// files of their contents, one under each root, are both relinked.
    #[test]
    fn two_cross_moves_are_both_relinked() {
        let mut store = empty_store();
        catalog(&mut store, &[("/a/x.txt", 5, 5), ("/b/y.txt", 6, 6)]);
        let (result, _, _) = run(
            &mut store,
            vec![
                root("/a", vec![file("/a/y2.txt", 6)]),
                root("/b", vec![file("/b/x2.txt", 5)]),
            ],
            &[],
            Prune::On,
            |_| None,
            inspect_in(&[("/a/y2.txt", 6, 6), ("/b/x2.txt", 5, 5)]),
        );
        let m = result.unwrap();
        // Spec 9.1: each list is in byte order of the relinked Resource's new path, so
        // /a/y2.txt sorts before /b/x2.txt (not old-path order).
        assert_eq!(
            moves(&m.value),
            [("/b/y.txt", "/a/y2.txt"), ("/a/x.txt", "/b/x2.txt")]
        );
        assert!(m.value.pruned.is_empty() && m.value.missing.is_empty());
        assert_eq!(tally(&m.value.counts), [0, 0, 2, 0, 0, 0, 0, 0]);
    }

    /// Row 6: Ambiguity spans the roots: two Missing Resources with one content, one under each
    /// root, and one new file with it: both are ambiguous, nothing is relinked, the file is not
    /// registered and neither Resource is pruned.
    #[test]
    fn ambiguity_spans_roots() {
        let mut store = empty_store();
        let made = catalog(&mut store, &[("/a/p.txt", 5, 5), ("/b/q.txt", 5, 5)]);
        let (result, _, _) = run(
            &mut store,
            vec![root("/a", vec![file("/a/n.txt", 5)]), root("/b", vec![])],
            &[],
            Prune::On,
            |_| None,
            inspect_in(&[("/a/n.txt", 5, 5)]),
        );
        let m = result.unwrap();
        let v = &m.value;
        let mut ambiguous: Vec<&str> = v.ambiguous.iter().map(|a| a.resource.id.as_str()).collect();
        ambiguous.sort();
        let mut want: Vec<&str> = made.iter().map(String::as_str).collect();
        want.sort();
        assert_eq!(ambiguous, want);
        assert!(v.relinked.is_empty() && v.added.is_empty() && v.pruned.is_empty());
        assert_eq!(v.counts.missing, 2, "both stay Missing");
        assert!(m.changes.is_empty());
    }

    // ---- row 7: unavailable roots -------------------------------------------------------------

    /// Row 7: A Resource under an unavailable root is never pruned, even when a scanned root
    /// contains that root (nested roots); a Missing Resource under a scanned root outside it is.
    #[test]
    fn unavailable_roots_protect_their_resources() {
        let mut store = empty_store();
        let made = catalog(
            &mut store,
            &[
                ("/u/a.txt", 5, 5),
                ("/r/b.txt", 6, 6),
                ("/r/sub/c.txt", 7, 7),
            ],
        );
        let (result, _, _) = run(
            &mut store,
            vec![root("/r", vec![])],
            &["/u", "/r/sub"],
            Prune::Forced,
            |_| None,
            |_| Err(Unhashable::Missing),
        );
        let m = result.unwrap();
        assert_eq!(ids(&m.value.pruned), [made[1].as_str()]);
        let ttl = resources_ttl(&store);
        assert!(ttl.contains("file:///u/a.txt"), "under /u: kept");
        assert!(ttl.contains("file:///r/sub/c.txt"), "under /r/sub: kept");
        assert!(!ttl.contains("file:///r/b.txt"));
    }

    /// Row 7: With no scanned root at all (every remembered root unavailable) nothing changes.
    #[test]
    fn only_unavailable_roots_change_nothing() {
        let mut store = empty_store();
        catalog(&mut store, &[("/u/a.txt", 5, 5)]);
        let before = resources_ttl(&store);
        let (result, stats, inspects) = run(
            &mut store,
            vec![],
            &["/u"],
            Prune::On,
            |_| None,
            |_| Err(Unhashable::Missing),
        );
        let m = result.unwrap();
        assert!(m.changes.is_empty());
        assert_eq!(tally(&m.value.counts), [0; 8]);
        assert_eq!(resources_ttl(&store), before);
        assert!(
            inspects.is_empty() && stats.is_empty(),
            "{stats:?} {inspects:?}"
        );
    }

    /// Row 8: No roots and none unavailable: an empty outcome, no change.
    #[test]
    fn no_roots_is_an_empty_scan() {
        let mut store = empty_store();
        let before = resources_ttl(&store);
        let (result, stats, inspects) = run(
            &mut store,
            vec![],
            &[],
            Prune::On,
            |_| None,
            |_| Err(Unhashable::Missing),
        );
        let m = result.unwrap();
        assert!(m.changes.is_empty());
        assert_eq!(tally(&m.value.counts), [0; 8]);
        assert!(m.value.media_types.is_empty() && !m.value.truncated);
        assert!(stats.is_empty() && inspects.is_empty());
        assert_eq!(resources_ttl(&store), before);
    }

    // ---- one run -------------------------------------------------------------------------------

    /// One run: a file walked under two roots (one inside the other) is registered once, a
    /// Resource under both is `stat`ed and counted once, and the order of the roots changes
    /// nothing.
    #[test]
    fn nested_roots_count_each_file_once() {
        let build = || {
            let mut store = empty_store();
            catalog(&mut store, &[("/r/sub/k.txt", 5, 5)]);
            store
        };
        const PRESENT: &[(&str, u64)] = &[("/r/sub/k.txt", 5)];
        const KNOWN: &[(&str, u64, usize)] = &[("/r/sub/n.txt", 6, 6), ("/r/top.txt", 7, 7)];
        let roots = |flip: bool| {
            let outer = root(
                "/r",
                vec![
                    file("/r/sub/k.txt", 5),
                    file("/r/sub/n.txt", 6),
                    file("/r/top.txt", 7),
                ],
            );
            let inner = root(
                "/r/sub",
                vec![file("/r/sub/k.txt", 5), file("/r/sub/n.txt", 6)],
            );
            if flip {
                vec![inner, outer]
            } else {
                vec![outer, inner]
            }
        };
        let mut seen = Vec::new();
        for flip in [false, true] {
            let mut store = build();
            let (result, stats, inspects) = run(
                &mut store,
                roots(flip),
                &[],
                Prune::Off,
                stat_in(PRESENT),
                inspect_in(KNOWN),
            );
            let m = result.unwrap();
            assert_eq!(
                paths(&m.value.added),
                ["/r/sub/n.txt", "/r/top.txt"],
                "flip {flip}"
            );
            assert_eq!(
                tally(&m.value.counts),
                [2, 0, 0, 0, 1, 0, 0, 0],
                "flip {flip}"
            );
            assert_eq!(
                stats.iter().filter(|p| *p == "/r/sub/k.txt").count(),
                1,
                "flip {flip}: {stats:?}"
            );
            assert_eq!(
                inspects.iter().filter(|p| *p == "/r/sub/n.txt").count(),
                1,
                "flip {flip}: {inspects:?}"
            );
            seen.push((resources_ttl(&store), m.changes));
        }
        assert_eq!(seen[0], seen[1], "the order of the roots does not matter");
    }

    /// One run: ids are minted in byte order of the paths of all the files registered, not
    /// root by root.
    #[test]
    fn ids_follow_path_order_across_roots() {
        let mut one = empty_store();
        let mut two = empty_store();
        const KNOWN: &[(&str, u64, usize)] =
            &[("/a/2.txt", 1, 1), ("/b/1.txt", 2, 2), ("/b/3.txt", 3, 3)];
        let walked = || {
            (
                vec![file("/a/2.txt", 1)],
                vec![file("/b/1.txt", 2), file("/b/3.txt", 3)],
            )
        };
        let (a, b) = walked();
        let (r1, _, _) = run(
            &mut one,
            vec![root("/b", b), root("/a", a)],
            &[],
            Prune::Off,
            |_| None,
            inspect_in(KNOWN),
        );
        let (r2, _, _) = run(
            &mut two,
            vec![root(
                "/",
                vec![
                    file("/a/2.txt", 1),
                    file("/b/1.txt", 2),
                    file("/b/3.txt", 3),
                ],
            )],
            &[],
            Prune::Off,
            |_| None,
            inspect_in(KNOWN),
        );
        let (m1, m2) = (r1.unwrap(), r2.unwrap());
        assert_eq!(paths(&m1.value.added), ["/a/2.txt", "/b/1.txt", "/b/3.txt"]);
        assert_eq!(
            ids(&m1.value.added),
            ids(&m2.value.added),
            "the same ids as one walk of the three files in path order"
        );
        assert_eq!(resources_ttl(&one), resources_ttl(&two));
    }

    /// One run: one SHACL check over the whole mutation: every Resource here violates the
    /// extension shape, so one check lists both files, one from each root, and the store is
    /// untouched.
    #[test]
    fn one_shacl_check_across_roots() {
        let mut store = hali::storage::load(&fixture("scan/needs-description")).unwrap();
        let before = resources_ttl(&store);
        let (result, _, _) = run(
            &mut store,
            vec![
                root("/r1", vec![file("/r1/a.txt", 1)]),
                root("/r2", vec![file("/r2/b.txt", 1)]),
            ],
            &[],
            Prune::Off,
            |_| None,
            inspect_in(&[("/r1/a.txt", 1, 1), ("/r2/b.txt", 1, 2)]),
        );
        let err = result.expect_err("the extension shape is violated");
        assert_eq!(err.rule, "SHACL_VIOLATION");
        assert_eq!(err.details["total"], 2);
        assert_eq!(resources_ttl(&store), before, "the store is untouched");
    }
}
