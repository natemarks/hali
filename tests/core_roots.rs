//! Goal tests for the remembered scan roots in `core` (#23, spec §7): pure text in, text out.
//!
//! Assumed API (invented here; the implementer follows it), module `hali::core::roots`:
//!   `struct RootEntry { pub root: String, pub include_hidden: bool, pub no_ignore: bool,
//!       pub exclude: Vec<String>, pub max_size: Option<u64>, pub media_types: Vec<String> }`
//!       `Clone + Debug + PartialEq + Eq + Serialize`, the fields serialised in that order
//!       (`serde_json::to_string` gives the entry the file and `--list` hold). `root` is the real,
//!       absolute path; `exclude` and `media_types` are the values given on the command line,
//!       in order (`media_types` as lowercase patterns); `max_size` is in bytes.
//!   `parse(text: &str) -> Result<Vec<RootEntry>, E>` (`E: Debug`): the contents of `roots.json`.
//!   `serialise(roots: &[RootEntry]) -> String`: the canonical file contents.
//!   `remember(roots: Vec<RootEntry>, entry: RootEntry) -> Vec<RootEntry>`: adds the entry or
//!       replaces the one for its root; the result is sorted by root, in byte order.
//!   `forget(roots: Vec<RootEntry>, root: &str) -> Option<(Vec<RootEntry>, RootEntry)>`: the
//!       rest and the entry as it was; `None` when `root` is not remembered.
//!
//! The file is `{"version":1,"roots":[<entry>…]}` and a newline, compact (no spaces), the roots
//! sorted in byte order; an entry is `{"root","include_hidden","no_ignore","exclude",
//! "max_size","media_types"}` in that order, `max_size` a number or `null`. A file that is not
//! JSON of that shape, has another `version`, or holds a root that is empty or not absolute is an
//! error, and so is one whose entries have the wrong types.

#[cfg(feature = "goal")]
mod goal {
    use hali::core::roots::{RootEntry, forget, parse, remember, serialise};

    fn entry(root: &str) -> RootEntry {
        RootEntry {
            root: root.to_string(),
            include_hidden: false,
            no_ignore: false,
            exclude: vec![],
            max_size: None,
            media_types: vec![],
        }
    }

    fn full(root: &str) -> RootEntry {
        RootEntry {
            root: root.to_string(),
            include_hidden: true,
            no_ignore: true,
            exclude: vec!["*.tmp".to_string(), "a b".to_string()],
            max_size: Some(1024),
            media_types: vec!["text/*".to_string(), "application/pdf".to_string()],
        }
    }

    fn names(roots: &[RootEntry]) -> Vec<&str> {
        roots.iter().map(|r| r.root.as_str()).collect()
    }

    const EMPTY: &str = "{\"version\":1,\"roots\":[]}\n";
    const CANONICAL: &str = concat!(
        "{\"version\":1,\"roots\":[",
        "{\"root\":\"/data/a\",\"include_hidden\":false,\"no_ignore\":false,\"exclude\":[],",
        "\"max_size\":null,\"media_types\":[]},",
        "{\"root\":\"/data/b c\",\"include_hidden\":true,\"no_ignore\":true,",
        "\"exclude\":[\"*.tmp\",\"a b\"],\"max_size\":1024,",
        "\"media_types\":[\"text/*\",\"application/pdf\"]}",
        "]}\n"
    );

    /// Row 2: The canonical text: compact, ordered keys, a trailing newline, `max_size` null or
    /// a number.
    #[test]
    fn serialise_is_canonical() {
        assert_eq!(serialise(&[]), EMPTY);
        assert_eq!(serialise(&[entry("/data/a"), full("/data/b c")]), CANONICAL);
        assert_eq!(
            serde_json::to_string(&full("/x")).unwrap(),
            "{\"root\":\"/x\",\"include_hidden\":true,\"no_ignore\":true,\
             \"exclude\":[\"*.tmp\",\"a b\"],\"max_size\":1024,\
             \"media_types\":[\"text/*\",\"application/pdf\"]}"
        );
    }

    /// Row 2: A load and re-serialise is byte-identical; a file written by hand in another
    /// layout and order comes out canonical.
    #[test]
    fn round_trip_byte_identical() {
        for text in [EMPTY, CANONICAL] {
            let roots = parse(text).unwrap();
            assert_eq!(serialise(&roots), text);
        }
        assert_eq!(
            parse(CANONICAL).unwrap(),
            vec![entry("/data/a"), full("/data/b c")]
        );

        let by_hand = "{ \"roots\": [ {\"max_size\": 1024, \"media_types\": [\"text/*\",\
            \"application/pdf\"], \"exclude\": [\"*.tmp\", \"a b\"], \"no_ignore\": true,\
            \"include_hidden\": true, \"root\": \"/data/b c\"}, {\"root\": \"/data/a\",\
            \"include_hidden\": false, \"no_ignore\": false, \"exclude\": [], \"max_size\": null,\
            \"media_types\": []} ], \"version\": 1 }";
        assert_eq!(serialise(&parse(by_hand).unwrap()), CANONICAL);
    }

    /// Row 2: Non-ASCII and control characters in a root survive a round trip.
    #[test]
    fn odd_paths_round_trip() {
        let roots = vec![
            entry("/caf\u{e9}"),
            entry("/new\nline"),
            entry("/q\"uote\\"),
            entry("/\u{65e5}\u{672c}"),
        ];
        let text = serialise(&roots);
        assert_eq!(
            text.matches('\n').count(),
            1,
            "newlines are escaped: {text:?}"
        );
        let mut sorted = roots.clone();
        sorted.sort_by(|a, b| a.root.cmp(&b.root));
        assert_eq!(parse(&text).unwrap(), sorted);
    }

    /// Row 2: `remember` adds in byte order and a rescan replaces the settings of its root.
    #[test]
    fn remember_upserts() {
        let roots = remember(vec![], entry("/b"));
        let roots = remember(roots, entry("/a/b"));
        let roots = remember(roots, entry("/a-b"));
        let roots = remember(roots, entry("/B"));
        assert_eq!(names(&roots), ["/B", "/a-b", "/a/b", "/b"], "byte order");

        let roots = remember(roots, full("/a/b"));
        assert_eq!(
            names(&roots),
            ["/B", "/a-b", "/a/b", "/b"],
            "replaced, not added"
        );
        assert_eq!(roots[2], full("/a/b"));
        assert_eq!(roots[3], entry("/b"));

        // Remembering the same entry again changes nothing.
        let again = remember(roots.clone(), full("/a/b"));
        assert_eq!(again, roots);
        assert_eq!(serialise(&again), serialise(&roots));
    }

    /// Row 11, 12: `forget` returns the rest and the entry as it was; an unknown root is `None`
    /// and the match is exact (no trailing slash, no prefix).
    #[test]
    fn forget_removes_one_root() {
        let roots = vec![entry("/a"), full("/a/b"), entry("/c")];
        let (rest, gone) = forget(roots.clone(), "/a/b").unwrap();
        assert_eq!(gone, full("/a/b"));
        assert_eq!(names(&rest), ["/a", "/c"]);

        let (rest, gone) = forget(roots.clone(), "/a").unwrap();
        assert_eq!(gone, entry("/a"));
        assert_eq!(
            names(&rest),
            ["/a/b", "/c"],
            "a root's subroots are not forgotten"
        );

        for unknown in ["/a/", "/", "/b", "/A", "a", ""] {
            assert!(forget(roots.clone(), unknown).is_none(), "{unknown:?}");
        }
        let (rest, _) = forget(vec![entry("/only")], "/only").unwrap();
        assert!(rest.is_empty());
        assert_eq!(serialise(&rest), EMPTY);
    }

    /// Row 15: What is not a valid `roots.json` is an error, whatever is wrong.
    #[test]
    fn parse_rejects_bad_files() {
        let good = "{\"root\":\"/a\",\"include_hidden\":false,\"no_ignore\":false,\
                    \"exclude\":[],\"max_size\":null,\"media_types\":[]}";
        let with = |entry: &str| format!("{{\"version\":1,\"roots\":[{entry}]}}\n");
        assert!(parse(&with(good)).is_ok());

        let mut bad: Vec<(String, &str)> = vec![
            (String::new(), "empty"),
            ("not json".into(), "not json"),
            (
                "{\"version\":1,\"roots\":[]} trailing".into(),
                "trailing text",
            ),
            ("[]".into(), "an array"),
            ("null".into(), "null"),
            ("{\"roots\":[]}".into(), "no version"),
            ("{\"version\":2,\"roots\":[]}".into(), "version 2"),
            ("{\"version\":0,\"roots\":[]}".into(), "version 0"),
            (
                "{\"version\":\"1\",\"roots\":[]}".into(),
                "version a string",
            ),
            ("{\"version\":1}".into(), "no roots"),
            ("{\"version\":1,\"roots\":{}}".into(), "roots an object"),
            (
                "{\"version\":1,\"roots\":[\"/a\"]}".into(),
                "a root as a string",
            ),
        ];
        for (key, value, what) in [
            ("root", "\"relative/dir\"", "a relative root"),
            ("root", "\"\"", "an empty root"),
            ("root", "\"~/docs\"", "a tilde root"),
            ("root", "\"./a\"", "a dot root"),
            ("root", "7", "a numeric root"),
            ("include_hidden", "\"no\"", "include_hidden a string"),
            ("no_ignore", "null", "no_ignore null"),
            ("exclude", "\"*.tmp\"", "exclude a string"),
            ("exclude", "[1]", "exclude holding a number"),
            ("max_size", "-1", "a negative max_size"),
            ("max_size", "\"1KiB\"", "max_size a string"),
            ("max_size", "1.5", "a fractional max_size"),
            (
                "max_size",
                "99999999999999999999",
                "an overflowing max_size",
            ),
            ("media_types", "\"text/*\"", "media_types a string"),
        ] {
            let mut object: serde_json::Value = serde_json::from_str(good).unwrap();
            object[key] = serde_json::from_str(value).unwrap();
            bad.push((with(&object.to_string()), what));
        }
        for (key, what) in [
            ("root", "no root"),
            ("include_hidden", "no include_hidden"),
            ("no_ignore", "no no_ignore"),
            ("exclude", "no exclude"),
            ("max_size", "no max_size"),
            ("media_types", "no media_types"),
        ] {
            let value: serde_json::Value = serde_json::from_str(good).unwrap();
            let mut object = value.as_object().unwrap().clone();
            object.remove(key);
            bad.push((with(&serde_json::Value::Object(object).to_string()), what));
        }
        for (text, what) in &bad {
            assert!(parse(text).is_err(), "{what}: {text:?} was accepted");
        }
    }
}
