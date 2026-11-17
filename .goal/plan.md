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
- Row 20: `.goal/checks/03-existing_goldens_unchanged.sh`: no existing golden changes; the three schema-list snapshots (`cli_schema__`, `cli_scan__`, `cli_relink__schema_list_json`) may gain exactly one `"query",`.
- Row 21: `.goal/checks/04-dry_run_guard_query.sh`: `tests/dry_run_guard.rs` has a `query` row in `CASES`.
- Extra tests inside rows: `cli_query::{construct_is_sorted_like_the_store, construct_empty_is_valid_turtle, select_json_terms, read_only_query_leaves_the_store_byte_identical, query_is_parsed_after_the_config_and_before_the_store}`, `core_query::{select_cells_and_unbound, select_uses_a_query_prefix_over_the_table, construct_template_blank_nodes_are_fresh, syntax_errors_are_usage_errors, a_comment_is_not_an_update}`, `schema_contract::query_schema_is_listed`.
- `tests/schema_contract.rs::relink_schema_is_listed` asserted exactly 26 names; it now asserts at least 26 (goal commit, because the file is locked).

## Assumed APIs (fixed by the goal tests; see each file's header)
`core::query::{parse(&str) -> Result<Prepared, Error>, run(&Store, &Prepared, Limits) -> Result<Answer, Error>}`; `Limits { timeout: Duration, max_results: usize }` (`Default` = 30 s, 100,000; `Clone + Copy + Debug + PartialEq`); `Answer::{Solutions { variables: Vec<String>, rows: Vec<Vec<Option<oxrdf::Term>>> }, Boolean(bool), Triples(Vec<oxrdf::Triple>)}` (`Clone + Debug + PartialEq`); `Error::{read_only_query, query_failed}` constructors are the implementer's. `hali query '<sparql>'`: one positional argument.

## Wording the goal fixes (flagged to the human)
- `suggested_fix` of `READ_ONLY_QUERY`: `Use SELECT, ASK, CONSTRUCT or DESCRIBE to read the store, or change it with hali's own commands.` (locked snapshot); `suggested_command` null.
- Text SELECT cells: N-Triples form, IRIs shortened to `prefix:local` through the prefix table when the local part is a plain name, else `<iri>`; literals quoted with the §9.3 escapes applied to the lexical form (ESC prints `\u{1b}`, not the N-Triples `\u001B`).
- Header-only output for a SELECT with no solution; `SELECT *` variable order is the evaluator's (tests compare it as a set).
- Turtle escapes are `\uXXXX`/`\UXXXXXXXX` (case not fixed); the layout TAB of the serializer stays raw.
- A timeout test needs a query whose work is store scans (cancellation is only checked there): three cross-joined patterns with a `FILTER NOT EXISTS` over the store.
