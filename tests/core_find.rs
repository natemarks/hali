//! Goal tests for `core::resource::find` and its helpers (#12, spec §6, §9.3, §9.7).
//!
//! Assumed API (invented here; the file facts come in only through the injected `stat`, and
//! `find` has no hash input, so core never touches the filesystem):
//!   `resource::Query { title: Option<String>, media_type: Option<MediaPattern>, missing: bool,
//!        stale: bool, tags: Vec<String>, exact: bool, sort: Sort, offset: usize, limit: usize }`
//!       `impl Default`: no filters, `Sort::Modified`, `offset: 0`, `limit: 50`.
//!   `resource::Sort::{Modified, Added, Title}` (Copy)
//!   `resource::MediaPattern`, built by
//!   `resource::parse_media_pattern(&str) -> Result<MediaPattern, hali::core::Error>`
//!       accepts `type/subtype` or `type/*`, matched case-insensitively; anything else is a
//!       usage error (exit code 2).
//!   `resource::find(&Context, &Store, &Query, stat: impl Fn(&str) -> Option<(u64, i64)>)
//!        -> Result<Found, hali::core::Error>`
//!       `stat(path)` is `Some((size, mtime_secs))` for a regular file, else `None`; it feeds
//!       `Record::observe`. Every Resource (a `hali:Resource` in resources.ttl) is a candidate.
//!   `Found { results: Vec<Record>, total: usize, truncated: bool, next_offset: Option<usize> }`
//!       `total` counts after the filters; `results` are observed records.
//!   `resource::parse_fields(&str) -> Result<Vec<String>, hali::core::Error>`
//!       the names in record order, without duplicates; an unknown or empty name is a usage
//!       error whose message lists the valid names.
//!   `resource::narrow(&Record, &[String]) -> serde_json::Value`
//!       the record as a JSON object holding only those fields, in record order.
//!
//! Stores are built with `resource::add`, whose behaviour #10 settled. Each Resource gets its
//! own `SeededIdGen` seed and `FixedClock` instant, so the catalog record's `issued` can vary.

mod goal {
    use hali::core::resource::{self, FileFacts, Found, Query, Record, Sort};
    use hali::core::{Context, FixedClock, SeededIdGen};
    use hali::storage::{File, Store};
    use std::cell::Cell;

    /// 2026-10-20T14:02:11Z, the `modified` of a fresh file.
    const MTIME: i64 = 1_792_504_931;
    const SHA: &str = "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03";

    struct Spec {
        path: &'static str,
        title: &'static str,
        media: &'static str,
        modified: &'static str,
        /// The `now` of the `add`, i.e. the catalog record's `issued`.
        added: &'static str,
    }

    fn spec(path: &'static str, title: &'static str) -> Spec {
        Spec {
            path,
            title,
            media: "text/plain",
            modified: "2026-10-20T14:02:11Z",
            added: "2026-10-24T09:30:00Z",
        }
    }

    fn ctx(now: &str, seed: u64) -> Context {
        Context::new(
            "en",
            Box::new(FixedClock::new(now)),
            Box::new(SeededIdGen::new(seed)),
        )
    }

    /// The store holding all `specs` (seed = index + 1) and the records `add` returned.
    fn build(specs: &[Spec]) -> (Store, Vec<Record>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = hali::storage::load(dir.path()).unwrap();
        let mut records = Vec::new();
        for (i, s) in specs.iter().enumerate() {
            let facts = FileFacts {
                path: s.path.to_string(),
                byte_size: 6,
                modified: s.modified.to_string(),
                sha256: SHA.to_string(),
                media_type: s.media.to_string(),
            };
            let seed = u64::try_from(i).unwrap() + 1;
            let added = resource::add(
                &ctx(s.added, seed),
                &mut store,
                &facts,
                Some(s.title),
                "user",
            )
            .unwrap();
            records.push(added.value.record);
        }
        (store, records, dir)
    }

    fn reader() -> Context {
        ctx("2026-10-25T00:00:00Z", 99)
    }

    /// Every file exists and is unchanged.
    fn fresh(_path: &str) -> Option<(u64, i64)> {
        Some((6, MTIME))
    }

    fn find(store: &Store, query: &Query) -> Found {
        resource::find(&reader(), store, query, fresh).unwrap()
    }

    fn ids(found: &Found) -> Vec<&str> {
        found.results.iter().map(|r| r.id.as_str()).collect()
    }

    fn titles(found: &Found) -> Vec<&str> {
        found.results.iter().map(|r| r.title.as_str()).collect()
    }

    /// Five Resources `/a/0` .. `/a/4`, modified one day apart, the last the newest.
    fn five() -> Vec<Spec> {
        let days = [
            "2026-10-10T00:00:00Z",
            "2026-10-11T00:00:00Z",
            "2026-10-12T00:00:00Z",
            "2026-10-13T00:00:00Z",
            "2026-10-14T00:00:00Z",
        ];
        let paths = ["/a/0", "/a/1", "/a/2", "/a/3", "/a/4"];
        (0..5)
            .map(|i| Spec {
                modified: days[i],
                ..spec(paths[i], ["zero", "one", "two", "three", "four"][i])
            })
            .collect()
    }

    // ---- criterion 1: paging, --fields --------------------------------------------------

    #[test]
    fn pages_with_limit_and_offset() {
        let (store, records, _dir) = build(&five());
        // Newest first: records 4, 3, 2, 1, 0.
        let by_age: Vec<&str> = records.iter().rev().map(|r| r.id.as_str()).collect();

        let q = |offset, limit| Query {
            offset,
            limit,
            ..Query::default()
        };
        let first = find(&store, &q(0, 2));
        assert_eq!(ids(&first), by_age[0..2]);
        assert_eq!(first.total, 5);
        assert!(first.truncated);
        assert_eq!(first.next_offset, Some(2));

        let second = find(&store, &q(2, 2));
        assert_eq!(ids(&second), by_age[2..4]);
        assert_eq!(second.total, 5);
        assert!(second.truncated);
        assert_eq!(second.next_offset, Some(4));

        let last = find(&store, &q(4, 2));
        assert_eq!(ids(&last), by_age[4..5]);
        assert_eq!(last.total, 5);
        assert!(!last.truncated);
        assert_eq!(last.next_offset, None);

        // A page that ends exactly at the total is not truncated.
        let exact = find(&store, &q(3, 2));
        assert_eq!(exact.results.len(), 2);
        assert!(!exact.truncated);
        assert_eq!(exact.next_offset, None);
    }

    #[test]
    fn offset_past_end_is_empty_page() {
        let (store, _records, _dir) = build(&five());
        for offset in [5, 6, 1000] {
            let found = find(
                &store,
                &Query {
                    offset,
                    ..Query::default()
                },
            );
            assert!(found.results.is_empty(), "offset {offset}");
            assert_eq!(found.total, 5, "offset {offset}");
            assert!(!found.truncated, "offset {offset}");
            assert_eq!(found.next_offset, None, "offset {offset}");
        }
    }

    #[test]
    fn default_limit_is_50() {
        let q = Query::default();
        assert_eq!(q.limit, 50);
        assert_eq!(q.offset, 0);
        assert!(matches!(q.sort, Sort::Modified));
        assert!(q.title.is_none() && q.media_type.is_none());
        assert!(!q.missing && !q.stale);

        let paths: Vec<String> = (0..51).map(|i| format!("/many/{i:02}")).collect();
        let specs: Vec<Spec> = paths
            .iter()
            .map(|p| spec(Box::leak(p.clone().into_boxed_str()), "same"))
            .collect();
        let (store, _records, _dir) = build(&specs);
        let found = find(&store, &q);
        assert_eq!(found.results.len(), 50);
        assert_eq!(found.total, 51);
        assert!(found.truncated);
        assert_eq!(found.next_offset, Some(50));
    }

    #[test]
    fn fields_narrow_in_record_order() {
        let (_store, records, _dir) = build(&five());
        let record = &records[0];

        // Given in another order, with a repeat: printed in record order, once.
        let fields = resource::parse_fields("title,id,title,modified").unwrap();
        assert_eq!(fields, ["id", "title", "modified"]);
        let narrowed = resource::narrow(record, &fields);
        assert_eq!(
            serde_json::to_string(&narrowed).unwrap(),
            format!(
                r#"{{"id":"{}","title":"zero","modified":"2026-10-10T00:00:00Z"}}"#,
                record.id
            )
        );

        // `id` is not forced in.
        let fields = resource::parse_fields("media_type,title").unwrap();
        assert_eq!(fields, ["title", "media_type"]);
        assert_eq!(
            serde_json::to_string(&resource::narrow(record, &fields)).unwrap(),
            r#"{"title":"zero","media_type":"text/plain"}"#
        );

        // Every name of the record is valid, and narrowing to all of them is the whole record.
        let all = "id,path,title,media_type,editable,missing,stale,types,tags,byte_size,modified";
        let fields = resource::parse_fields(all).unwrap();
        assert_eq!(fields.join(","), all);
        assert_eq!(
            resource::narrow(record, &fields),
            serde_json::to_value(record).unwrap()
        );
    }

    #[test]
    fn unknown_field_is_usage() {
        for bad in [
            "",
            "id,",
            ",id",
            "id,,title",
            "nope",
            "id,content",
            "ID",
            "id title",
        ] {
            let err = resource::parse_fields(bad).unwrap_err();
            assert_eq!(err.exit_code(), 2, "{bad:?}");
            for name in [
                "id",
                "path",
                "title",
                "media_type",
                "editable",
                "missing",
                "stale",
                "types",
                "tags",
                "byte_size",
                "modified",
            ] {
                assert!(
                    err.message.contains(name),
                    "{bad:?}: the message lists the valid names: {}",
                    err.message
                );
            }
        }
    }

    // ---- criterion 2: filters -----------------------------------------------------------

    #[test]
    fn title_is_case_insensitive_substring() {
        let specs = [
            spec("/a/1", "Field Notes"),
            spec("/a/2", "notebook"),
            spec("/a/3", "Other"),
            spec("/a/4", "\u{c9}COLE"),
        ];
        let (store, records, _dir) = build(&specs);
        let with = |title: &str| Query {
            title: Some(title.to_string()),
            sort: Sort::Title,
            ..Query::default()
        };

        assert_eq!(
            titles(&find(&store, &with("NOTE"))),
            ["Field Notes", "notebook"]
        );
        assert_eq!(titles(&find(&store, &with("d no"))), ["Field Notes"]);
        // Unicode lowercase, not ASCII only.
        assert_eq!(titles(&find(&store, &with("\u{e9}cole"))), ["\u{c9}COLE"]);
        assert!(find(&store, &with("absent")).results.is_empty());
        // An empty --title matches everything.
        let everything = find(&store, &with(""));
        assert_eq!(everything.total, 4);
        assert_eq!(everything.results.len(), records.len());
    }

    #[test]
    fn media_type_exact_and_wildcard() {
        let specs = [
            Spec {
                media: "text/plain",
                ..spec("/a/1", "one")
            },
            Spec {
                media: "text/markdown",
                ..spec("/a/2", "two")
            },
            Spec {
                media: "application/pdf",
                ..spec("/a/3", "three")
            },
            Spec {
                media: "application/x-shellscript",
                ..spec("/a/4", "four")
            },
        ];
        let (store, _records, _dir) = build(&specs);
        let with = |pattern: &str| Query {
            media_type: Some(resource::parse_media_pattern(pattern).unwrap()),
            sort: Sort::Title,
            ..Query::default()
        };

        assert_eq!(titles(&find(&store, &with("text/plain"))), ["one"]);
        assert_eq!(titles(&find(&store, &with("TEXT/Plain"))), ["one"]);
        assert_eq!(titles(&find(&store, &with("text/*"))), ["one", "two"]);
        assert_eq!(
            titles(&find(&store, &with("Application/*"))),
            ["four", "three"]
        );
        assert_eq!(titles(&find(&store, &with("application/pdf"))), ["three"]);
        assert_eq!(
            titles(&find(&store, &with("application/x-shellscript"))),
            ["four"]
        );
        // The subtype is matched whole, not as a prefix.
        assert!(find(&store, &with("text/plai")).results.is_empty());
        assert!(find(&store, &with("image/*")).results.is_empty());
    }

    #[test]
    fn bad_media_pattern_is_usage() {
        for bad in [
            "*/*",
            "text/x-*",
            "text/pl*",
            "*/plain",
            "text",
            "",
            "text/",
            "/plain",
            "/",
            "text/plain/extra",
            "text/plain,text/html",
            "text /plain",
        ] {
            let err = resource::parse_media_pattern(bad)
                .err()
                .unwrap_or_else(|| panic!("{bad:?} should be refused"));
            assert_eq!(err.exit_code(), 2, "{bad:?}");
        }
        for good in [
            "text/plain",
            "text/*",
            "TEXT/Plain",
            "application/x-shellscript",
        ] {
            assert!(resource::parse_media_pattern(good).is_ok(), "{good:?}");
        }
    }

    #[test]
    fn filters_and() {
        let specs = [
            Spec {
                media: "text/plain",
                ..spec("/a/1", "Field Notes")
            },
            Spec {
                media: "application/pdf",
                ..spec("/a/2", "Field Guide")
            },
            Spec {
                media: "text/plain",
                ..spec("/a/3", "Other Notes")
            },
        ];
        let (store, _records, _dir) = build(&specs);
        let q = Query {
            title: Some("field".to_string()),
            media_type: Some(resource::parse_media_pattern("text/*").unwrap()),
            ..Query::default()
        };
        let found = find(&store, &q);
        assert_eq!(titles(&found), ["Field Notes"]);
        assert_eq!(found.total, 1);

        // The filters apply before paging: `total` counts the filtered set.
        let q = Query {
            title: Some("notes".to_string()),
            limit: 1,
            sort: Sort::Title,
            ..Query::default()
        };
        let found = find(&store, &q);
        assert_eq!(titles(&found), ["Field Notes"]);
        assert_eq!(found.total, 2);
        assert!(found.truncated);
        assert_eq!(found.next_offset, Some(1));
    }

    // ---- criterion 3: sort --------------------------------------------------------------

    #[test]
    fn default_sort_is_modified_desc() {
        // Added in the order that is not the modified order.
        let specs = [
            Spec {
                modified: "2026-10-12T00:00:00Z",
                ..spec("/a/1", "middle")
            },
            Spec {
                modified: "2026-10-14T00:00:00Z",
                ..spec("/a/2", "newest")
            },
            Spec {
                modified: "2026-10-10T00:00:00Z",
                ..spec("/a/3", "oldest")
            },
        ];
        let (store, _records, _dir) = build(&specs);
        let found = find(&store, &Query::default());
        assert_eq!(titles(&found), ["newest", "middle", "oldest"]);
        let explicit = find(
            &store,
            &Query {
                sort: Sort::Modified,
                ..Query::default()
            },
        );
        assert_eq!(titles(&explicit), ["newest", "middle", "oldest"]);
    }

    #[test]
    fn sort_added_uses_issued() {
        // `modified` runs opposite to `issued`, so only the right key gives this order.
        let specs = [
            Spec {
                modified: "2026-10-14T00:00:00Z",
                added: "2026-10-21T00:00:00Z",
                ..spec("/a/1", "first added")
            },
            Spec {
                modified: "2026-10-12T00:00:00Z",
                added: "2026-10-23T00:00:00Z",
                ..spec("/a/2", "last added")
            },
            Spec {
                modified: "2026-10-10T00:00:00Z",
                added: "2026-10-22T00:00:00Z",
                ..spec("/a/3", "second added")
            },
            Spec {
                modified: "2026-10-16T00:00:00Z",
                added: "2026-10-24T00:00:00Z",
                ..spec("/a/4", "no record")
            },
        ];
        let (store, records, _dir) = build(&specs);
        let q = Query {
            sort: Sort::Added,
            ..Query::default()
        };
        assert_eq!(
            titles(&find(&store, &q)),
            ["no record", "last added", "second added", "first added"]
        );

        // A Resource with no catalog record goes last, whatever its other dates.
        let shown = resource::show(&reader(), &store, &records[3].id).unwrap();
        let record_id = shown.catalog_record.expect("add makes a catalog record").id;
        let subject = oxigraph::model::NamedNode::new(format!("urn:uuid:{record_id}")).unwrap();
        let graph = File::Resources.graph();
        let quads: Vec<_> = store
            .quads_for_pattern(
                Some(subject.as_ref().into()),
                None,
                None,
                Some(graph.as_ref().into()),
            )
            .map(Result::unwrap)
            .collect();
        assert!(!quads.is_empty());
        for quad in &quads {
            store.remove(quad).unwrap();
        }
        assert_eq!(
            titles(&find(&store, &q)),
            ["last added", "second added", "first added", "no record"]
        );
    }

    #[test]
    fn sort_title_case_insensitive() {
        let specs = [
            spec("/a/1", "banana"),
            spec("/a/2", "Apple"),
            spec("/a/3", "cherry"),
            spec("/a/4", "apple pie"),
            spec("/a/5", "Banana split"),
        ];
        let (store, _records, _dir) = build(&specs);
        let found = find(
            &store,
            &Query {
                sort: Sort::Title,
                ..Query::default()
            },
        );
        assert_eq!(
            titles(&found),
            ["Apple", "apple pie", "banana", "Banana split", "cherry"]
        );
    }

    #[test]
    fn ties_break_on_id() {
        // Same title, same modified, same issued: only the id orders them.
        let specs = [
            spec("/a/1", "same"),
            spec("/a/2", "same"),
            spec("/a/3", "same"),
            spec("/a/4", "Same"),
        ];
        let (store, records, _dir) = build(&specs);
        let mut want: Vec<&str> = records.iter().map(|r| r.id.as_str()).collect();
        want.sort_unstable();
        for sort in [Sort::Modified, Sort::Added, Sort::Title] {
            let found = find(
                &store,
                &Query {
                    sort,
                    ..Query::default()
                },
            );
            assert_eq!(
                ids(&found),
                want,
                "ties are ascending by id, even when descending"
            );
        }
    }

    // ---- criterion 4: missing and stale -------------------------------------------------

    /// Four Resources: `/a/ok`, `/a/gone`, `/a/grown` (size differs), `/a/touched` (mtime).
    fn observed_store() -> (Store, tempfile::TempDir) {
        let specs = [
            spec("/a/ok", "ok"),
            spec("/a/gone", "gone"),
            spec("/a/grown", "grown"),
            spec("/a/touched", "touched"),
        ];
        let (store, _records, dir) = build(&specs);
        (store, dir)
    }

    fn stat(path: &str) -> Option<(u64, i64)> {
        match path {
            "/a/gone" => None,
            "/a/grown" => Some((7, MTIME)),
            "/a/touched" => Some((6, MTIME + 1)),
            _ => Some((6, MTIME)),
        }
    }

    fn flags(found: &Found) -> Vec<(&str, bool, bool)> {
        let mut out: Vec<(&str, bool, bool)> = found
            .results
            .iter()
            .map(|r| (r.title.as_str(), r.missing, r.stale))
            .collect();
        out.sort_unstable();
        out
    }

    #[test]
    fn missing_and_stale_from_injected_stat() {
        let (store, _dir) = observed_store();
        let found = resource::find(&reader(), &store, &Query::default(), stat).unwrap();
        assert_eq!(found.total, 4);
        assert_eq!(
            flags(&found),
            [
                ("gone", true, false),
                ("grown", false, true),
                ("ok", false, false),
                ("touched", false, true),
            ]
        );
    }

    #[test]
    fn missing_filter() {
        let (store, _dir) = observed_store();
        let q = Query {
            missing: true,
            ..Query::default()
        };
        let found = resource::find(&reader(), &store, &q, stat).unwrap();
        assert_eq!(flags(&found), [("gone", true, false)]);
        assert_eq!(found.total, 1);
    }

    #[test]
    fn stale_filter() {
        let (store, _dir) = observed_store();
        let q = Query {
            stale: true,
            limit: 1,
            sort: Sort::Title,
            ..Query::default()
        };
        // The filter applies before paging, so `total` counts both stale Resources.
        let found = resource::find(&reader(), &store, &q, stat).unwrap();
        assert_eq!(titles(&found), ["grown"]);
        assert_eq!(found.total, 2);
        assert!(found.truncated);
        assert_eq!(found.next_offset, Some(1));
        let next = resource::find(&reader(), &store, &Query { offset: 1, ..q }, stat).unwrap();
        assert_eq!(titles(&next), ["touched"]);
        assert!(!next.truncated);
    }

    #[test]
    fn missing_and_stale_together_empty() {
        let (store, _dir) = observed_store();
        let q = Query {
            missing: true,
            stale: true,
            ..Query::default()
        };
        let found = resource::find(&reader(), &store, &q, stat).unwrap();
        assert!(found.results.is_empty());
        assert_eq!(found.total, 0);
        assert!(!found.truncated);
        assert_eq!(found.next_offset, None);
    }

    // ---- P1: lazy stat ------------------------------------------------------------------

    #[test]
    fn stat_only_the_page_without_missing_or_stale_filters() {
        let (store, _records, _dir) = build(&five());
        let calls = Cell::new(0usize);
        let counting = |_path: &str| {
            calls.set(calls.get() + 1);
            Some((6, MTIME))
        };

        let q = Query {
            limit: 2,
            offset: 1,
            ..Query::default()
        };
        let found = resource::find(&reader(), &store, &q, counting).unwrap();
        assert_eq!(found.results.len(), 2);
        assert_eq!(found.total, 5);
        assert_eq!(calls.get(), 2, "only the returned page is stat'ed");

        // An empty page stats nothing.
        calls.set(0);
        let q = Query {
            offset: 9,
            ..Query::default()
        };
        let found = resource::find(&reader(), &store, &q, counting).unwrap();
        assert!(found.results.is_empty());
        assert_eq!(calls.get(), 0);

        // The filters need every candidate's state, so every candidate is stat'ed.
        calls.set(0);
        let q = Query {
            missing: true,
            limit: 1,
            ..Query::default()
        };
        let found = resource::find(&reader(), &store, &q, counting).unwrap();
        assert_eq!(found.total, 0);
        assert_eq!(calls.get(), 5);
    }
}
