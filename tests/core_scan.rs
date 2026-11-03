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

#[cfg(feature = "goal")]
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
            facts_of,
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
            &facts("/fixtures/refresh/new.txt", 3, &sha(1), "text/plain"),
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
        let err = result.err().expect("the extension shape is violated");
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
}
