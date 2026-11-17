//! Source guard for the SPARQL evaluator (#9, spec §9.9, §15, `.cargo/audit.toml`). Permanent:
//! it passes on the code as it stands and keeps passing as `hali query` lands.
//!
//! rudof's `shacl` forces oxigraph's `http-client` feature on, so a `SERVICE` clause in user
//! text could reach the network unless every evaluator is built without the default HTTP service
//! handler; and quick-xml (RUSTSEC-2026-0194 and -0195) stays unreachable only while hali never
//! reads or writes RDF/XML or SPARQL XML results. This scans the text of `src/`:
//!
//! 1. every `SparqlEvaluator::new()` is followed, in the same statement, by
//!    `.without_default_http_service_handler()`;
//! 2. nothing in `src/` names `QueryResultsFormat::Xml`, `RdfFormat::RdfXml`, `RdfXml`,
//!    `QueryResultsParser` or `RdfParser`; nor any way to pick or write a format by name
//!    (`QueryResultsFormat::from_extension`, `QueryResultsFormat::from_media_type`,
//!    `QueryResultsSerializer`, `RdfSerializer`), the crates that hold the XML code
//!    (`oxrdfxml`, `sparesults`, `quick_xml`), or an evaluator with default settings
//!    (`SparqlEvaluator::default()`, which would keep the HTTP service handler), the
//!    path-style calls `Store::query(` and `Store::update(`, `RdfFormat::from_extension` and
//!    `RdfFormat::from_media_type`, and renaming `SparqlEvaluator` or `QueryResultsFormat`
//!    with `as` (which would hide a banned name from this scan); and nothing
//!    outside `src/core/query` (the module or directory of the query seam, where UPDATE text is
//!    refused) names `parse_update`;
//! 3. nothing in `src/` runs a query or an update through the store's own entry points
//!    (`.query(`, `.query_opt(`, `.query_with_options(`, `.update(` and `.update_opt(` on a
//!    store, whose default evaluator has the HTTP service handler). hali's JSON results are
//!    written by hand, so no result serializer is needed: nothing is allow-listed.
//!
//! Comments are not code: a line comment may name what the code must never use.

use std::path::{Path, PathBuf};

const ALWAYS_BANNED: &[&str] = &[
    "QueryResultsFormat::Xml",
    "RdfFormat::RdfXml",
    "RdfXml",
    "QueryResultsParser",
    "RdfParser",
    "QueryResultsFormat::from_extension",
    "QueryResultsFormat::from_media_type",
    "QueryResultsSerializer",
    "RdfSerializer",
    "SparqlEvaluator::default()",
    "oxrdfxml",
    "sparesults",
    "quick_xml",
    "RdfFormat::from_extension",
    "RdfFormat::from_media_type",
    "Store::query(",
    "Store::update(",
    "as SparqlEvaluator",
    "as QueryResultsFormat",
];
const STORE_ENTRY_POINTS: &[&str] = &[
    ".query(",
    ".query_opt(",
    ".query_with_options(",
    ".update(",
    ".update_opt(",
];
const ONLY_IN_QUERY_MODULE: &[&str] = &["parse_update"];
const HANDLER: &str = ".without_default_http_service_handler()";

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

fn sources() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    rust_files(&root.join("src"), &mut files);
    assert!(files.len() >= 10, "found only {} source files", files.len());
    files
        .into_iter()
        .map(|p| {
            let name = p.strip_prefix(root).unwrap().to_string_lossy().into_owned();
            (name, std::fs::read_to_string(&p).unwrap())
        })
        .collect()
}

/// The text without `//` comments (a `//` inside a string literal is not a comment).
fn code(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let mut in_string = false;
        let mut escaped = false;
        let mut cut = line.len();
        let bytes: Vec<(usize, char)> = line.char_indices().collect();
        for (n, &(at, c)) in bytes.iter().enumerate() {
            if in_string {
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    in_string = false;
                }
            } else if c == '"' {
                in_string = true;
            } else if c == '/' && bytes.get(n + 1).is_some_and(|&(_, d)| d == '/') {
                cut = at;
                break;
            }
        }
        out.push_str(&line[..cut]);
        out.push('\n');
    }
    out
}

/// The byte offsets of every `SparqlEvaluator::new()` whose statement does not call
/// `without_default_http_service_handler()`. The statement ends at the first `;` outside any
/// bracket and string, or when a bracket closes that the statement did not open.
fn unpaired(text: &str) -> Vec<usize> {
    let text = code(text);
    let needle = "SparqlEvaluator::new()";
    let mut bad = Vec::new();
    let mut from = 0;
    while let Some(found) = text[from..].find(needle) {
        let start = from + found;
        let rest = &text[start + needle.len()..];
        let mut depth = 0i32;
        let mut in_string = false;
        let mut escaped = false;
        let mut end = rest.len();
        for (at, c) in rest.char_indices() {
            if in_string {
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    in_string = false;
                }
                continue;
            }
            match c {
                '"' => in_string = true,
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => {
                    depth -= 1;
                    if depth < 0 {
                        end = at;
                        break;
                    }
                }
                ';' if depth == 0 => {
                    end = at;
                    break;
                }
                _ => {}
            }
        }
        let statement: String = rest[..end].split_whitespace().collect();
        if !statement.contains(HANDLER) {
            bad.push(start);
        }
        from = start + needle.len();
    }
    bad
}

/// The banned names that `text` (of the file `path`) uses.
fn banned(path: &str, text: &str) -> Vec<&'static str> {
    let text = code(text);
    let in_query_module = path.starts_with("src/core/query");
    let mut found = Vec::new();
    for name in ALWAYS_BANNED {
        if text.contains(name) {
            found.push(*name);
        }
    }
    if !in_query_module {
        for name in ONLY_IN_QUERY_MODULE {
            if text.contains(name) {
                found.push(*name);
            }
        }
    }
    found
}

/// The store-entry-point calls in `text`: `.query(` and its variants anywhere (nothing in hali
/// has a method of that name), and `.update(` and `.update_opt(` when the receiver is a store
/// (an identifier holding `store`, a copy of one, or the result of a call).
fn store_calls(text: &str) -> Vec<String> {
    let text = code(text);
    let mut found = Vec::new();
    for name in STORE_ENTRY_POINTS {
        let mut from = 0;
        while let Some(at) = text[from..].find(name) {
            let at = from + at;
            from = at + name.len();
            let receiver: String = text[..at]
                .chars()
                .rev()
                .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == ')')
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            let lower = receiver.to_lowercase();
            let store = name.starts_with(".query")
                || lower.ends_with(')')
                || lower.contains("store")
                || matches!(lower.as_str(), "merged" | "copy" | "db" | "graph");
            if store {
                found.push(format!("{receiver}{name}"));
            }
        }
    }
    found
}

fn line_of(text: &str, offset: usize) -> usize {
    code(text)[..offset].matches('\n').count() + 1
}

#[test]
fn every_evaluator_is_built_without_the_http_service_handler() {
    let sources = sources();
    let mut seen = 0;
    for (path, text) in &sources {
        seen += text.matches("SparqlEvaluator::new()").count();
        if let Some(&at) = unpaired(text).first() {
            panic!(
                "{path}:{}: SparqlEvaluator::new() is not followed by \
                 .without_default_http_service_handler() (a SERVICE clause could reach the network)",
                line_of(text, at)
            );
        }
    }
    assert!(seen >= 4, "expected the existing evaluators, found {seen}");
}

#[test]
fn src_never_calls_the_stores_own_query_or_update() {
    for (path, text) in sources() {
        let found = store_calls(&text);
        assert!(
            found.is_empty(),
            "{path} calls {found:?}: a store's own query and update use an evaluator with the \
             default HTTP service handler; build one with SparqlEvaluator::new() and \
             .without_default_http_service_handler()"
        );
    }
}

#[test]
fn src_never_names_xml_or_rdf_xml_formats() {
    for (path, text) in sources() {
        let found = banned(&path, &text);
        assert!(
            found.is_empty(),
            "{path} names {found:?}: SPARQL XML and RDF/XML must stay unreachable \
             (quick-xml, .cargo/audit.toml), and an UPDATE is only parsed to be refused in \
             src/core/query"
        );
    }
}

/// The scanners find what they are there to find.
#[test]
fn the_scanners_catch_violations() {
    let paired = "let q = SparqlEvaluator::new()\n    .without_default_http_service_handler()\n    .parse_query(\"ASK {}\")\n    .unwrap();";
    assert!(unpaired(paired).is_empty());
    let bare = "let q = SparqlEvaluator::new().parse_query(\"ASK {}\").unwrap();";
    assert_eq!(unpaired(bare).len(), 1);
    let other_statement =
        "let e = SparqlEvaluator::new();\nlet f = e.without_default_http_service_handler();";
    assert_eq!(unpaired(other_statement).len(), 1);
    let in_a_string = "let e = SparqlEvaluator::new().without_default_http_service_handler().parse_query(\"a;b\").unwrap();";
    assert!(
        unpaired(in_a_string).is_empty(),
        "a `;` in a string ends nothing"
    );
    let in_a_string_bare = "let e = SparqlEvaluator::new().parse_query(\"a;b\").unwrap();\nlet h = \".without_default_http_service_handler()\";";
    assert_eq!(unpaired(in_a_string_bare).len(), 1);
    let two = format!("{paired}\n{bare}");
    assert_eq!(unpaired(&two).len(), 1);
    let comment_only = "// SparqlEvaluator::new() is how it starts\nlet x = 1;";
    assert!(unpaired(comment_only).is_empty());
    let in_call =
        "run(SparqlEvaluator::new().without_default_http_service_handler().parse_query(q))";
    assert!(unpaired(in_call).is_empty());
    let in_call_bare = "run(SparqlEvaluator::new().parse_query(q))";
    assert_eq!(unpaired(in_call_bare).len(), 1);

    assert_eq!(
        banned(
            "src/core/x.rs",
            "use oxigraph::sparql::results::QueryResultsFormat; let f = QueryResultsFormat::Xml;"
        ),
        ["QueryResultsFormat::Xml"]
    );
    assert_eq!(
        banned("src/core/x.rs", "let f = RdfFormat::RdfXml;"),
        ["RdfFormat::RdfXml", "RdfXml"]
    );
    assert_eq!(
        banned("src/core/x.rs", "QueryResultsParser::from_format(f)"),
        ["QueryResultsParser"]
    );
    assert_eq!(
        banned("src/core/x.rs", "RdfParser::from_format(f)"),
        ["RdfParser"]
    );
    assert_eq!(
        banned("src/core/x.rs", "e.parse_update(text)"),
        ["parse_update"]
    );
    assert!(banned("src/core/query.rs", "e.parse_update(text)").is_empty());
    assert!(banned("src/core/query/mod.rs", "e.parse_update(text)").is_empty());
    assert_eq!(
        banned("src/core/query.rs", "RdfParser::from_format(f)"),
        ["RdfParser"],
        "the query module may parse an update, nothing else"
    );
    assert!(
        banned(
            "src/core/x.rs",
            "// never QueryResultsFormat::Xml, RdfParser or parse_update"
        )
        .is_empty()
    );
    assert!(banned("src/core/x.rs", "let json = QueryResultsFormat::Json;").is_empty());
    // The ways around the handler or into the XML code.
    for name in [
        "SparqlEvaluator::default()",
        "RdfSerializer",
        "QueryResultsSerializer",
        "QueryResultsFormat::from_extension",
        "QueryResultsFormat::from_media_type",
        "oxrdfxml",
        "sparesults",
        "quick_xml",
        "RdfFormat::from_extension",
        "RdfFormat::from_media_type",
        "Store::query(",
        "Store::update(",
        "as SparqlEvaluator",
        "as QueryResultsFormat",
    ] {
        assert_eq!(
            banned("src/core/x.rs", &format!("let x = {name};")),
            [name],
            "{name}"
        );
        assert_eq!(
            banned("src/core/query.rs", &format!("let x = {name};")),
            [name],
            "{name}: not even the query module may name it"
        );
        assert!(banned("src/core/x.rs", &format!("// {name}")).is_empty());
    }
    assert_eq!(
        banned("src/cli/mod.rs", "use sparesults::QueryResultsSerializer;"),
        ["QueryResultsSerializer", "sparesults"]
    );

    // The store's own entry points.
    assert_eq!(store_calls("let r = store.query(text);"), ["store.query("]);
    assert_eq!(store_calls("self.store.update(text)?;"), ["store.update("]);
    assert_eq!(
        store_calls("merged.update(u).unwrap();"),
        ["merged.update("]
    );
    assert_eq!(
        store_calls("Store::new().unwrap().update(u)"),
        [").update("]
    );
    assert_eq!(
        store_calls("s.query_with_options(q, o)"),
        ["s.query_with_options("]
    );
    assert!(store_calls("hasher.update(&buf[..n]);").is_empty());
    assert!(store_calls("progress.update(walked, 0);").is_empty());
    assert!(store_calls("// store.query(text)").is_empty());
    assert!(store_calls("evaluator.parse_query(text)").is_empty());
}
