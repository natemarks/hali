# Plan for #9 `hali query` (approved)

The approved contract is the "Groomed contract (approved)" section of issue #9. All nine open questions took the planner's default (set-merge default graph; new rule `QUERY_FAILED` with reasons service/timeout/too_large; fixed 30 s and 100,000 bounds; predeclared prefixes; Turtle in text mode for CONSTRUCT/DESCRIBE; syntax error is usage exit 2; `SERVICE SILENT` follows SPARQL semantics; accept the parser stack-overflow risk with a follow-up; argument only).

Not cross-cutting: implementer on its default model. Security-sensitive (the SPARQL query path, `.cargo/audit.toml`; touches `src/storage/` Turtle helper): security-reviewer required.

## Seams
- `core::query` (new): `parse(text) -> Result<Prepared, Error>`, `run(store, &Prepared, Limits) -> Result<Answer, Error>` (Answer = Solutions | Boolean | Triples); `Error::{read_only_query, query_failed}`; limits injected for tests.
- CLI `Command::Query { query }` (`assert_cmd` + insta in `tests/cli_query.rs`, `tests/snapshots/cli_query__*`).
- `hali schema query` (`schemas/query.json`, `SCHEMAS` 26 to 27), `error.json` branches; `tests/schema_contract.rs`.
- A test-local `TcpListener` on 127.0.0.1 proves no connection is attempted.
- Turtle output cannot be an insta snapshot (schema_contract requires every `.snap` to be JSON): exact assertions instead.

## Goal map
| # | Criterion | Check |
|---|---|---|
| 1 | SELECT `--json` prints W3C JSON (golden: the 4 standard relations from the core graph, ORDER BY) | `cli_query::select_json` + locked `cli_query__select_json.snap` |
| 2 | ASK `--json` prints `{"head":{},"boolean":true}` | `cli_query::ask_json` + locked snap |
| 3 | SELECT, ASK, CONSTRUCT, DESCRIBE all evaluate | `core_query::{select, ask, construct, describe}` |
| 4 | CONSTRUCT prints sorted Turtle that parses back to the same triples, identical with and without `--json`; DESCRIBE `<concept>` includes its prefLabel | `cli_query::{construct_turtle, describe_turtle}` |
| 5 | Turtle has no raw §9.3 characters (U+202E, U+0007 in a literal, U+FEFF in an IRI) and still parses | `cli_query::turtle_escaped` |
| 6 | Text SELECT TSV header and rows; ASK `true`; ESC comes out as `\u{1b}` | `cli_query::{select_text, ask_text, text_escaped}` |
| 7 | Default graph is a set merge; `GRAPH <.../extensions>` sees only extensions; `FROM` restricts | `core_query::{dataset_merge, graph_and_from}` |
| 8 | Every UPDATE form (INSERT DATA, DELETE WHERE, LOAD <file:///etc/passwd>, CLEAR ALL, multi-op) is exit 1 `READ_ONLY_QUERY`; store byte-identical; no data dir created | `core_query::update_refused`, `cli_query::read_only_query` + locked stderr golden `cli_query__read_only_query_json_envelope.snap` |
| 9 | `# INSERT DATA {}` then `ASK {}` runs and prints `true` | `cli_query::comment_not_update` |
| 10 | Bad SPARQL is exit 2 `invalid SPARQL query`, escaped in text | `cli_query::syntax_error` |
| 11 | `SERVICE <http://127.0.0.1:PORT/>` is `QUERY_FAILED`/service with no connection; `SERVICE SILENT` exits 0 with no connection; `FROM <http://127.0.0.1:PORT/>` makes no connection | `core_query::service_offline`, `cli_query::service_never_connects` |
| 12 | Timeout and result cap give `QUERY_FAILED` timeout and too_large; defaults 30 s and 100,000 | `core_query::{timeout, too_large, default_limits}` |
| 13 | No `--limit` flag | `cli_query::no_limit_flag` |
| 14 | `--dry-run` and `--yes` change nothing (success and `READ_ONLY_QUERY`) | `cli_query::dry_run_identical` |
| 15 | Absent data dir: exit 0, core graph, nothing created; unparseable store: `STORE_UNPARSEABLE` exit 3 | `cli_query::{absent_store, unparseable_store}` |
| 16 | `hali schema` lists `query`; accepts goldens 1-2, rejects an envelope or missing `head`; error branches exist | `schema_contract::{query_schema_accepts_and_rejects, query_error_branches}` |
| 17 | Every `SparqlEvaluator::new()` in `src/` paired with `without_default_http_service_handler()`; `src/` never names XML result or RDF/XML formats or parsers | `tests/sparql_guard.rs` (permanent source scan; may pass today) |
| 18 | Spec §9.9/§9.4 updated | `.goal/checks/01-spec_query.sh` |
| 19 | `.cargo/audit.toml` re-checked | `.goal/checks/02-audit_query.sh` |

Guards: existing goldens unchanged (`hali schema` list changes only by the new `query` name, handled in schema_contract); existing commands unchanged.

Tests set HOME, XDG_STATE_HOME and the data dir to temp dirs. Output order needs ORDER BY.

## Added by the goal (define)
- Row 20a: `.goal/checks/03-existing_goldens_unchanged.sh`: no existing golden changes; the three schema-list snapshots (`cli_schema__`, `cli_scan__`, `cli_relink__schema_list_json`) may gain exactly one `"query",`.
- Row 20b: `.goal/checks/04-dry_run_guard_query.sh`: `tests/dry_run_guard.rs` has a `query` row in `CASES`.
- Extra tests inside rows: `cli_query::{construct_is_sorted_like_the_store, construct_empty_is_valid_turtle, select_json_terms, read_only_query_leaves_the_store_byte_identical, query_is_parsed_after_the_config_and_before_the_store}`, `core_query::{select_cells_and_unbound, select_uses_a_query_prefix_over_the_table, construct_template_blank_nodes_are_fresh, syntax_errors_are_usage_errors, a_comment_is_not_an_update}`, `schema_contract::query_schema_is_listed`.
- `tests/schema_contract.rs::relink_schema_is_listed` asserted exactly 26 names; it now asserts at least 26 (goal commit, because the file is locked).

## Added by the update (review of #9: reviewer and security-reviewer)
Decisions taken (delegated approval): an evaluation error that the user's text can trigger is a usage error; empty or comment-only text is a usage error. (The earlier "Row 20/21" of the define section are 20a and 20b.)

| # | Finding | Check | Fails today because |
|---|---|---|---|
| 21 | F1, security F9 (blocking): the 30 s limit fires for every query, the sort phase included | `core_query::{timeout_covers_a_query_that_never_reads_the_store, timeout_covers_the_sort_of_a_cross_join}`, `cli_query::a_quick_query_does_not_wait_for_the_time_limit` (mutation proof for `drop(done)`; passes today) | cancellation is only checked in store scans: a billion-solution VALUES cross join runs on; the million-row sort ends normally. There is no test-hooks way to shorten the 30 s limit, so no CLI test of a timed-out query exiting |
| 22 | F2, security 4 (blocking): evaluation errors a user can trigger are usage errors, never STORE_IO | `core_query::unsupported_custom_functions_are_usage_errors`, `cli_query::unsupported_custom_functions_are_usage_errors` | an unsupported custom function (any arity, in FILTER or an expression) is STORE_IO, exit 3, text about the data dir |
| 23 | F3, security 2 (blocking): CONSTRUCT/DESCRIBE output is always valid Turtle | `cli_query::{turtle_blank_node_labels_with_unsafe_characters, turtle_bnode_function_and_triple_terms_are_valid}`; `turtle_stand_in_iris_in_every_position` (subject, predicate, object, datatype; passes today) | a blank node label (extensions, `BNODE(...)`) or a triple-term IRI with U+FEFF, U+061C or a tag character is written with a `\u` escape inside `_:label` / `<<( )>>`, which is not Turtle |
| 24 | F4: empty, comment-only and PREFIX-only text | `core_query::empty_text_is_a_usage_error`, `cli_query::empty_text_is_a_usage_error` | they parse as an empty SPARQL Update and give READ_ONLY_QUERY, exit 1 |
| 25 | coverage: mutations of cell_text, result_term, stand-ins, marker, F5, SERVICE SILENT, `drop(done)` | `cli_query::{text_literal_cells, text_blank_node_cells, json_triple_term, turtle_stand_in_iris_in_every_position, turtle_stand_in_marker_avoids_the_data, service_variables_and_silent_ask, a_quick_query_does_not_wait_for_the_time_limit}`, `core_query::service_variables_are_service_failures` | all pass today by design; each kills its mutation (evidence in the report) |
| 26 | F6: the text triple-term cell is `<<( s p o )>>` | `cli_query::text_triple_term_cells` | the cell is `<< s p o >>` |
| 27 | security 3: source guard | `tests/sparql_guard.rs::{the_scanners_catch_violations, src_never_calls_the_stores_own_query_or_update}` | passes today (src is clean); the scanners now catch `SparqlEvaluator::default()`, `RdfSerializer`, `QueryResultsSerializer`, `QueryResultsFormat::from_extension`/`from_media_type`, `oxrdfxml`, `sparesults`, `quick_xml` and `.query(`/`.update(` on a store. Nothing is allow-listed: hali writes its JSON results by hand |

Checks extended: `.goal/checks/01-spec_query.sh` (F9, F4/F2, F10: `except LF and TAB`; empty or comment-only text and unsupported custom function evaluation errors are usage errors; CONSTRUCT/DESCRIBE blank node labels are not stable across runs) and `.goal/checks/02-audit_query.sh` (F8: the `hali query` paragraph of `.cargo/audit.toml` comes after the quick-xml reasoning ending "cannot be bumped from here.").

## Assumed APIs (fixed by the goal tests; see each file's header)
`core::query::{parse(&str) -> Result<Prepared, Error>, run(&Store, &Prepared, Limits) -> Result<Answer, Error>}`; `Limits { timeout: Duration, max_results: usize }` (`Default` = 30 s, 100,000; `Clone + Copy + Debug + PartialEq`); `Answer::{Solutions { variables: Vec<String>, rows: Vec<Vec<Option<oxrdf::Term>>> }, Boolean(bool), Triples(Vec<oxrdf::Triple>)}` (`Clone + Debug + PartialEq`); `Error::{read_only_query, query_failed}` constructors are the implementer's. `hali query '<sparql>'`: one positional argument.

## Wording the goal fixes (flagged to the human)
- `suggested_fix` of `READ_ONLY_QUERY`: `Use SELECT, ASK, CONSTRUCT or DESCRIBE to read the store, or change it with hali's own commands.` (locked snapshot); `suggested_command` null.
- Text SELECT cells: N-Triples form, IRIs shortened to `prefix:local` through the prefix table when the local part is a plain name, else `<iri>`; literals quoted with the §9.3 escapes applied to the lexical form (ESC prints `\u{1b}`, not the N-Triples `\u001B`).
- Header-only output for a SELECT with no solution; `SELECT *` variable order is the evaluator's (tests compare it as a set).
- Turtle escapes are `\uXXXX`/`\UXXXXXXXX` (case not fixed); the layout TAB of the serializer stays raw.
- A timeout test needs a query whose work is store scans (cancellation is only checked there): three cross-joined patterns with a `FILTER NOT EXISTS` over the store.
