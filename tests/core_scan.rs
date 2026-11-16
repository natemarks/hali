//! Goal tests for `scan` in `core` (#22, spec §7, §9.1 **Scan:**, §9.5).
//!
//! Assumed API (invented here; the implementer follows it).
//!
//! `hali::core`:
//!   `parse_size(&str) -> Option<u64>`: the `--max-size` / `[scan] max_size` grammar
//!       `^[0-9]+(B|KiB|MiB|GiB|TiB)?$`, suffix case-insensitive, no suffix meaning bytes,
//!       binary units only; `None` for a bad value or an overflow of `u64`.
//!   `apply` becomes `pub` (it is `pub(crate)` today): `apply(&Store, add: &[Quad], remove: &[Quad])
//!       -> Vec<Change>`, with the semantics it has now and linear time (#112).
//!
//! `hali::core::config::Config` gains `pub scan: ScanConfig` with `exclude: Vec<String>` (default
//! empty), `include_hidden: bool` (default false) and `max_size: Option<u64>` (bytes, default
//! `None`). A wrong type or a bad `max_size` value is `CONFIG_INVALID`.
//!
//! Module `hali::core::scan`:
//!   `struct ScanOptions { root: String, max_size: u64, media_types: Vec<MediaPattern> }`
//!       `root` is the real, absolute path of the Scan root, no trailing slash; `max_size` is in
//!       bytes; `media_types` are the `--media-type` patterns (ORed; empty means no filter).
//!   `enum Unhashable { Missing, NotRegular, Unreadable }`: why `inspect` could not hash a file.
//!   `struct Skipped { path: String, reason: &'static str }`, `Serialize`. `reason` is one of
//!       `too_large|unreadable|not_regular|non_utf8|outside_root|bad_ignore_file`.
//!   `struct Counts { added, refreshed, unchanged, missing, skipped: usize }`, `Serialize`, the
//!       keys in that order; always exact.
//!   `struct Scanned { counts: Counts, media_types: BTreeMap<String, usize>, added: Vec<Record>,
//!       refreshed: Vec<Record>, missing: Vec<Record>, skipped: Vec<Skipped>, truncated: bool }`
//!       `media_types` counts the added Resources by media type. The three record lists and
//!       `skipped` hold at most 100 entries each, in byte order of the path; `truncated` is true
//!       when any of them was cut. The records are as `refresh` returns them (`missing` and
//!       `stale` left false: the CLI observes them).
//!   `scan(&Context, &mut Store, &ScanOptions, entries, stat, inspect, glob_type, progress,
//!       user_name) -> Result<Mutation<Scanned>, Error>`
//!       `entries: impl IntoIterator<Item = storage::walk::Walked>`; `stat: impl Fn(&str) ->
//!       Option<(u64, i64)>` (as `refresh`); `inspect: impl FnMut(&str) -> Result<FileFacts,
//!       Unhashable>` (the path is a real path; the facts carry the size, mtime, SHA-256 and
//!       detected media type read from it); `glob_type: impl Fn(&str) -> Option<String>` (the
//!       media type the file name alone gives, a cheap pre-filter, given the final path
//!       component); `progress: impl FnMut(usize, usize)` (files seen, files hashed; the last
//!       call reports the totals); `user_name: &str`. `core` reads no file.
//!
//! What `scan` does, in order:
//!   1. Walk entries. A `Skipped` entry goes to `skipped` with its reason's name. A `File`
//!      whose path is a cataloged Resource is not new. A new `File` over `max_size`
//!      (`byte_size > max_size`) is `too_large` and never inspected; else if `glob_type(name)`
//!      is `Some` and matches no `media_types` pattern (when there are any) it is dropped
//!      silently and never inspected; else it is inspected: `Missing` drops it silently,
//!      `NotRegular` / `Unreadable` skip it; else if its facts' media type matches no pattern
//!      it is dropped silently, otherwise it is registered.
//!   2. Every cataloged Resource under the root (its recorded path is the root, or starts with
//!      `<root>/`, with no `.`, `..` or empty segment) is `stat`ed once. No regular file there:
//!      `missing`, nothing changes. Not Stale: `unchanged`, never inspected. Stale: inspected
//!      and updated exactly as `refresh` does (`refresh_one` + `touch`); an `Unhashable` one is
//!      skipped (`unreadable` / `not_regular`) and left unchanged. The size limit, `glob_type`
//!      and `media_types` do not apply to them. Resources not under the root are never `stat`ed.
//!   3. New files are sorted by path (byte order) before ids are minted, and registered with
//!      exactly the triples `add` writes (untyped, untagged).
//!   4. One mutation, one SHACL check; on an error the store is untouched. `skipped` is in path
//!      byte order.
//!
//! The tests read tests/fixtures/refresh (see tests/core_refresh.rs: R1-R5 at
//! /fixtures/refresh/{same,edited,touched,gone,sneaky}.txt) and
//! tests/fixtures/scan/needs-description (a shape every Resource violates).

mod goal {
    use hali::core::config;
    use hali::core::resource::{self, FileFacts, Target, parse_media_pattern};
    use hali::core::scan::{self, ScanOptions, Scanned, Unhashable};
    use hali::core::{Context, Error, FixedClock, Mutation, Op, SeededIdGen};
    use hali::storage::walk::{SkipReason, Walked};
    use hali::storage::{File, Store};
    use oxigraph::model::{Literal, NamedNode, Quad};
    use std::cell::RefCell;
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::{Path, PathBuf};
    use std::time::{Instant, SystemTime};

    const NOW: &str = "2026-10-24T09:30:00Z";
    const OLD: &str = "2026-10-20T14:02:11Z";
    const NEW: &str = "2026-10-22T10:00:00Z";
    const R2: &str = "0192f0c4-0000-7000-8000-000000000902";
    const R3: &str = "0192f0c4-0000-7000-8000-000000000903";
    const R4: &str = "0192f0c4-0000-7000-8000-000000000904";
    const SHA_HELLO: &str = "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03";
    const SHA_WORLD: &str = "a948904f2f0f479b8f8197694b30184b0d2ed1c1cd2a1ec0fb85d299a192a447";
    const SAME: &str = "/fixtures/refresh/same.txt";
    const EDITED: &str = "/fixtures/refresh/edited.txt";
    const TOUCHED: &str = "/fixtures/refresh/touched.txt";
    const GONE: &str = "/fixtures/refresh/gone.txt";
    const SNEAKY: &str = "/fixtures/refresh/sneaky.txt";
    const ROOT: &str = "/fixtures/refresh";

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

    fn empty_store() -> Store {
        hali::storage::load(Path::new("/nonexistent/hali-core-scan")).unwrap()
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

    fn facts(path: &str, size: u64, sha256: &str, media_type: &str) -> FileFacts {
        FileFacts {
            path: path.to_string(),
            byte_size: size,
            modified: OLD.to_string(),
            sha256: sha256.to_string(),
            media_type: media_type.to_string(),
        }
    }

    fn file(path: &str, size: u64) -> Walked {
        Walked::File {
            path: path.to_string(),
            byte_size: size,
            mtime: SystemTime::UNIX_EPOCH,
        }
    }

    fn skipped_entry(path: &str, reason: SkipReason) -> Walked {
        Walked::Skipped {
            path: path.to_string(),
            reason,
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

    /// The refresh fixture's files: what `stat` sees (see tests/core_refresh.rs).
    fn stat_of(path: &str) -> Option<(u64, i64)> {
        match path {
            SAME | SNEAKY => Some((6, 1_792_504_931)),
            EDITED => Some((12, 1_792_663_200)),
            TOUCHED => Some((6, 1_792_663_200)),
            _ => None,
        }
    }

    /// What hashing the refresh fixture's files reads.
    fn facts_of(path: &str) -> Result<FileFacts, Unhashable> {
        match path {
            EDITED => Ok(FileFacts {
                modified: NEW.to_string(),
                ..facts(path, 12, SHA_WORLD, "text/plain")
            }),
            TOUCHED => Ok(FileFacts {
                modified: NEW.to_string(),
                ..facts(path, 6, SHA_HELLO, "text/plain")
            }),
            other => Ok(facts(other, 6, SHA_HELLO, "text/plain")),
        }
    }

    fn glob_by_extension(name: &str) -> Option<String> {
        match name.rsplit_once('.')?.1 {
            "txt" => Some("text/plain".to_string()),
            "pdf" => Some("application/pdf".to_string()),
            "png" => Some("image/png".to_string()),
            _ => None,
        }
    }

    fn no_glob(_: &str) -> Option<String> {
        None
    }

    /// Calls the fakes saw: (paths `stat` was called with, paths `inspect` was called with).
    type Calls = (Vec<String>, Vec<String>);

    /// Runs `scan` with logging fakes.
    fn go(
        store: &mut Store,
        options: &ScanOptions,
        entries: Vec<Walked>,
        stat: fn(&str) -> Option<(u64, i64)>,
        inspect: fn(&str) -> Result<FileFacts, Unhashable>,
        glob: fn(&str) -> Option<String>,
    ) -> (Result<Mutation<Scanned>, Error>, Calls) {
        let stats = RefCell::new(Vec::new());
        let inspects = RefCell::new(Vec::new());
        let result = scan::scan(
            &ctx(),
            store,
            options,
            entries,
            |p: &str| {
                stats.borrow_mut().push(p.to_string());
                stat(p)
            },
            |p: &str| {
                inspects.borrow_mut().push(p.to_string());
                inspect(p)
            },
            glob,
            |_seen: usize, _hashed: usize| {},
            "tester",
        );
        (result, (stats.into_inner(), inspects.into_inner()))
    }

    fn sorted(mut list: Vec<String>) -> Vec<String> {
        list.sort();
        list
    }

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn paths(records: &[resource::Record]) -> Vec<&str> {
        records.iter().map(|r| r.path.as_str()).collect()
    }

    fn skips(scanned: &Scanned) -> Vec<(&str, &str)> {
        scanned
            .skipped
            .iter()
            .map(|s| (s.path.as_str(), s.reason))
            .collect()
    }

    fn assert_sorted(m: &Mutation<Scanned>) {
        assert!(
            m.changes.windows(2).all(|w| w[0] < w[1]),
            "changes sorted, without duplicates"
        );
    }

    // ---- --max-size grammar ------------------------------------------------------------------

    #[test]
    fn parse_size_grammar() {
        use hali::core::parse_size;
        let ok: &[(&str, u64)] = &[
            ("0", 0),
            ("1024", 1024),
            ("007", 7),
            ("1B", 1),
            ("1b", 1),
            ("1KiB", 1024),
            ("1kib", 1024),
            ("1KIB", 1024),
            ("1kIb", 1024),
            ("2MiB", 2 << 20),
            ("1GiB", 1 << 30),
            ("1gib", 1 << 30),
            ("3TiB", 3 << 40),
            ("18446744073709551615", u64::MAX),
            ("18446744073709551615B", u64::MAX),
            ("16777215TiB", 16_777_215 << 40),
            ("17179869183GiB", 17_179_869_183 << 30),
        ];
        for (text, want) in ok {
            assert_eq!(parse_size(text), Some(*want), "{text:?}");
        }
        let bad = [
            "",
            " 1",
            "1 ",
            "1 KiB",
            "KiB",
            "GiB",
            "B",
            "1KB",
            "1K",
            "1M",
            "1G",
            "1GB",
            "1MB",
            "1TB",
            "1.5GiB",
            "1.0",
            "-1",
            "+1",
            "0x10",
            "1e3",
            "1_000",
            "1,000",
            "\u{ff11}",
            "1KiBB",
            "1BKiB",
            "1 B",
            "one",
            // Overflow of u64.
            "18446744073709551616",
            "18446744073709551616B",
            "16777216TiB",
            "17179869184GiB",
            "99999999999999999999",
            "99999999999999999999GiB",
        ];
        for text in bad {
            assert_eq!(parse_size(text), None, "{text:?}");
        }
    }

    // ---- the [scan] config keys ----------------------------------------------------------------

    #[test]
    fn config_scan_keys() {
        let path = Path::new("config.toml");
        let none = config::parse("", path).ok().unwrap();
        assert!(none.scan.exclude.is_empty());
        assert!(!none.scan.include_hidden);
        assert_eq!(none.scan.max_size, None);

        let text = "[scan]\nexclude = [\"*.tmp\", \"Downloads/**\"]\ninclude_hidden = true\nmax_size = \"2MiB\"\n";
        let set = config::parse(text, path).ok().unwrap();
        assert_eq!(set.scan.exclude, strings(&["*.tmp", "Downloads/**"]));
        assert!(set.scan.include_hidden);
        assert_eq!(set.scan.max_size, Some(2 << 20));

        // The other tables are still read beside it.
        let both = config::parse("[user]\nname = \"Nate\"\n[scan]\nmax_size = \"10\"\n", path)
            .ok()
            .unwrap();
        assert_eq!(both.user_name.as_deref(), Some("Nate"));
        assert_eq!(both.scan.max_size, Some(10));

        for bad in [
            "[scan]\nexclude = \"*.tmp\"\n",
            "[scan]\nexclude = [1]\n",
            "[scan]\nexclude = [[\"a\"]]\n",
            "[scan]\ninclude_hidden = \"yes\"\n",
            "[scan]\ninclude_hidden = 1\n",
            "[scan]\nmax_size = 5\n",
            "[scan]\nmax_size = true\n",
            "[scan]\nmax_size = \"\"\n",
            "[scan]\nmax_size = \"1.5GiB\"\n",
            "[scan]\nmax_size = \"1GB\"\n",
            "[scan]\nmax_size = \"99999999999999999999GiB\"\n",
            "scan = 3\n",
        ] {
            let err = config::parse(bad, path).err().unwrap_or_else(|| {
                panic!("{bad:?} should be CONFIG_INVALID");
            });
            assert_eq!(err.rule, "CONFIG_INVALID", "{bad:?}");
        }
    }

    // ---- registering new files -----------------------------------------------------------------

    #[test]
    fn registers_untyped() {
        // Walk order is not path order: entries arrive unsorted, and ids are minted in byte
        // order of the real path ("a.txt" < "a/x.txt" because '.' < '/').
        let paths_in_order = ["/r/a.txt", "/r/a/x.txt", "/r/b.txt"];
        let entries = vec![
            file("/r/b.txt", 5),
            file("/r/a/x.txt", 7),
            file("/r/a.txt", 6),
        ];
        fn inspect(p: &str) -> Result<FileFacts, Unhashable> {
            let size = match p {
                "/r/a.txt" => 6,
                "/r/a/x.txt" => 7,
                _ => 5,
            };
            Ok(facts(p, size, &sha(size as usize), "text/plain"))
        }
        let mut store = empty_store();
        let m = scan::scan(
            &ctx(),
            &mut store,
            &opts("/r", 1 << 30, &[]),
            entries,
            |_| None,
            inspect,
            no_glob,
            |_, _| {},
            "tester",
        )
        .unwrap();

        // The oracle: `add_all` of the same files, in path order, on a fresh store.
        let mut oracle_store = empty_store();
        let files: Vec<FileFacts> = paths_in_order
            .iter()
            .map(|p| inspect(p).ok().unwrap())
            .collect();
        let oracle =
            resource::add_all(&ctx(), &mut oracle_store, &files, None, &[], &[], "tester").unwrap();

        assert_eq!(resources_ttl(&store), resources_ttl(&oracle_store));
        assert_eq!(
            hali::storage::to_turtle(&store, File::Vocabulary),
            hali::storage::to_turtle(&oracle_store, File::Vocabulary)
        );
        assert_eq!(m.changes, oracle.changes);
        assert_sorted(&m);
        let got: Vec<(&str, &str)> = m
            .value
            .added
            .iter()
            .map(|r| (r.id.as_str(), r.path.as_str()))
            .collect();
        let want: Vec<(&str, &str)> = oracle
            .value
            .added
            .iter()
            .map(|r| (r.id.as_str(), r.path.as_str()))
            .collect();
        assert_eq!(got, want);
        for r in &m.value.added {
            assert!(r.types.is_empty() && r.tags.is_empty(), "{r:?}");
        }
        let c = &m.value.counts;
        assert_eq!(
            (c.added, c.refreshed, c.unchanged, c.missing, c.skipped),
            (3, 0, 0, 0, 0)
        );
        assert_eq!(
            m.value.media_types,
            BTreeMap::from([("text/plain".to_string(), 3)])
        );
        assert!(!m.value.truncated);
        assert!(m.value.refreshed.is_empty() && m.value.missing.is_empty());
    }

    #[test]
    fn size_filter() {
        let entries = vec![
            file("/r/ten.txt", 10),
            file("/r/eleven.txt", 11),
            file("/r/twelve.txt", 12),
            file("/r/huge.txt", u64::MAX),
        ];
        let mut store = empty_store();
        let (result, (stats, inspects)) = go(
            &mut store,
            &opts("/r", 11, &[]),
            entries,
            |_| None,
            |p| Ok(facts(p, 1, &sha(1), "text/plain")),
            no_glob,
        );
        let m = result.unwrap();
        // `byte_size > max_size` is too large; the two over it are never opened.
        assert_eq!(sorted(inspects), strings(&["/r/eleven.txt", "/r/ten.txt"]));
        assert!(
            stats.is_empty(),
            "stat is for cataloged Resources: {stats:?}"
        );
        assert_eq!(
            skips(&m.value),
            vec![("/r/huge.txt", "too_large"), ("/r/twelve.txt", "too_large")]
        );
        assert_eq!(paths(&m.value.added), vec!["/r/eleven.txt", "/r/ten.txt"]);
        assert_eq!((m.value.counts.added, m.value.counts.skipped), (2, 2));

        // A max_size of 0 skips every non-empty file and keeps an empty one.
        let mut store = empty_store();
        let (result, (_, inspects)) = go(
            &mut store,
            &opts("/r", 0, &[]),
            vec![file("/r/empty", 0), file("/r/one", 1)],
            |_| None,
            |p| Ok(facts(p, 0, &sha(2), "application/octet-stream")),
            no_glob,
        );
        let m = result.unwrap();
        assert_eq!(inspects, strings(&["/r/empty"]));
        assert_eq!(skips(&m.value), vec![("/r/one", "too_large")]);
        assert_eq!(paths(&m.value.added), vec!["/r/empty"]);
    }

    #[test]
    fn media_type_filter() {
        let entries = || {
            vec![
                file("/r/a.txt", 1),
                file("/r/b.pdf", 1),
                file("/r/c.png", 1),
                file("/r/noext", 1),
                file("/r/d.bin", 1),
            ]
        };
        let inspect = |p: &str| -> Result<FileFacts, Unhashable> {
            let media = match p {
                "/r/a.txt" | "/r/d.bin" => "text/plain",
                "/r/b.pdf" => "application/pdf",
                "/r/c.png" => "image/png",
                _ => "application/octet-stream",
            };
            Ok(facts(p, 1, &sha(p.len()), media))
        };

        // One wildcard pattern. The file-name glob alone rules out b.pdf and c.png: they are not
        // inspected. noext and d.bin have no glob type, so they are inspected, and noext's
        // detected type matches nothing: dropped, not reported.
        let mut store = empty_store();
        let (result, (_, inspects)) = go(
            &mut store,
            &opts("/r", 1 << 30, &["text/*"]),
            entries(),
            |_| None,
            inspect,
            glob_by_extension,
        );
        let m = result.unwrap();
        assert_eq!(
            sorted(inspects),
            strings(&["/r/a.txt", "/r/d.bin", "/r/noext"])
        );
        assert_eq!(paths(&m.value.added), vec!["/r/a.txt", "/r/d.bin"]);
        assert!(m.value.skipped.is_empty(), "{:?}", skips(&m.value));
        assert_eq!((m.value.counts.added, m.value.counts.skipped), (2, 0));
        assert_eq!(
            m.value.media_types,
            BTreeMap::from([("text/plain".to_string(), 2)])
        );

        // Two patterns, ORed, case-insensitive, an exact one and a wildcard.
        let mut store = empty_store();
        let (result, (_, inspects)) = go(
            &mut store,
            &opts("/r", 1 << 30, &["application/pdf", "IMAGE/*"]),
            entries(),
            |_| None,
            inspect,
            glob_by_extension,
        );
        let m = result.unwrap();
        assert_eq!(
            sorted(inspects),
            strings(&["/r/b.pdf", "/r/c.png", "/r/d.bin", "/r/noext"])
        );
        assert_eq!(paths(&m.value.added), vec!["/r/b.pdf", "/r/c.png"]);
        assert_eq!(
            m.value.media_types,
            BTreeMap::from([
                ("application/pdf".to_string(), 1),
                ("image/png".to_string(), 1)
            ])
        );

        // No pattern: everything is registered and the glob is not needed.
        let mut store = empty_store();
        let (result, (_, inspects)) = go(
            &mut store,
            &opts("/r", 1 << 30, &[]),
            entries(),
            |_| None,
            inspect,
            glob_by_extension,
        );
        let m = result.unwrap();
        assert_eq!(inspects.len(), 5);
        assert_eq!(m.value.counts.added, 5);
        assert_eq!(
            m.value.media_types,
            BTreeMap::from([
                ("application/octet-stream".to_string(), 1),
                ("application/pdf".to_string(), 1),
                ("image/png".to_string(), 1),
                ("text/plain".to_string(), 2)
            ])
        );
    }

    #[test]
    fn new_file_problems_never_abort() {
        // A file the walk saw that vanished before it was opened is dropped silently; one that
        // turned non-regular or cannot be read is skipped; the rest is registered.
        let entries = vec![
            file("/r/ok.txt", 1),
            file("/r/vanished.txt", 1),
            file("/r/fifo", 1),
            file("/r/locked.txt", 1),
        ];
        let mut store = empty_store();
        let (result, _) = go(
            &mut store,
            &opts("/r", 1 << 30, &[]),
            entries,
            |_| None,
            |p| match p {
                "/r/vanished.txt" => Err(Unhashable::Missing),
                "/r/fifo" => Err(Unhashable::NotRegular),
                "/r/locked.txt" => Err(Unhashable::Unreadable),
                other => Ok(facts(other, 1, &sha(1), "text/plain")),
            },
            no_glob,
        );
        let m = result.unwrap();
        assert_eq!(paths(&m.value.added), vec!["/r/ok.txt"]);
        assert_eq!(
            skips(&m.value),
            vec![("/r/fifo", "not_regular"), ("/r/locked.txt", "unreadable")]
        );
        assert_eq!((m.value.counts.added, m.value.counts.skipped), (1, 2));
    }

    #[test]
    fn walk_skips_pass_through_in_path_order() {
        let entries = vec![
            skipped_entry("/r/z-link", SkipReason::OutsideRoot),
            skipped_entry("/r/\u{fffd}bad.txt", SkipReason::NonUtf8),
            skipped_entry("/r/locked", SkipReason::Unreadable),
            skipped_entry("/r/fifo", SkipReason::NotRegular),
            skipped_entry("/r/.ignore", SkipReason::BadIgnoreFile),
            skipped_entry("/r/big.bin", SkipReason::TooLarge),
            file("/r/m.txt", 1),
            file("/r/a-big.bin", 9),
        ];
        let mut store = empty_store();
        let (result, _) = go(
            &mut store,
            &opts("/r", 5, &[]),
            entries,
            |_| None,
            |p| Ok(facts(p, 1, &sha(1), "text/plain")),
            no_glob,
        );
        let m = result.unwrap();
        assert_eq!(
            skips(&m.value),
            vec![
                ("/r/.ignore", "bad_ignore_file"),
                ("/r/a-big.bin", "too_large"),
                ("/r/big.bin", "too_large"),
                ("/r/fifo", "not_regular"),
                ("/r/locked", "unreadable"),
                ("/r/z-link", "outside_root"),
                ("/r/\u{fffd}bad.txt", "non_utf8"),
            ]
        );
        assert_eq!(m.value.counts.skipped, 7);
        assert_eq!(paths(&m.value.added), vec!["/r/m.txt"]);
    }

    #[test]
    fn progress_totals() {
        let calls = RefCell::new(Vec::new());
        let mut store = empty_store();
        let m = scan::scan(
            &ctx(),
            &mut store,
            &opts("/r", 1 << 30, &[]),
            vec![
                file("/r/a.txt", 1),
                file("/r/b.txt", 1),
                file("/r/c.txt", 1),
            ],
            |_| None,
            |p: &str| Ok(facts(p, 1, &sha(1), "text/plain")),
            no_glob,
            |seen: usize, hashed: usize| calls.borrow_mut().push((seen, hashed)),
            "tester",
        )
        .unwrap();
        assert_eq!(m.value.counts.added, 3);
        let calls = calls.into_inner();
        assert_eq!(calls.last(), Some(&(3, 3)), "{calls:?}");
        assert!(
            calls
                .windows(2)
                .all(|w| w[0].0 <= w[1].0 && w[0].1 <= w[1].1),
            "counts never go down: {calls:?}"
        );
    }

    // ---- cataloged Resources under the root ----------------------------------------------------

    /// `facts_of`, but `new.txt` has a content of its own (size 3), so it matches no Missing
    /// Resource of the fixture (`gone.txt` has the size and hash of `hello\n`) and is not a move.
    fn facts_with_new(path: &str) -> Result<FileFacts, Unhashable> {
        if path.ends_with("new.txt") {
            Ok(facts(path, 3, &sha(3), "text/plain"))
        } else {
            facts_of(path)
        }
    }

    #[test]
    fn refreshes_stale_under_root_only() {
        let mut store = hali::storage::load(&fixture("refresh")).unwrap();
        let before = triples(&resources_ttl(&store));
        // The walk also yields cataloged files (they are not new) and one new file.
        let entries = vec![
            file(SAME, 6),
            file(EDITED, 12),
            file(TOUCHED, 6),
            file("/fixtures/refresh/new.txt", 3),
        ];
        let (result, (stats, inspects)) = go(
            &mut store,
            &opts(ROOT, 1 << 30, &[]),
            entries,
            stat_of,
            facts_with_new,
            no_glob,
        );
        let m = result.unwrap();

        // Every cataloged Resource under the root is stat'ed once; only the Stale ones and the
        // new file are opened (sneaky.txt has the recorded size and mtime: not Stale).
        let cataloged: Vec<String> = stats
            .iter()
            .filter(|p| !p.ends_with("new.txt"))
            .cloned()
            .collect();
        assert_eq!(
            sorted(cataloged),
            strings(&[EDITED, GONE, SAME, SNEAKY, TOUCHED])
        );
        assert_eq!(
            sorted(inspects),
            strings(&[EDITED, "/fixtures/refresh/new.txt", TOUCHED])
        );

        let v = &m.value;
        assert_eq!(
            v.refreshed
                .iter()
                .map(|r| r.id.as_str())
                .collect::<Vec<_>>(),
            vec![R2, R3]
        );
        assert_eq!(v.refreshed[0].byte_size, 12);
        assert_eq!(v.refreshed[0].modified, NEW);
        assert_eq!(
            v.missing.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            vec![R4]
        );
        assert_eq!(paths(&v.added), vec!["/fixtures/refresh/new.txt"]);
        let c = &v.counts;
        assert_eq!(
            (c.added, c.refreshed, c.unchanged, c.missing, c.skipped),
            (1, 2, 2, 1, 0)
        );

        // The oracle: `refresh --all` then `add`, with the same fakes.
        let mut oracle = hali::storage::load(&fixture("refresh")).unwrap();
        let refreshed = resource::refresh(&ctx(), &mut oracle, &Target::All, stat_of, |p| {
            let f = facts_of(p).ok().unwrap();
            Ok(resource::Seen::Hashed(resource::Hashed {
                byte_size: f.byte_size,
                modified: f.modified,
                sha256: f.sha256,
            }))
        })
        .unwrap();
        let added = resource::add(
            &ctx(),
            &mut oracle,
            &facts_with_new("/fixtures/refresh/new.txt").ok().unwrap(),
            None,
            "tester",
        )
        .unwrap();
        let mut want_changes: Vec<_> = refreshed.changes.into_iter().chain(added.changes).collect();
        want_changes.sort();
        want_changes.dedup();
        assert_eq!(m.changes, want_changes);
        assert_eq!(
            triples(&resources_ttl(&store)),
            triples(&resources_ttl(&oracle))
        );
        assert_ne!(triples(&resources_ttl(&store)), before);
        assert_sorted(&m);
    }

    #[test]
    fn filters_gate_only_new_registrations() {
        // max_size 0 and a media pattern nothing here matches: they rule out every new file but
        // never un-catalog or skip a cataloged one.
        let mut store = hali::storage::load(&fixture("refresh")).unwrap();
        let (result, (_, inspects)) = go(
            &mut store,
            &opts(ROOT, 0, &["application/pdf"]),
            vec![file(EDITED, 12), file("/fixtures/refresh/new.txt", 3)],
            stat_of,
            facts_of,
            glob_by_extension,
        );
        let m = result.unwrap();
        assert_eq!(sorted(inspects), strings(&[EDITED, TOUCHED]));
        assert_eq!(
            m.value
                .refreshed
                .iter()
                .map(|r| r.id.as_str())
                .collect::<Vec<_>>(),
            vec![R2, R3]
        );
        assert!(m.value.added.is_empty());
        assert_eq!(
            skips(&m.value),
            vec![("/fixtures/refresh/new.txt", "too_large")]
        );
        assert_eq!(m.value.counts.missing, 1);
    }

    #[test]
    fn unhashable_cataloged_file_is_skipped_and_left_alone() {
        let mut store = hali::storage::load(&fixture("refresh")).unwrap();
        let before = resources_ttl(&store);
        let (result, _) = go(
            &mut store,
            &opts(ROOT, 1 << 30, &[]),
            vec![],
            stat_of,
            |p| match p {
                EDITED => Err(Unhashable::Unreadable),
                TOUCHED => Err(Unhashable::NotRegular),
                other => facts_of(other),
            },
            no_glob,
        );
        let m = result.unwrap();
        assert_eq!(
            skips(&m.value),
            vec![(EDITED, "unreadable"), (TOUCHED, "not_regular")]
        );
        assert!(m.value.refreshed.is_empty() && m.value.added.is_empty());
        assert!(m.changes.is_empty());
        assert_eq!(resources_ttl(&store), before);
        assert_eq!(m.value.counts.skipped, 2);
    }

    #[test]
    fn vanished_after_stat_counts_as_missing() {
        // `stat` sees EDITED (Stale) but `inspect` finds it gone: missing, not unchanged.
        let mut store = hali::storage::load(&fixture("refresh")).unwrap();
        let before = resources_ttl(&store);
        let (result, _) = go(
            &mut store,
            &opts(ROOT, 1 << 30, &[]),
            vec![],
            stat_of,
            |p| match p {
                EDITED => Err(Unhashable::Missing),
                other => facts_of(other),
            },
            no_glob,
        );
        let m = result.unwrap();
        assert!(
            paths(&m.value.missing).contains(&EDITED),
            "{:?}",
            m.value.missing
        );
        assert!(!paths(&m.value.refreshed).contains(&EDITED));
        // GONE is missing by `stat`; EDITED joins it; TOUCHED is refreshed; SAME and SNEAKY
        // are unchanged.
        assert_eq!(m.value.counts.missing, 2);
        assert_eq!(m.value.counts.unchanged, 2);
        assert_eq!(m.value.counts.refreshed, 1);
        assert!(m.value.skipped.is_empty());
        assert_eq!(
            resources_ttl(&store).contains("edited"),
            before.contains("edited")
        );
        assert!(
            m.changes.iter().all(|c| !c.triple.contains("edited.txt")),
            "the vanished file's record is left alone"
        );
    }

    #[test]
    fn stale_refreshed_list_is_capped() {
        // 101 Stale cataloged Resources: counts exact, 100 listed, `truncated`.
        let mut store = empty_store();
        let cataloged: Vec<FileFacts> = names("/c", 101)
            .iter()
            .enumerate()
            .map(|(i, p)| facts(p, 1, &sha(i + 1), "text/plain"))
            .collect();
        resource::add_all(&ctx(), &mut store, &cataloged, None, &[], &[], "tester").unwrap();
        let (result, _) = go(
            &mut store,
            &opts("/c", 1 << 30, &[]),
            vec![],
            |_| Some((2, 1_792_504_931)),
            |p| {
                Ok(FileFacts {
                    modified: NEW.to_string(),
                    ..facts(p, 2, &sha(1000), "text/plain")
                })
            },
            no_glob,
        );
        let m = result.unwrap();
        assert_eq!(m.value.counts.refreshed, 101);
        assert_eq!(m.value.counts.unchanged, 0);
        assert_eq!(m.value.refreshed.len(), 100);
        assert_eq!(m.value.refreshed[0].path, "/c/f000.txt");
        assert_eq!(m.value.refreshed[99].path, "/c/f099.txt");
        assert!(m.value.truncated);
    }

    #[test]
    fn missing_and_prefix_rule() {
        // Resources whose recorded paths are, for the root `/r`: the root itself and files
        // under it (under the root); and look-alikes that are not (never stat'ed).
        let under = ["/r", "/r/ok.txt", "/r/sub/deep.txt", "/r/gone.txt"];
        let not_under = [
            "/rx/other.txt",
            "/r2.txt",
            "/q/r/nested.txt",
            "/r/../r/dotdot.txt",
            "/r/./dot.txt",
            "/r//empty.txt",
        ];
        let mut store = empty_store();
        let all: Vec<FileFacts> = under
            .iter()
            .chain(not_under.iter())
            .enumerate()
            .map(|(i, p)| facts(p, 6, &sha(i + 1), "text/plain"))
            .collect();
        resource::add_all(&ctx(), &mut store, &all, None, &[], &[], "tester").unwrap();
        let before = resources_ttl(&store);

        let seen = RefCell::new(Vec::new());
        let inspected = RefCell::new(Vec::new());
        let m = scan::scan(
            &ctx(),
            &mut store,
            &opts("/r", 1 << 30, &[]),
            Vec::<Walked>::new(),
            |p: &str| {
                seen.borrow_mut().push(p.to_string());
                // Every file is there, as recorded (not Stale), except gone.txt.
                (p != "/r/gone.txt").then_some((6, 1_792_504_931))
            },
            |p: &str| {
                inspected.borrow_mut().push(p.to_string());
                Err(Unhashable::Missing)
            },
            no_glob,
            |_, _| {},
            "tester",
        )
        .unwrap();

        assert_eq!(sorted(seen.into_inner()), sorted(strings(&under)));
        assert!(
            inspected.into_inner().is_empty(),
            "nothing is Stale, so nothing is opened"
        );
        let v = &m.value;
        assert_eq!(paths(&v.missing), vec!["/r/gone.txt"]);
        assert_eq!(v.counts.missing, 1);
        assert_eq!(v.counts.unchanged, 3);
        assert_eq!(v.counts.added + v.counts.refreshed + v.counts.skipped, 0);
        assert!(m.changes.is_empty(), "a missing Resource is never changed");
        assert_eq!(resources_ttl(&store), before);
    }

    // ---- atomicity -----------------------------------------------------------------------------

    #[test]
    fn one_shacl_check() {
        // Every Resource here violates the extension shape. A per-file check would stop at the
        // first file and list one violation; one check on the whole mutation lists both.
        let mut store = hali::storage::load(&fixture("scan/needs-description")).unwrap();
        let before = resources_ttl(&store);
        let (result, _) = go(
            &mut store,
            &opts("/r", 1 << 30, &[]),
            vec![file("/r/a.txt", 1), file("/r/b.txt", 1)],
            |_| None,
            |p| Ok(facts(p, 1, &sha(p.len()), "text/plain")),
            no_glob,
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
        assert_eq!(focus.len(), 2, "both files are listed: {focus:?}");
        assert_eq!(resources_ttl(&store), before, "the store is untouched");
    }

    #[test]
    fn nothing_to_do() {
        let mut store = hali::storage::load(&fixture("refresh")).unwrap();
        let before = resources_ttl(&store);
        let (result, (stats, inspects)) = go(
            &mut store,
            &opts("/nowhere", 1 << 30, &[]),
            vec![],
            stat_of,
            facts_of,
            no_glob,
        );
        let m = result.unwrap();
        assert!(stats.is_empty() && inspects.is_empty());
        assert!(m.changes.is_empty());
        assert_eq!(resources_ttl(&store), before);
        let c = &m.value.counts;
        assert_eq!(
            (c.added, c.refreshed, c.unchanged, c.missing, c.skipped),
            (0, 0, 0, 0, 0)
        );
        assert!(m.value.media_types.is_empty());
        assert!(!m.value.truncated);
    }

    // ---- caps ----------------------------------------------------------------------------------

    fn names(prefix: &str, n: usize) -> Vec<String> {
        (0..n).map(|i| format!("{prefix}/f{i:03}.txt")).collect()
    }

    #[test]
    fn caps_and_truncated() {
        // 101 new files: counts are exact, the list is cut at 100, `truncated` is set.
        let new = names("/n", 101);
        let mut store = empty_store();
        let (result, _) = go(
            &mut store,
            &opts("/n", 1 << 30, &[]),
            new.iter().rev().map(|p| file(p, 1)).collect(),
            |_| None,
            |p| Ok(facts(p, 1, &sha(p.len()), "text/plain")),
            no_glob,
        );
        let m = result.unwrap();
        assert_eq!(m.value.counts.added, 101);
        assert_eq!(m.value.added.len(), 100);
        assert_eq!(m.value.added[0].path, "/n/f000.txt");
        assert_eq!(m.value.added[99].path, "/n/f099.txt");
        assert!(m.value.truncated);
        assert_eq!(
            m.value.media_types,
            BTreeMap::from([("text/plain".to_string(), 101)])
        );
        // All 101 are registered, whatever is listed.
        let resource_type = m
            .changes
            .iter()
            .filter(|c| {
                c.op == Op::Add
                    && c.triple.ends_with(
                        "<http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <https://w3id.org/hali#Resource> .",
                    )
            })
            .count();
        assert_eq!(resource_type, 101);

        // 101 skipped.
        let mut store = empty_store();
        let entries = names("/s", 101)
            .iter()
            .map(|p| skipped_entry(p, SkipReason::NotRegular))
            .collect();
        let (result, _) = go(
            &mut store,
            &opts("/s", 1 << 30, &[]),
            entries,
            |_| None,
            |p| Ok(facts(p, 1, &sha(1), "text/plain")),
            no_glob,
        );
        let m = result.unwrap();
        assert_eq!(m.value.counts.skipped, 101);
        assert_eq!(m.value.skipped.len(), 100);
        assert!(m.value.truncated);
        assert!(m.changes.is_empty());

        // Exactly 100 skipped: not truncated.
        let mut store = empty_store();
        let entries = names("/s", 100)
            .iter()
            .map(|p| skipped_entry(p, SkipReason::NotRegular))
            .collect();
        let (result, _) = go(
            &mut store,
            &opts("/s", 1 << 30, &[]),
            entries,
            |_| None,
            |p| Ok(facts(p, 1, &sha(1), "text/plain")),
            no_glob,
        );
        let m = result.unwrap();
        assert_eq!(m.value.skipped.len(), 100);
        assert!(!m.value.truncated);

        // 101 missing.
        let mut store = empty_store();
        let cataloged: Vec<FileFacts> = names("/m", 101)
            .iter()
            .enumerate()
            .map(|(i, p)| facts(p, 1, &sha(i + 1), "text/plain"))
            .collect();
        resource::add_all(&ctx(), &mut store, &cataloged, None, &[], &[], "tester").unwrap();
        let before = resources_ttl(&store);
        let (result, _) = go(
            &mut store,
            &opts("/m", 1 << 30, &[]),
            vec![],
            |_| None,
            |p| Ok(facts(p, 1, &sha(1), "text/plain")),
            no_glob,
        );
        let m = result.unwrap();
        assert_eq!(m.value.counts.missing, 101);
        assert_eq!(m.value.missing.len(), 100);
        assert!(m.value.truncated);
        assert_eq!(resources_ttl(&store), before);
    }

    // ---- apply is linear (#112) ----------------------------------------------------------------

    fn quad(i: usize) -> Quad {
        Quad::new(
            NamedNode::new(format!("urn:uuid:00000000-0000-7000-8000-{i:012}")).unwrap(),
            NamedNode::new("http://example.org/p").unwrap(),
            Literal::new_simple_literal(format!("v{i}")),
            File::Resources.graph(),
        )
    }

    fn line(i: usize) -> String {
        format!("<urn:uuid:00000000-0000-7000-8000-{i:012}> <http://example.org/p> \"v{i}\" .")
    }

    #[test]
    fn apply_is_linear() {
        const N: usize = 20_000;
        let store = Store::new().unwrap();
        // Held: 0..N (to be removed), N+40_000 (x) and N+40_002 (z).
        let x = 40_000;
        let y = 40_001;
        let z = 40_002;
        let w = 40_003;
        for i in (0..N).chain([x, z]) {
            store.insert(&quad(i)).unwrap();
        }
        // add: N..2N, x (also in `remove`, held), y twice (not held), z (held, not removed).
        let mut add: Vec<Quad> = (N..2 * N).map(quad).collect();
        add.extend([quad(x), quad(y), quad(y), quad(z)]);
        // remove: 0..N, x (put back by `add`), w (not held).
        let mut remove: Vec<Quad> = (0..N).map(quad).collect();
        remove.extend([quad(x), quad(w)]);

        let started = Instant::now();
        let changes = hali::core::apply(&store, &add, &remove);
        let elapsed = started.elapsed();
        assert!(
            elapsed.as_secs_f64() < 2.0,
            "apply of 20k adds and 20k removes took {elapsed:?}"
        );

        let mut adds: Vec<String> = (N..2 * N).map(line).collect();
        adds.push(line(y));
        adds.sort();
        let mut removes: Vec<String> = (0..N).map(line).collect();
        removes.sort();
        let want: Vec<(Op, String)> = adds
            .into_iter()
            .map(|t| (Op::Add, t))
            .chain(removes.into_iter().map(|t| (Op::Remove, t)))
            .collect();
        let got: Vec<(Op, String)> = changes.iter().map(|c| (c.op, c.triple.clone())).collect();
        assert_eq!(got.len(), want.len());
        assert!(got == want, "changes differ from the expected sorted list");
        assert_eq!(store.len().unwrap(), N + 3, "20k added, x, y and z held");
        assert!(!store.contains(&quad(0)).unwrap());
        assert!(store.contains(&quad(x)).unwrap());
        assert!(store.contains(&quad(y)).unwrap());
    }

    // ---- moves and --prune (#24, spec §9.1 **Scan:**) -----------------------------------------

    /// Goal tests for moves and pruning during a scan (#24).
    ///
    /// Assumed API (D14 of the approved plan; the implementer follows it).
    ///
    /// Module `hali::core::scan`:
    ///   `pub enum Prune { Off, On, Forced }`: `Off` forgets nothing; `On` forgets what `forget`
    ///       would forget; `Forced` is `--prune --force`.
    ///   `scan_with(&Context, &mut Store, &ScanOptions, Prune, entries, stat, inspect, glob_type,
    ///       progress, user_name) -> Result<Mutation<Scanned>, Error>`: `scan` with `Prune`
    ///       before `entries`; `scan(...)` keeps its signature and is `scan_with(.., Prune::Off,
    ///       ..)`.
    ///   `Counts` gains `relinked`, `ambiguous` and `pruned`: `added, refreshed, relinked,
    ///       ambiguous, unchanged, missing, pruned, skipped`, always exact.
    ///   `Scanned` gains `relinked: Vec<relink::Relinked>`, `ambiguous: Vec<relink::Ambiguous>`
    ///       and `pruned: Vec<Record>`: `counts, media_types, added, refreshed, relinked,
    ///       ambiguous, missing, pruned, skipped, truncated`. Every list is in byte order of the
    ///       record's path (the new path for a relinked one), at most 100 entries; `candidates`
    ///       at most 10, in byte order; `truncated` covers every list. `missing` is the
    ///       Resources under the root still cataloged and Missing after the run (ambiguous ones
    ///       and ones `--prune` kept included). `pruned` holds the records as they were.
    ///
    /// What `scan_with` does beyond `scan`:
    ///   - A new file that would be registered (not too large, not ruled out by the media type,
    ///     hashed, non-empty) and has the byte size and SHA-256 of exactly one Missing Resource
    ///     (store-wide, outside the root too), with no other Missing Resource having its content,
    ///     is a move: the Resource's `dcat:downloadURL` and `dcterms:modified` are replaced as
    ///     `relink::explicit` replaces them and its catalog record's `dcterms:modified` is set to
    ///     now; the file is not registered and takes no id. Resources outside the root are
    ///     `stat`ed (once) only when a new file has their size and SHA-256.
    ///   - Otherwise (several candidates, or several Missing Resources with that content) the
    ///     Resource is ambiguous: nothing changes, its candidate files are not registered and it
    ///     is never pruned.
    ///   - An empty file and a Missing Resource recorded empty never match.
    ///   - `Prune::On`: every Resource under the root still Missing after the moves and not
    ///     ambiguous is forgotten as `forget` forgets it, except one with an inbound relation
    ///     (from any Resource, in any graph, a pruned one included), which is kept (it stays in
    ///     `missing`). `Prune::Forced` forgets those too and removes their inbound triples from
    ///     resources.ttl; an inbound relation held only in extensions.ttl is `SHACL_VIOLATION`.
    ///     No file is opened for it.
    ///   - Moves, refreshes, prunes and adds are one mutation with one SHACL check.
    #[cfg(feature = "goal")]
    mod moves_and_prune {
        use super::*;
        use hali::core::relink;
        use hali::core::scan::Prune;

        /// The mtime (seconds) that matches `OLD`.
        const PINNED: i64 = 1_792_504_931;
        const SETUP_NOW: &str = "2026-10-21T08:00:00Z";
        const F1: &str = "0192f0c4-0000-7000-8000-000000000201";
        const F2: &str = "0192f0c4-0000-7000-8000-000000000202";
        const F3: &str = "0192f0c4-0000-7000-8000-000000000203";
        const RELATIONS: &str = "/fixtures/relations";

        /// A context for building the store a scan starts from: other ids (a scan mints from
        /// seed 1) and another clock (a relink touches a catalog record with the scan's).
        fn setup_ctx() -> Context {
            Context::new(
                "en",
                Box::new(FixedClock::new(SETUP_NOW)),
                Box::new(SeededIdGen::new(100)),
            )
        }

        /// Catalogs these files; returns their records, in order.
        fn catalog_facts(store: &mut Store, files: &[FileFacts]) -> Vec<resource::Record> {
            resource::add_all(&setup_ctx(), store, files, None, &[], &[], "tester")
                .unwrap()
                .value
                .added
        }

        /// Catalogs `(path, byte size, sha(i))` text files, mtime `OLD`.
        fn catalog(store: &mut Store, items: &[(&str, u64, usize)]) -> Vec<resource::Record> {
            let files: Vec<FileFacts> = items
                .iter()
                .map(|(p, size, i)| facts(p, *size, &sha(*i), "text/plain"))
                .collect();
            catalog_facts(store, &files)
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

        /// Runs `scan_with` with logging fakes.
        fn run(
            store: &mut Store,
            options: &ScanOptions,
            prune: Prune,
            entries: Vec<Walked>,
            stat: impl Fn(&str) -> Option<(u64, i64)>,
            inspect: impl Fn(&str) -> Result<FileFacts, Unhashable>,
            glob: impl Fn(&str) -> Option<String>,
        ) -> (Result<Mutation<Scanned>, Error>, Calls) {
            let stats = RefCell::new(Vec::new());
            let inspects = RefCell::new(Vec::new());
            let result = scan::scan_with(
                &ctx(),
                store,
                options,
                prune,
                entries,
                |p: &str| {
                    stats.borrow_mut().push(p.to_string());
                    stat(p)
                },
                |p: &str| {
                    inspects.borrow_mut().push(p.to_string());
                    inspect(p)
                },
                glob,
                |_: usize, _: usize| {},
                "tester",
            );
            (result, (stats.into_inner(), inspects.into_inner()))
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

        fn ids(records: &[resource::Record]) -> Vec<&str> {
            records.iter().map(|r| r.id.as_str()).collect()
        }

        fn moves(scanned: &Scanned) -> Vec<(&str, &str)> {
            scanned
                .relinked
                .iter()
                .map(|r| (r.from.as_str(), r.resource.path.as_str()))
                .collect()
        }

        fn moved_facts(path: &str, size: u64, sha_index: usize) -> FileFacts {
            FileFacts {
                modified: NEW.to_string(),
                ..facts(path, size, &sha(sha_index), "text/plain")
            }
        }

        /// Row 1: A one-to-one move is relinked exactly as `relink::explicit` writes it.
        #[test]
        fn relinks_a_one_to_one_move() {
            let build = || {
                let mut store = empty_store();
                let records = catalog(&mut store, &[("/r/old/a.txt", 5, 5), ("/r/keep.txt", 6, 6)]);
                (store, records)
            };
            let (mut store, records) = build();
            let a = records[0].id.clone();
            let (result, (stats, inspects)) = run(
                &mut store,
                &opts("/r", 1 << 30, &[]),
                Prune::Off,
                vec![file("/r/keep.txt", 6), file("/r/new/a.txt", 5)],
                stat_in(&[("/r/keep.txt", 6)]),
                |p| match p {
                    "/r/new/a.txt" => Ok(moved_facts(p, 5, 5)),
                    _ => Err(Unhashable::Missing),
                },
                no_glob,
            );
            let m = result.unwrap();

            // The oracle: `relink::explicit` of the same file on an identical store.
            let (mut oracle_store, _) = build();
            let oracle = relink::explicit(
                &ctx(),
                &mut oracle_store,
                &a,
                "/r/new/a.txt",
                &resource::Hashed {
                    byte_size: 5,
                    modified: NEW.to_string(),
                    sha256: sha(5),
                },
                false,
            )
            .unwrap();
            assert_eq!(resources_ttl(&store), resources_ttl(&oracle_store));
            assert_eq!(m.changes, oracle.changes);
            assert!(!m.changes.is_empty());
            assert_sorted(&m);

            let v = &m.value;
            assert_eq!(moves(v), vec![("/r/old/a.txt", "/r/new/a.txt")]);
            assert_eq!(v.relinked[0].resource.id, a);
            assert_eq!(v.relinked[0].resource.modified, NEW);
            assert!(v.added.is_empty() && v.ambiguous.is_empty() && v.pruned.is_empty());
            assert!(v.missing.is_empty(), "a relinked Resource is not missing");
            assert!(v.media_types.is_empty(), "only added Resources are counted");
            assert_eq!(tally(&v.counts), [0, 0, 1, 0, 1, 0, 0, 0]);
            // The file was hashed (it is a candidate) and registered nowhere; every Resource
            // under the root is `stat`ed once, the missing one not again for the match.
            assert_eq!(inspects, strings(&["/r/new/a.txt"]));
            assert_eq!(sorted(stats), strings(&["/r/keep.txt", "/r/old/a.txt"]));
            assert!(!resources_ttl(&store).contains("file:///r/old/a.txt"));
        }

        /// Row 2: A Missing Resource outside the root is matched too, and Resources outside the
        /// root are `stat`ed only when a new file has their size and SHA-256.
        #[test]
        fn matches_missing_resources_outside_the_root() {
            let mut store = empty_store();
            let records = catalog(
                &mut store,
                &[
                    ("/o/x.txt", 5, 5), // Missing; a new file has its content
                    ("/o/y.txt", 6, 6), // Missing; no new file has its content
                    ("/o/z.txt", 7, 7), // present; a new file has its content
                    ("/r/w.txt", 8, 8), // under the root, present
                ],
            );
            let x = records[0].id.clone();
            let (result, (stats, inspects)) = run(
                &mut store,
                &opts("/r", 1 << 30, &[]),
                Prune::Off,
                vec![
                    file("/r/w.txt", 8),
                    file("/r/n1.txt", 5),
                    file("/r/n2.txt", 7),
                    file("/r/n3.txt", 9),
                ],
                stat_in(&[("/o/z.txt", 7), ("/r/w.txt", 8)]),
                |p| match p {
                    "/r/n1.txt" => Ok(moved_facts(p, 5, 5)),
                    "/r/n2.txt" => Ok(facts(p, 7, &sha(7), "text/plain")),
                    _ => Ok(facts(p, 9, &sha(9), "text/plain")),
                },
                no_glob,
            );
            let m = result.unwrap();
            let v = &m.value;
            assert_eq!(moves(v), vec![("/o/x.txt", "/r/n1.txt")]);
            assert_eq!(v.relinked[0].resource.id, x);
            // The present Resource's content is no move: the copy is registered as usual.
            assert_eq!(paths(&v.added), vec!["/r/n2.txt", "/r/n3.txt"]);
            assert!(
                v.missing.is_empty(),
                "/o/y.txt is outside the root: not listed"
            );
            assert_eq!(tally(&v.counts), [2, 0, 1, 0, 1, 0, 0, 0]);
            assert_eq!(
                sorted(inspects),
                strings(&["/r/n1.txt", "/r/n2.txt", "/r/n3.txt"])
            );
            // Outside the root only x (Missing, matched) and z (a new file has its content) are
            // `stat`ed, once each; y never is.
            assert_eq!(
                sorted(stats),
                strings(&["/o/x.txt", "/o/z.txt", "/r/w.txt"])
            );
            let ttl = resources_ttl(&store);
            assert!(ttl.contains("file:///r/n1.txt") && !ttl.contains("file:///o/x.txt"));
            assert!(ttl.contains("file:///o/y.txt"), "y is untouched");
            assert_sorted(&m);
        }

        /// Row 3: Several matches: nothing changes, no candidate is registered, nothing is pruned.
        #[test]
        fn several_candidates_change_nothing() {
            let build = || {
                let mut store = empty_store();
                let records = catalog(&mut store, &[("/r/a.txt", 5, 5), ("/r/c.txt", 8, 8)]);
                (store, records)
            };
            // 11 files with the content of a.txt, in reverse byte order.
            let candidates: Vec<String> = (0..11).map(|i| format!("/r/m{i:02}.txt")).collect();
            let entries =
                || -> Vec<Walked> { candidates.iter().rev().map(|p| file(p, 5)).collect() };
            let inspect = |p: &str| Ok(facts(p, 5, &sha(5), "text/plain"));

            // Off: nothing at all changes.
            let (mut store, records) = build();
            let a = records[0].id.clone();
            let before = resources_ttl(&store);
            let (result, _) = run(
                &mut store,
                &opts("/r", 1 << 30, &[]),
                Prune::Off,
                entries(),
                |_| None,
                inspect,
                no_glob,
            );
            let m = result.unwrap();
            assert!(m.changes.is_empty());
            assert_eq!(resources_ttl(&store), before);
            let v = &m.value;
            assert!(v.relinked.is_empty() && v.added.is_empty() && v.pruned.is_empty());
            assert_eq!(v.ambiguous.len(), 1);
            assert_eq!(v.ambiguous[0].resource.id, a);
            assert_eq!(v.ambiguous[0].resource.path, "/r/a.txt");
            // 11 candidates, at most 10 listed, the first ten in byte order.
            assert_eq!(v.ambiguous[0].candidates, candidates[..10].to_vec());
            assert_eq!(paths(&v.missing), vec!["/r/a.txt", "/r/c.txt"]);
            assert_eq!(tally(&v.counts), [0, 0, 0, 1, 0, 2, 0, 0]);

            // On: the ambiguous Resource is never pruned, the plain Missing one is.
            let (mut store, records) = build();
            let c = records[1].id.clone();
            let (result, _) = run(
                &mut store,
                &opts("/r", 1 << 30, &[]),
                Prune::On,
                entries(),
                |_| None,
                inspect,
                no_glob,
            );
            let m = result.unwrap();
            let v = &m.value;
            assert_eq!(v.ambiguous.len(), 1);
            assert_eq!(ids(&v.pruned), vec![c.as_str()]);
            assert_eq!(paths(&v.missing), vec!["/r/a.txt"]);
            assert_eq!(tally(&v.counts), [0, 0, 0, 1, 0, 1, 1, 0]);
            let ttl = resources_ttl(&store);
            assert!(ttl.contains("file:///r/a.txt") && !ttl.contains("file:///r/c.txt"));
            assert!(!ttl.contains("m00.txt"), "no candidate is registered");

            // Two Missing Resources with one file's content: both are ambiguous, the file is
            // not registered.
            let mut store = empty_store();
            catalog(&mut store, &[("/r/a1.txt", 5, 5), ("/r/a2.txt", 5, 5)]);
            let before = resources_ttl(&store);
            let (result, _) = run(
                &mut store,
                &opts("/r", 1 << 30, &[]),
                Prune::On,
                vec![file("/r/one.txt", 5)],
                |_| None,
                inspect,
                no_glob,
            );
            let m = result.unwrap();
            let v = &m.value;
            assert_eq!(
                v.ambiguous
                    .iter()
                    .map(|a| (a.resource.path.as_str(), a.candidates.clone()))
                    .collect::<Vec<_>>(),
                vec![
                    ("/r/a1.txt", strings(&["/r/one.txt"])),
                    ("/r/a2.txt", strings(&["/r/one.txt"]))
                ]
            );
            assert!(v.relinked.is_empty() && v.added.is_empty() && v.pruned.is_empty());
            assert_eq!(tally(&v.counts), [0, 0, 0, 2, 0, 2, 0, 0]);
            assert!(m.changes.is_empty());
            assert_eq!(resources_ttl(&store), before);
        }

        /// Row 4: What is never a move candidate: empty files, a Missing Resource recorded empty, a
        /// file over the size limit, one the media type patterns rule out, one that cannot be
        /// hashed; and content only present Resources have is registered as usual.
        #[test]
        fn what_is_never_a_move_candidate() {
            let mut store = empty_store();
            catalog(
                &mut store,
                &[
                    ("/r/e-old", 0, 0),              // recorded empty
                    ("/r/huge-old.bin", 1000, 1000), // its file is too large
                    ("/r/pic-old.png", 8, 8),        // its file is ruled out by name
                    ("/r/blob-old", 9, 9),           // its file is ruled out by content
                    ("/r/locked-old.txt", 10, 10),   // its file cannot be read
                    ("/r/present.txt", 11, 11),      // present
                    ("/o/q.txt", 12, 12),            // present, outside the root
                ],
            );
            let entries = vec![
                file("/r/e-new", 0),
                file("/r/huge.bin", 1000),
                file("/r/pic.png", 8),
                file("/r/noext9", 9),
                file("/r/locked.txt", 10),
                file("/r/copy.txt", 11),
                file("/r/copyq.txt", 12),
            ];
            let (result, (_, inspects)) = run(
                &mut store,
                &opts("/r", 100, &["text/*"]),
                Prune::Off,
                entries,
                stat_in(&[("/r/present.txt", 11), ("/o/q.txt", 12)]),
                |p| match p {
                    "/r/e-new" => Ok(facts(p, 0, &sha(0), "text/plain")),
                    "/r/noext9" => Ok(facts(p, 9, &sha(9), "application/octet-stream")),
                    "/r/locked.txt" => Err(Unhashable::Unreadable),
                    "/r/copy.txt" => Ok(facts(p, 11, &sha(11), "text/plain")),
                    "/r/copyq.txt" => Ok(facts(p, 12, &sha(12), "text/plain")),
                    other => panic!("{other} must not be opened"),
                },
                glob_by_extension,
            );
            let m = result.unwrap();
            let v = &m.value;
            assert!(v.relinked.is_empty() && v.ambiguous.is_empty() && v.pruned.is_empty());
            // The empty file and both copies of present content are registered as usual.
            assert_eq!(
                paths(&v.added),
                vec!["/r/copy.txt", "/r/copyq.txt", "/r/e-new"]
            );
            assert_eq!(
                skips(v),
                vec![
                    ("/r/huge.bin", "too_large"),
                    ("/r/locked.txt", "unreadable")
                ]
            );
            assert_eq!(
                paths(&v.missing),
                vec![
                    "/r/blob-old",
                    "/r/e-old",
                    "/r/huge-old.bin",
                    "/r/locked-old.txt",
                    "/r/pic-old.png"
                ]
            );
            assert_eq!(tally(&v.counts), [3, 0, 0, 0, 1, 5, 0, 2]);
            // Too large and ruled out by name: never opened.
            assert_eq!(
                sorted(inspects),
                strings(&[
                    "/r/copy.txt",
                    "/r/copyq.txt",
                    "/r/e-new",
                    "/r/locked.txt",
                    "/r/noext9"
                ])
            );
        }

        /// Row 5: Relinks, refreshes and adds are one mutation; ids go to registered files only, in
        /// path order.
        #[test]
        fn moves_refreshes_and_adds_in_one_mutation() {
            let moved = "/fixtures/refresh/m-moved.txt";
            let a_new = "/fixtures/refresh/a-new.txt";
            let z_new = "/fixtures/refresh/z-new.txt";
            let inspect = |p: &str| match p {
                p if p == a_new => Ok(facts(p, 3, &sha(3), "text/plain")),
                p if p == z_new => Ok(facts(p, 4, &sha(4), "text/plain")),
                p if p == moved => Ok(moved_facts_hello(p)),
                other => facts_of(other),
            };
            fn moved_facts_hello(p: &str) -> FileFacts {
                FileFacts {
                    modified: NEW.to_string(),
                    ..facts(p, 6, SHA_HELLO, "text/plain")
                }
            }
            let mut store = hali::storage::load(&fixture("refresh")).unwrap();
            let (result, _) = run(
                &mut store,
                &opts(ROOT, 1 << 30, &[]),
                Prune::Off,
                vec![
                    file(SAME, 6),
                    file(EDITED, 12),
                    file(TOUCHED, 6),
                    file(z_new, 4),
                    file(moved, 6),
                    file(a_new, 3),
                ],
                stat_of,
                inspect,
                no_glob,
            );
            let m = result.unwrap();
            let v = &m.value;
            assert_eq!(moves(v), vec![(GONE, moved)]);
            assert_eq!(v.relinked[0].resource.id, R4);
            assert_eq!(paths(&v.added), vec![a_new, z_new]);
            assert_eq!(ids(&v.refreshed), vec![R2, R3]);
            assert_eq!(tally(&v.counts), [2, 2, 1, 0, 2, 0, 0, 0]);

            // The oracle: `refresh --all`, `relink::explicit` and `add` of the registered files,
            // on a copy, each with the same fakes.
            let mut oracle = hali::storage::load(&fixture("refresh")).unwrap();
            let refreshed = resource::refresh(&ctx(), &mut oracle, &Target::All, stat_of, |p| {
                let f = facts_of(p).ok().unwrap();
                Ok(resource::Seen::Hashed(resource::Hashed {
                    byte_size: f.byte_size,
                    modified: f.modified,
                    sha256: f.sha256,
                }))
            })
            .unwrap();
            let relinked = relink::explicit(
                &ctx(),
                &mut oracle,
                R4,
                moved,
                &resource::Hashed {
                    byte_size: 6,
                    modified: NEW.to_string(),
                    sha256: SHA_HELLO.to_string(),
                },
                false,
            )
            .unwrap();
            let added = resource::add_all(
                &ctx(),
                &mut oracle,
                &[
                    facts(a_new, 3, &sha(3), "text/plain"),
                    facts(z_new, 4, &sha(4), "text/plain"),
                ],
                None,
                &[],
                &[],
                "tester",
            )
            .unwrap();
            let mut want: Vec<_> = refreshed
                .changes
                .into_iter()
                .chain(relinked.changes)
                .chain(added.changes)
                .collect();
            want.sort();
            want.dedup();
            assert_eq!(m.changes, want);
            assert_eq!(
                triples(&resources_ttl(&store)),
                triples(&resources_ttl(&oracle))
            );
            assert_sorted(&m);
        }

        /// Row 6: Without prune nothing is forgotten, and `scan` is `scan_with` with `Prune::Off`.
        #[test]
        fn prune_off_forgets_nothing() {
            let mut store = hali::storage::load(&fixture("forget")).unwrap();
            let before = resources_ttl(&store);
            let (result, _) = run(
                &mut store,
                &opts("/fixtures/forget", 1 << 30, &[]),
                Prune::Off,
                vec![],
                |_| None,
                |_| Err(Unhashable::Missing),
                no_glob,
            );
            let m = result.unwrap();
            assert!(m.changes.is_empty());
            assert_eq!(resources_ttl(&store), before);
            let v = &m.value;
            assert!(v.pruned.is_empty());
            assert_eq!(
                paths(&v.missing),
                vec![
                    "/fixtures/forget/one.txt",
                    "/fixtures/forget/three.txt",
                    "/fixtures/forget/two.txt"
                ]
            );
            assert_eq!(tally(&v.counts), [0, 0, 0, 0, 0, 3, 0, 0]);

            let mut plain = hali::storage::load(&fixture("forget")).unwrap();
            let old = scan::scan(
                &ctx(),
                &mut plain,
                &opts("/fixtures/forget", 1 << 30, &[]),
                Vec::<Walked>::new(),
                |_| None,
                |_: &str| Err(Unhashable::Missing),
                no_glob,
                |_, _| {},
                "tester",
            )
            .unwrap();
            assert_eq!(old.value, m.value);
            assert_eq!(old.changes, m.changes);
            assert_eq!(resources_ttl(&plain), before);
        }

        /// Row 7: With prune, the Missing Resources under the root are forgotten as `forget`
        /// forgets them; a shared format node goes only when no Resource uses it; outside the
        /// root nothing is pruned; no file is opened.
        #[test]
        fn prune_forgets_like_forget() {
            let build = || {
                let mut store = hali::storage::load(&fixture("forget")).unwrap();
                catalog(&mut store, &[("/elsewhere/x.txt", 6, 77)]);
                store
            };
            let options = opts("/fixtures/forget", 1 << 30, &[]);
            let forgot = |store: &mut Store, id: &str, force: bool| {
                resource::forget(&ctx(), store, id, force).unwrap()
            };

            // On: F3 relates to F2, so F2 keeps an inbound relation (from a Resource that is
            // pruned too: it still blocks). F1 and F3 go; the format node stays, F2 uses it.
            let mut store = build();
            let (result, (stats, inspects)) = run(
                &mut store,
                &options,
                Prune::On,
                vec![],
                |_| None,
                |p| panic!("{p}: no file is opened to prune"),
                no_glob,
            );
            let m = result.unwrap();
            let mut oracle = build();
            let one = forgot(&mut oracle, F1, false);
            let three = forgot(&mut oracle, F3, false);
            assert_eq!(resources_ttl(&store), resources_ttl(&oracle));
            let v = &m.value;
            assert_eq!(ids(&v.pruned), vec![F1, F3]);
            assert_eq!(v.pruned[0], one.value);
            assert_eq!(v.pruned[1], three.value);
            assert_eq!(v.pruned[0].path, "/fixtures/forget/one.txt");
            assert_eq!(ids(&v.missing), vec![F2], "F2 is kept, so still missing");
            assert_eq!(tally(&v.counts), [0, 0, 0, 0, 0, 1, 2, 0]);
            assert!(inspects.is_empty());
            assert_eq!(
                sorted(stats),
                strings(&[
                    "/fixtures/forget/one.txt",
                    "/fixtures/forget/three.txt",
                    "/fixtures/forget/two.txt"
                ]),
                "each Resource under the root once, none outside"
            );
            assert!(resources_ttl(&store).contains("text/x-forget"));
            assert!(resources_ttl(&store).contains("file:///elsewhere/x.txt"));
            let want: Vec<_> = {
                let mut c: Vec<_> = one.changes.into_iter().chain(three.changes).collect();
                c.sort();
                c.dedup();
                c
            };
            assert_eq!(m.changes, want);
            assert!(m.changes.iter().all(|c| c.op == Op::Remove));
            assert_sorted(&m);

            // Forced: all three go; with the last user of the format node, the node goes too.
            let mut store = build();
            let (result, _) = run(
                &mut store,
                &options,
                Prune::Forced,
                vec![],
                |_| None,
                |p| panic!("{p}: no file is opened to prune"),
                no_glob,
            );
            let m = result.unwrap();
            let mut oracle = build();
            forgot(&mut oracle, F3, false);
            forgot(&mut oracle, F2, false);
            forgot(&mut oracle, F1, false);
            assert_eq!(resources_ttl(&store), resources_ttl(&oracle));
            let v = &m.value;
            assert_eq!(ids(&v.pruned), vec![F1, F3, F2], "in path order");
            assert!(v.missing.is_empty());
            assert_eq!(tally(&v.counts), [0, 0, 0, 0, 0, 0, 3, 0]);
            assert!(!resources_ttl(&store).contains("text/x-forget"));
            assert!(resources_ttl(&store).contains("file:///elsewhere/x.txt"));
        }

        /// Row 8: An inbound relation (in any graph, from a Resource that is pruned too) keeps a
        /// Resource unless `Forced`, which removes it; one held only in extensions.ttl is
        /// `SHACL_VIOLATION` under `Forced`, with nothing written.
        #[test]
        fn prune_skips_inbound_relations_unless_forced() {
            // r1..r5: r1 is related to by r3 and r4, r2 by r1 and r4, r3 by r5 (extensions.ttl
            // only), r4 and r5 by nobody.
            let (r1, r2, r3, r4, r5) = (
                "0192f0c4-0000-7000-8000-000000000701",
                "0192f0c4-0000-7000-8000-000000000702",
                "0192f0c4-0000-7000-8000-000000000703",
                "0192f0c4-0000-7000-8000-000000000704",
                "0192f0c4-0000-7000-8000-000000000705",
            );
            let load = || hali::storage::load(&fixture("relations")).unwrap();
            let options = opts(RELATIONS, 1 << 30, &[]);
            let all_missing = |_: &str| None;
            let forgot = |store: &mut Store, id: &str, force: bool| {
                resource::forget(&ctx(), store, id, force).unwrap();
            };

            // (a) Everything is Missing, On: r4 and r5 have no inbound relation and go; r1, r2
            // and r3 are kept, though what relates to r1, r2 and r3 is pruned too.
            let mut store = load();
            let (result, _) = run(
                &mut store,
                &options,
                Prune::On,
                vec![],
                all_missing,
                |_| Err(Unhashable::Missing),
                no_glob,
            );
            let m = result.unwrap();
            let mut oracle = load();
            forgot(&mut oracle, r4, false);
            forgot(&mut oracle, r5, false);
            assert_eq!(resources_ttl(&store), resources_ttl(&oracle));
            let v = &m.value;
            assert_eq!(ids(&v.pruned), vec![r4, r5]);
            assert_eq!(ids(&v.missing), vec![r1, r2, r3]);
            assert_eq!(tally(&v.counts), [0, 0, 0, 0, 0, 3, 2, 0]);

            // (b) Forced while r5 is present: r3 is related to only by r5, in extensions.ttl,
            // which `Forced` cannot remove: the store would not conform, nothing is written.
            let mut store = load();
            let before = resources_ttl(&store);
            let (result, _) = run(
                &mut store,
                &options,
                Prune::Forced,
                vec![],
                stat_in(&[("/fixtures/relations/r5.txt", 6)]),
                |_| Err(Unhashable::Missing),
                no_glob,
            );
            let err = result.expect_err("an extensions-only inbound relation");
            assert_eq!(err.rule, "SHACL_VIOLATION");
            assert_eq!(err.exit_code(), 1);
            assert_eq!(resources_ttl(&store), before, "the store is untouched");

            // (c) r3 and r5 are present, On: r4 goes; r1 (related to by the present r3) and r2
            // (by r1, kept) stay.
            const PRESENT: &[(&str, u64)] = &[
                ("/fixtures/relations/r3.txt", 6),
                ("/fixtures/relations/r5.txt", 6),
            ];
            let mut store = load();
            let (result, _) = run(
                &mut store,
                &options,
                Prune::On,
                vec![],
                stat_in(PRESENT),
                |_| Err(Unhashable::Missing),
                no_glob,
            );
            let m = result.unwrap();
            let mut oracle = load();
            forgot(&mut oracle, r4, false);
            assert_eq!(resources_ttl(&store), resources_ttl(&oracle));
            let v = &m.value;
            assert_eq!(ids(&v.pruned), vec![r4]);
            assert_eq!(ids(&v.missing), vec![r1, r2]);
            assert_eq!(tally(&v.counts), [0, 0, 0, 0, 2, 2, 1, 0]);

            // (d) The same, Forced: r1, r2 and r4 go, and the triples that point at r1 and r2
            // are removed from resources.ttl; r3 and r5 stay.
            let mut store = load();
            let (result, _) = run(
                &mut store,
                &options,
                Prune::Forced,
                vec![],
                stat_in(PRESENT),
                |_| Err(Unhashable::Missing),
                no_glob,
            );
            let m = result.unwrap();
            let mut oracle = load();
            forgot(&mut oracle, r1, true);
            forgot(&mut oracle, r2, true);
            forgot(&mut oracle, r4, true);
            assert_eq!(resources_ttl(&store), resources_ttl(&oracle));
            let v = &m.value;
            assert_eq!(ids(&v.pruned), vec![r1, r2, r4]);
            assert!(v.missing.is_empty());
            assert_eq!(tally(&v.counts), [0, 0, 0, 0, 2, 0, 3, 0]);
            let ttl = resources_ttl(&store);
            assert!(ttl.contains(r3) && ttl.contains(r5));
            for gone in [r1, r2, r4] {
                assert!(!ttl.contains(gone), "{gone} is still in resources.ttl");
            }
        }

        /// Row 9: One SHACL check over the refresh, the relink, the prune and the adds together: a
        /// violation writes nothing and lists every Resource the final store has.
        #[test]
        fn one_shacl_check_with_moves_and_prune() {
            let build = || {
                let mut store = empty_store();
                let records = catalog(
                    &mut store,
                    &[
                        ("/r/a.txt", 5, 5), // moves
                        ("/r/b.txt", 6, 6), // is refreshed
                        ("/r/u.txt", 7, 7), // unchanged
                        ("/r/c.txt", 8, 8), // is pruned
                    ],
                );
                (store, records)
            };
            let entries = || {
                vec![
                    file("/r/b.txt", 9),
                    file("/r/u.txt", 7),
                    file("/r/d1.txt", 1),
                    file("/r/d2.txt", 2),
                    file("/r/m/a.txt", 5),
                ]
            };
            let inspect = |p: &str| match p {
                "/r/b.txt" => Ok(moved_facts(p, 9, 9)),
                "/r/m/a.txt" => Ok(moved_facts(p, 5, 5)),
                "/r/d1.txt" => Ok(facts(p, 1, &sha(1), "text/plain")),
                "/r/d2.txt" => Ok(facts(p, 2, &sha(2), "text/plain")),
                other => panic!("{other} must not be opened"),
            };
            // b.txt differs in size (Stale), u.txt is as recorded, a.txt and c.txt are gone.
            let stat = |p: &str| match p {
                "/r/b.txt" => Some((9, PINNED)),
                "/r/u.txt" => Some((7, PINNED)),
                _ => None,
            };

            // Without the extension shape the scenario goes through.
            let (mut store, _) = build();
            let (result, _) = run(
                &mut store,
                &opts("/r", 1 << 30, &[]),
                Prune::On,
                entries(),
                stat,
                inspect,
                no_glob,
            );
            let m = result.unwrap();
            assert_eq!(tally(&m.value.counts), [2, 1, 1, 0, 1, 0, 1, 0]);

            // With a shape every Resource violates: one check, over the final store.
            let (mut store, records) = build();
            let c = records[3].id.clone();
            let shaped = hali::storage::load(&fixture("scan/needs-description")).unwrap();
            let extension: Vec<Quad> = shaped
                .iter()
                .map(|q| q.unwrap())
                .filter(|q| {
                    matches!(&q.graph_name, oxigraph::model::GraphName::NamedNode(g)
                        if g.as_str() == hali::storage::EXTENSIONS_GRAPH)
                })
                .collect();
            assert!(!extension.is_empty());
            for q in &extension {
                store.insert(q).unwrap();
            }
            let before = resources_ttl(&store);
            let (result, _) = run(
                &mut store,
                &opts("/r", 1 << 30, &[]),
                Prune::On,
                entries(),
                stat,
                inspect,
                no_glob,
            );
            let err = result.expect_err("the extension shape is violated");
            assert_eq!(err.rule, "SHACL_VIOLATION");
            assert_eq!(err.exit_code(), 1);
            // a (relinked), b (refreshed), u, d1 and d2: five Resources in the final store, and
            // not c, which the same mutation forgets.
            assert_eq!(err.details["total"], 5);
            let focus: BTreeSet<&str> = err.details["violations"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v["focus_node"].as_str().unwrap())
                .collect();
            assert_eq!(focus.len(), 5, "{focus:?}");
            assert!(
                focus.iter().all(|f| !f.contains(&c)),
                "c is pruned in the same mutation: {focus:?}"
            );
            assert_eq!(resources_ttl(&store), before, "the store is untouched");
        }

        /// The three new lists are capped at 100 and `truncated` covers each; the counts stay
        /// exact; the order is the record's path (the new path for a relinked Resource).
        #[test]
        fn caps_cover_the_new_lists() {
            let old: Vec<FileFacts> = (0..101)
                .map(|i| facts(&format!("/r/old/f{i:03}.txt"), 2, &sha(i + 1), "text/plain"))
                .collect();

            // 101 relinked: the new paths run the other way round.
            let mut store = empty_store();
            catalog_facts(&mut store, &old);
            let entries: Vec<Walked> = (0..101)
                .map(|i| file(&format!("/r/new/g{:03}.txt", 100 - i), 2))
                .collect();
            let (result, _) = run(
                &mut store,
                &opts("/r", 1 << 30, &[]),
                Prune::Off,
                entries,
                |_| None,
                |p| {
                    let i: usize = 100 - p["/r/new/g".len()..][..3].parse::<usize>().unwrap();
                    Ok(facts(p, 2, &sha(i + 1), "text/plain"))
                },
                no_glob,
            );
            let m = result.unwrap();
            assert_eq!(m.value.counts.relinked, 101);
            assert_eq!(m.value.relinked.len(), 100);
            assert_eq!(
                (
                    m.value.relinked[0].resource.path.as_str(),
                    m.value.relinked[0].from.as_str()
                ),
                ("/r/new/g000.txt", "/r/old/f100.txt")
            );
            assert_eq!(m.value.relinked[99].resource.path, "/r/new/g099.txt");
            assert!(m.value.truncated);

            // 101 ambiguous, two candidate files each.
            let mut store = empty_store();
            catalog_facts(&mut store, &old);
            let entries: Vec<Walked> = (0..101)
                .flat_map(|i| [format!("/r/x/{i:03}b.txt"), format!("/r/x/{i:03}a.txt")])
                .map(|p| file(&p, 2))
                .collect();
            let (result, _) = run(
                &mut store,
                &opts("/r", 1 << 30, &[]),
                Prune::On,
                entries,
                |_| None,
                |p| {
                    let i: usize = p["/r/x/".len()..][..3].parse().unwrap();
                    Ok(facts(p, 2, &sha(i + 1), "text/plain"))
                },
                no_glob,
            );
            let m = result.unwrap();
            assert_eq!(m.value.counts.ambiguous, 101);
            assert_eq!(m.value.counts.pruned, 0);
            assert_eq!(m.value.ambiguous.len(), 100);
            assert_eq!(m.value.ambiguous[0].resource.path, "/r/old/f000.txt");
            assert_eq!(
                m.value.ambiguous[0].candidates,
                strings(&["/r/x/000a.txt", "/r/x/000b.txt"])
            );
            assert!(m.value.truncated);

            // 101 pruned.
            let mut store = empty_store();
            catalog_facts(&mut store, &old);
            let (result, _) = run(
                &mut store,
                &opts("/r", 1 << 30, &[]),
                Prune::On,
                vec![],
                |_| None,
                |_| Err(Unhashable::Missing),
                no_glob,
            );
            let m = result.unwrap();
            assert_eq!(m.value.counts.pruned, 101);
            assert_eq!(m.value.counts.missing, 0);
            assert_eq!(m.value.pruned.len(), 100);
            assert_eq!(m.value.pruned[0].path, "/r/old/f000.txt");
            assert_eq!(m.value.pruned[99].path, "/r/old/f099.txt");
            assert!(m.value.truncated);
            assert!(!resources_ttl(&store).contains("/r/old/"));
        }
    }
}
