# Plan for #8: `--dry-run` for every mutating command

Approved by the human with all planner recommendations (open questions 1-6).

## Decisions
1. Text mode: `would add concept <id> "<label>"`, then one `+ <triple>` / `- <triple>` line per change (all through `printable()`), stderr `note: dry run; nothing was written`, exit 0.
2. `--dry-run` on read-only commands (`concept list`, `validate`, later `find`, `show`, `query`): accepted and ignored. Output, stderr and exit code are byte-identical to a run without it, no `dry_run` key.
3. `changes` order: `add` before `remove`, then the whole triple string in plain byte order (Rust `str` order). The `>` closing the subject IRI takes part, so `<...#topics-2>` sorts before `<...#topics>` and `<...#ml2>` before `<...#ml>`. `rdf:type` is NOT forced first. No duplicates.
4. A change has exactly the keys `op` and `triple` (no `file`).
5. Test hooks after settle (`make unit` is plain `cargo test` without `test-hooks`): deferred to #10 (comment posted there). Does not affect #8: `concept add` mints no UUID or timestamp.
6. A dry run does not check that the data dir is writable, so it may succeed where the real run would fail with `STORE_IO`.
7. The JSON Schema criterion moves to #6 (comment posted there); whichever PR lands second adds the schemas.

## Contract
- `--dry-run` is a global clap flag like `--json`, accepted before or after the subcommand; help text: "Show the triple changes a mutating command would make, and write nothing". Today it is a usage error (exit 2) on every command. `concept add` is the only mutating command.
- JSON payload: same keys as a real run in the same order, then `"dry_run":true`, then `"changes":[...]`. For `concept add`: `{"schema_version":1,"concept":{"id","label","alt_labels","broader"},"dry_run":true,"changes":[...]}`. `changes` is always present and `[]` when nothing would change. A real run never prints `dry_run` or `changes`; `schema_version` stays 1 (additive).
- `changes`: one `{"op":"add"|"remove","triple":T}` per triple the real run would add to or remove from a tool-managed file (`vocabulary.ttl`, `resources.ttl`). A triple already in the file (graph) it goes to is not listed: a first `concept add` lists the scheme triple, later adds do not; if the scheme triple is only in `extensions.ttl` (another graph) it IS listed, because the real run writes it into `vocabulary.ttl`. `T` is canonical N-Triples exactly as oxrdf prints it, plus ` .`: full IRIs in `<>`, no prefixes; simple (xsd:string) literals with no datatype, other datatypes as `^^<iri>`, language tags lowercase; escapes `\" \\ \t \n \r \b \f` and `\uXXXX` (uppercase hex) for other C0 controls, DEL, U+FFFE and U+FFFF; everything else raw UTF-8 (bidi characters stay raw in JSON). Do not render triples with `compact`.
- Writes nothing: no data dir created (even when it does not exist yet), no file created/modified/deleted, no temp or lock file; directory listing and every file's bytes unchanged. Locking (#5) is not built.
- Errors: a dry run reports exactly the errors a real run would, with byte-identical stderr (JSON envelope or `error:`/`hint:` lines) and the same exit code. Error envelopes carry no `dry_run` or `changes`. Order unchanged: clap/normalise (exit 2) -> config (`CONFIG_INVALID` 3) -> load (`STORE_IO`/`STORE_UNPARSEABLE` 3) -> `LABEL_TAKEN` (1) -> SHACL (`SHACL_VIOLATION` 1, `SHAPES_INVALID` 3). The `SHACL_VIOLATION` message "...; nothing was written" stays as is.
- Exit codes: a successful dry run exits 0 even though a real run would modify the store; usage errors with `--dry-run` exit 2 as without it.
- Ids and clock: the dry run calls `Clock`/`IdGen` exactly as the real run (same code); nothing it mints is persisted or reserved. The Concept id is the same as a real run's (slug-derived, deterministic).
- Out of scope: `scan --dry-run` summary, `skill install --dry-run`, `--help` examples of §11.1, the lock file and `STORE_LOCKED` (#5), `hali schema` (#6), SHACL on removals and any core code that produces `remove` (the first ticket that removes), the `DRY_RUN=1` release targets, new error rules, new dependencies.
- Constraint: a dry run must never write; the skill pre-approves `--dry-run` invocations without a permission prompt, so a writing dry run would bypass that prompt. `core` stays unaware of dry-run.
- Spec §9.5 gets the rules above (enforced by `.goal/checks/docs.sh`).

## Seams
- `hali::core::concept::add(&Context, &mut Store, &str) -> Result<Mutation<Concept>, Error>`: the change set, its order and escaping; the in-memory store diff equals `changes`.
- `hali::core::{Change, Op, Mutation}`: `Op {Add, Remove}` serialises lowercase, `Op::Add < Op::Remove`; `Change {op, triple: String}` (field order = sort order) serialises `{"op":"add","triple":"..."}`; `Mutation<T> {value: T, changes: Vec<Change>}`. Tests import them from `hali::core`.
- The `hali` binary through `assert_cmd`: payload, text output, flag position, read-only commands, errors, and that nothing is written in the data dir.
- `tests/support/mod.rs::assert_dry_run_matches_real_run`: §14.3 equality between the dry run and the real run, on the files.
- `docs/spec.md` §9.5 and clippy/dead-code with the goal feature on: checks.

## Reusable helper (tests/support/mod.rs, a plain module, not a test target)
Exactly one `pub fn`, so every private helper is reachable from any file that uses it (dead-code clean in both builds without `#[allow]`). Callers declare `#[cfg(feature = "goal")]` + `mod support;`; settle strips the tag and later tickets declare `mod support;` and call it.
```rust
/// Runs `hali --json --dry-run <args>` and `hali --json <args>`, each in a fresh temp env
/// (data dir = copy of the files in `store`, or absent for None; XDG_CONFIG_HOME/HOME empty
/// temp dirs; env_clear; HALI_TEST_NOW=2026-10-24T09:30:00Z, HALI_TEST_ID_SEED=1), and asserts:
/// both exit 0 with stderr empty and one JSON line; the dry run left the data dir as it was
/// (absent stays absent; same names and bytes); dry JSON == real JSON + `dry_run: true` +
/// `changes`, these two being the last keys in that order; each change has exactly keys
/// op ("add"|"remove") and triple; `changes` is strictly sorted by (op, triple); each triple
/// parses (oxttl NTriplesParser) as exactly one triple whose `format!("{t} .")` is the string;
/// the add/remove sets equal the union over vocabulary.ttl and resources.ttl of
/// (after - before) / (before - after), comparing parsed `oxrdf::Triple` sets (so a
/// formatting-only rewrite counts as no change); every other file is byte-identical after the
/// real run; no blank node occurs in either file set or in `changes` (panics if one does).
/// Returns the dry run's JSON.
pub fn assert_dry_run_matches_real_run(store: Option<&Path>, args: &[&str]) -> serde_json::Value
```

## Goal map
All goal items carry `#[cfg(feature = "goal")]` on their own line: every `#[test]`, the `mod support;` line, and every `use`/helper in the goal files.

| # | Criterion | Check | Expected failure before implementation |
|---|---|---|---|
| 1 | Empty store, `concept add "machine learning" --dry-run --json` -> exit 0, stderr empty, stdout = snapshot A | `tests/cli_dry_run.rs::concept_add_dry_run_json` | exit 2 (unexpected argument `--dry-run`) |
| 1 | `store` fixture (scheme present), dry-run adding "deep learning": 3 Concept triples, no scheme triple (snapshot B) | `cli_dry_run.rs::concept_add_dry_run_json_existing_scheme` | exit 2 != 0 |
| 1 | `changes` follows the byte-order rule across subjects (`topics-2` before `topics`, snapshot C) | `cli_dry_run.rs::concept_add_dry_run_json_suffix_order` | exit 2 != 0 |
| 1 | core: first add gives exactly 4 `Change`s, in order, all `Op::Add` | `tests/core_dry_run.rs::add_reports_changes_for_a_first_add` | unresolved imports `hali::core::{Change, Op, Mutation}` |
| 1 | core: triples already in the file are omitted | `core_dry_run.rs::add_omits_triples_already_in_the_file` (store fixture -> 3) | same compile error |
| 1 | core: scheme triple listed when only `extensions.ttl` has it | `core_dry_run.rs::add_lists_the_scheme_when_only_another_graph_has_it` (fixture `dry-run/scheme-in-extensions`) | same |
| 1 | core: `changes` equals the in-memory diff of the vocabulary graph (after - before, as `format!("{triple} .")`) | `core_dry_run.rs::changes_match_the_store_diff` | same |
| 2 | Missing data dir, dry-run `concept add` (json and text) -> data dir still does not exist | `cli_dry_run.rs::dry_run_creates_no_data_dir` | exit 2 != 0 |
| 2 | `store` and `noncanonical` fixtures, dry run repeated twice -> listing and every file's bytes unchanged, both outputs identical | `cli_dry_run.rs::dry_run_leaves_store_bytes_unchanged` | exit 2 != 0 |
| 3 | `shacl/extension-shape`, `concept add "deep learning" --dry-run --json` -> exit 1, rule SHACL_VIOLATION, stderr byte-equal to the real run's, files unchanged | `cli_dry_run.rs::shacl_violation_reported_in_dry_run` | exit 2 != 1 |
| 3 | Every error is the real run's, json and text: LABEL_TAKEN (`store`, "machine learning"), SHAPES_INVALID (`shacl/extension-panics`, `shacl/extension-node-core-shape`, "brand new"), STORE_UNPARSEABLE (broken vocabulary.ttl), STORE_IO (data dir is a file): (code, stdout, stderr) equal and nothing written | `cli_dry_run.rs::dry_run_errors_match_the_real_run` | dry-run code 2 != real 1/3 |
| 3 | A core refusal still returns `Err` and leaves the store unchanged | existing `core_concept.rs::duplicate_label_is_label_taken`, `core_shacl.rs` refusals (call sites updated) | stay green |
| 4 | Reusable helper: dry-run `changes` equal exactly what a real run writes | `cli_dry_run.rs::dry_run_changes_equal_real_run_writes` calling `support::assert_dry_run_matches_real_run` with: (None, "machine learning"), (`store`, "deep learning"), (`noncanonical`, "deep learning"), (`dry-run/scheme-in-extensions`, "machine learning"), (None, "topics"), (None, `say "hi" \ now`), (None, `Straße 日本`) | exit 2 inside the helper |
| 5 | Spec §14.1: core test first, assert_cmd test, insta snapshot; no new error rules, so no triggering test | covered by rows 1-4 | - |
| 6 | Text mode: exact stdout (`would add concept ...` + `+ ` lines) and the stderr note | `cli_dry_run.rs::concept_add_dry_run_text` | exit 2 != 0 |
| 6 | Text mode escapes labels: label `abc\u{202E}def` gives `\u{202e}` in both the summary line and the `+` line, no raw U+202E on stdout; JSON keeps it raw | `cli_dry_run.rs::dry_run_text_escapes_labels` | exit 2 != 0 |
| 7 | `--dry-run` global and position-free: `hali --dry-run concept add x --json` == `hali concept add x --dry-run --json` | `cli_dry_run.rs::dry_run_flag_position_is_free` | exit 2 != 0 |
| 7 | Read-only commands ignore it: `concept list` and `validate` (json and text, `store` fixture and missing store): output identical to without the flag, no dir created | `cli_dry_run.rs::dry_run_on_read_only_commands_changes_nothing` | code 2 != 0 |
| 7 | Usage errors stay exit 2: `concept add "!!!" --dry-run`, `hali --dry-run` (no subcommand); nothing written | `cli_dry_run.rs::dry_run_usage_errors_exit_2` | likely passes before (exit 2 either way): a guard |
| 7 | `hali concept add --help` stdout contains `--dry-run` | `cli_dry_run.rs::help_lists_dry_run` | assertion fails |
| 8 | Canonical N-Triples escaping: label `say "hi" \ now` gives `<https://w3id.org/hali#say-hi-now> <http://www.w3.org/2004/02/skos/core#prefLabel> "say \"hi\" \\ now"@en .` | `core_dry_run.rs::changes_escape_literals_as_n_triples` | compile error |
| 8 | Serialisation and order: `Change{op:Op::Add,..}` -> `{"op":"add","triple":"..."}`, `Op::Remove` -> `"remove"`, `Op::Add < Op::Remove`; the `topics` add gives `topics-2` triples first; `changes` sorted | `core_dry_run.rs::change_serializes_as_op_and_triple`, `::changes_are_sorted_by_op_then_triple` | compile error |
| 9 | Real-run payload unchanged (no `dry_run`/`changes`) | existing `cli_concept.rs::concept_add_json` snapshot, and the helper's payload-equality assertion | stay green |
| 10 | Spec §9.5 records: global flag, read-only commands ignore it, `add` before `remove`, "byte order", text format, `note: dry run; nothing was written`, errors identical | `.goal/checks/docs.sh` (awk §9.5 out of docs/spec.md, grep for `byte order`, `read-only`, `note: dry run; nothing was written`, `add` before `remove`) | `docs: spec §9.5 does not state the order of changes` |
| 11 | Goal tests and helper are lint- and dead-code-clean with the goal feature on, so settle cannot break the hook | `.goal/checks/lint-goal.sh`: `cargo clippy --all-targets --features test-hooks,goal -- -D warnings` and `RUSTFLAGS="-D dead_code" cargo check --all-targets --features test-hooks,goal` | fails while goal tests do not compile |

### Hand-written locked snapshots
Each file has the header `---\nsource: tests/cli_dry_run.rs\nexpression: out.stdout\n---\n` followed by one line; every test calls `insta::assert_snapshot!("<name>", out.stdout)`.

`tests/snapshots/cli_dry_run__concept_add_dry_run_json.snap` (snapshot A, empty store, "machine learning"):
```
{"schema_version":1,"concept":{"id":"machine-learning","label":"machine learning","alt_labels":[],"broader":[]},"dry_run":true,"changes":[{"op":"add","triple":"<https://w3id.org/hali#machine-learning> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.w3.org/2004/02/skos/core#Concept> ."},{"op":"add","triple":"<https://w3id.org/hali#machine-learning> <http://www.w3.org/2004/02/skos/core#inScheme> <https://w3id.org/hali#topics> ."},{"op":"add","triple":"<https://w3id.org/hali#machine-learning> <http://www.w3.org/2004/02/skos/core#prefLabel> \"machine learning\"@en ."},{"op":"add","triple":"<https://w3id.org/hali#topics> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.w3.org/2004/02/skos/core#ConceptScheme> ."}]}
```
`tests/snapshots/cli_dry_run__concept_add_dry_run_json_existing_scheme.snap` (snapshot B, `store` fixture, "deep learning"):
```
{"schema_version":1,"concept":{"id":"deep-learning","label":"deep learning","alt_labels":[],"broader":[]},"dry_run":true,"changes":[{"op":"add","triple":"<https://w3id.org/hali#deep-learning> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.w3.org/2004/02/skos/core#Concept> ."},{"op":"add","triple":"<https://w3id.org/hali#deep-learning> <http://www.w3.org/2004/02/skos/core#inScheme> <https://w3id.org/hali#topics> ."},{"op":"add","triple":"<https://w3id.org/hali#deep-learning> <http://www.w3.org/2004/02/skos/core#prefLabel> \"deep learning\"@en ."}]}
```
`tests/snapshots/cli_dry_run__concept_add_dry_run_json_suffix_order.snap` (snapshot C, empty store, "topics", id `topics-2`):
```
{"schema_version":1,"concept":{"id":"topics-2","label":"topics","alt_labels":[],"broader":[]},"dry_run":true,"changes":[{"op":"add","triple":"<https://w3id.org/hali#topics-2> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.w3.org/2004/02/skos/core#Concept> ."},{"op":"add","triple":"<https://w3id.org/hali#topics-2> <http://www.w3.org/2004/02/skos/core#inScheme> <https://w3id.org/hali#topics> ."},{"op":"add","triple":"<https://w3id.org/hali#topics-2> <http://www.w3.org/2004/02/skos/core#prefLabel> \"topics\"@en ."},{"op":"add","triple":"<https://w3id.org/hali#topics> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.w3.org/2004/02/skos/core#ConceptScheme> ."}]}
```
Text mode, exact output (empty store, "machine learning"). stdout:
```
would add concept machine-learning "machine learning"
+ <https://w3id.org/hali#machine-learning> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.w3.org/2004/02/skos/core#Concept> .
+ <https://w3id.org/hali#machine-learning> <http://www.w3.org/2004/02/skos/core#inScheme> <https://w3id.org/hali#topics> .
+ <https://w3id.org/hali#machine-learning> <http://www.w3.org/2004/02/skos/core#prefLabel> "machine learning"@en .
+ <https://w3id.org/hali#topics> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.w3.org/2004/02/skos/core#ConceptScheme> .
```
stderr: `note: dry run; nothing was written\n`

### New fixture
`tests/fixtures/dry-run/scheme-in-extensions/extensions.ttl` (no vocabulary.ttl):
```
@prefix hali: <https://w3id.org/hali#> .
@prefix skos: <http://www.w3.org/2004/02/skos/core#> .
hali:topics a skos:ConceptScheme .
```

### Locked paths (`.goal/locked`)
```
tests/core_dry_run.rs
tests/cli_dry_run.rs
tests/support/mod.rs
tests/fixtures/
tests/snapshots/
```
Checks: `.goal/checks/docs.sh` and `.goal/checks/lint-goal.sh`.

## Approach
1. Core types in `src/core/mod.rs`: `Op`, `Change`, `Mutation<T>` (derives as above) and `pub(crate) fn insert_all(store: &Store, quads: &[Quad]) -> Vec<Change>`: filters out quads `store.contains` already has (per graph; oxigraph `insert` does not report newness), inserts the rest, returns sorted deduped changes (`format!("{} .", TripleRef::from(quad))`). `Op::Remove` exists because spec §9.5 names it and the schema needs it; its first producer is a later ticket (no `remove_all` now).
2. `concept::add` returns `Result<Mutation<Concept>, Error>`; it keeps normalise -> LABEL_TAKEN -> id -> quads -> `validate::guard`, then calls `insert_all`. `core` never learns about dry-run: both paths run the same code, and the in-memory store is simply thrown away on a dry run.
3. `cli`: add `#[arg(long, global = true)] dry_run: bool` with the help text above; in `Concept Add` call `storage::save` only when `!cli.dry_run`; JSON via a generic `DryRunOutput<P> { #[serde(flatten)] payload: P, dry_run: bool, changes: &[Change] }`; text via a shared `print_changes(&changes)` writing `+ `/`- ` lines through `printable()` and the stderr note; read-only arms ignore the flag.
4. Update the 23 `concept::add` call sites in `tests/core_concept.rs` and `tests/core_shacl.rs` mechanically (`.unwrap()` -> `.unwrap().value`, `c.id` -> `c.value.id`; `is_err()`/`unwrap_err()` stay). These files are NOT locked; the implementer does this in the same commit as the signature change or `make unit` and the hook break. The reviewer checks that no assertion changed.
5. Spec §9.5: add the contract text.
Order of work: core types and `insert_all` (core tests) -> `concept::add` -> call sites -> cli flag, JSON and text -> spec.
Cross-cutting: no (three source files plus the spec and mechanical test edits, but it sets a shared contract); sonnet is enough for the implementer. Risks: the changes must not come from the graph-blind `quads` list (the scheme triple would then appear on every add; the `contains` check is per graph); do not sort by the serializer's key (rdf:type first); do not render triples with `compact`.

## Security
Sensitive: use the security-reviewer as well as the reviewer. The diff probably touches no listed path (`src/storage/` and `Cargo.toml` unchanged, no new deps), but the skill pre-approves `--dry-run` invocations without a permission prompt, so "a dry run writes nothing" is a permission boundary. New output carries user-derived text (labels in N-Triples): escaping is pinned (`\"`, `\\` in JSON and text) and text mode runs every line through `printable()` (bidi test). Errors under `--dry-run` must not leak more than the real run's (guaranteed by byte-equal stderr). Per the project memory, Turtle-file hardening stays low priority.
