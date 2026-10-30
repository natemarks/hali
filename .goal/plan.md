# Plan: #19 `hali describe`

Contract: see issue #19 ("Contract (groomed)"). Approved with all planner recommendations (open questions 1-13).

## Goal map

Fixtures: `R` = `tests/fixtures/relations` (reused, not copied: 5 Resources, Concept `ml`, Type `Note`, 10 relation kinds of which 2 only in `extensions.ttl`, one with no label and one with an `@fr` label, and 6 relation triples of which 1 only in `extensions.ttl`); `A` = an absent data dir; `E` = an empty data dir. Stored paths are shown as `/TMP` (the `cli_find.rs` pattern).

| # | Criterion | Kind | Check | Expected failure |
|---|---|---|---|---|
| 1 | Given `R`, `describe --json` prints the full object | test | `tests/cli_describe.rs::describe_json` vs locked golden `cli_describe__describe_json` (`store` `/TMP/data`, counts `{5,1,1,10,6}`); `describe_lists_match_list_commands` (lists equal `concept\|type\|relation list --json --limit 1000` `results`) | exit 2 (unrecognized subcommand) |
| 2 | `--brief` is the full object minus the three lists | test | `describe_brief_json` vs locked golden `cli_describe__describe_brief_json`; `brief_is_full_minus_lists`; core `tests/core_describe.rs::brief_has_no_lists` | exit 2; unresolved import `hali::core::describe` |
| 3 | Absent store: exit 0, empty-store output, data dir not created | test | `describe_absent_store` (golden `cli_describe__describe_json_empty`, `!data.exists()` after, empty stderr); `describe_empty_dir_matches_absent` (stdout byte-equal, dir listing unchanged); `describe_writes_nothing` on `R` | exit 2 |
| 4 | Count semantics | test | core: `counts_relations_fixture` ({5,1,1,10,6}), `relation_in_two_graphs_counts_once`, `resource_only_in_extensions_not_counted`, `empty_store_counts` ({0,0,0,4,0}) | unresolved import |
| 5 | Core version from the core graph only | test | core: `core_version_is_core_ttl_version_info` (equals the literal parsed from `ontology::CORE_TTL`, `"0.1.0"`), `extensions_cannot_override_core_version` | unresolved import |
| 6 | Store path as `paths::data_dir` resolves it | test | cli: `store_is_hali_data_dir`, `store_falls_back_to_xdg_data_home` (`HALI_DATA_DIR` unset) | exit 2 |
| 7 | Text output, brief text, terminal escaping | test | cli: `describe_text`, `describe_brief_text` (exact `assert_eq`), `describe_text_escapes_controls` (an `extensions.ttl` label with ESC prints `\u{1b}`) | exit 2 |
| 8 | Labels in the configured language | test | cli: `describe_uses_configured_language` (`config.toml` `language="fr"` gives `annotates` labelled `annote`; standard kinds fall back to their ids) | exit 2 |
| 9 | Read-only; guard covered | test, check | cli: `describe_dry_run_and_yes_identical`; `tests/dry_run_guard.rs` `CASES` row `("describe", "relations", &["describe"])` (one line, no `--yes`); `.goal/checks/01-dry_run_guard_describe.sh` | exit 2; `MISSING: no CASES row for "describe"` |
| 10 | Existing store errors apply, no new rule | test, check | cli: `describe_unparseable_store` (exit 3, `STORE_UNPARSEABLE`, `--json` envelope on stderr), `describe_data_dir_is_a_file` (exit 3, `STORE_IO`); `.goal/checks/02-no_new_rule.sh` (`git diff main -- src/core/schemas/error.json` empty, no new `Error::new("` rule string in `src/core/mod.rs`) | exit 2 ≠ 3; the check is a guard (passes before the change) |
| 11 | JSON Schema added; goldens validate | test | `core_schema.rs::names_and_get` and `cli_schema.rs::schema_lists_names_text` with `describe` inserted; locked golden `cli_schema__schema_list_json` updated; `schema_contract.rs` `GOAL_SNAPS_19`/`GOAL_STDOUT_GOLDENS_19` (three describe goldens → `describe`); `core_schema.rs::describe_schema_rejects_partial_lists` (full output minus `types` is invalid) | names mismatch; schema `describe` not served |
| 12 | Spec updated | check | `.goal/checks/03-spec_describe.sh`: §9.1 `**Describe:**` paragraph naming `core_version`, `store`, `counts`, `--brief`; `describe` in the §9.5 read-only list; the §9.4 exit-3 row no longer says "no store" (the test-author writes these into the spec in the goal commit, as #15/#18 did) | missing paragraph |
| 13 | Security-sensitive seams untouched | check | `.goal/checks/04-seams_untouched.sh`: `git diff --quiet main -- src/paths src/storage Cargo.toml Cargo.lock` | a guard |
| 14 | Existing goldens unchanged | check | `.goal/checks/05-existing_goldens_unchanged.sh` (the #18 pattern; `cli_schema__schema_list_json` excluded) | a guard |

`.goal/locked`: `tests/snapshots/cli_describe__describe_json.snap`, `cli_describe__describe_brief_json.snap`, `cli_describe__describe_json_empty.snap`, `cli_schema__schema_list_json.snap`, `tests/cli_describe.rs`, `tests/core_describe.rs`. Goldens are hand-written from the contract. `relation_kinds` order for `R`: annotates, digestOf, excerptOf ("excerpt of"), isPartOf, quotes, references, relation, sketch, summarises, wasDerivedFrom (sorted by label).

## Approach

Not cross-cutting: one new core module, one CLI arm, one schema. Implementer on its default model.

1. `src/core/describe.rs` (new; `pub mod describe;` in `core/mod.rs`): `describe(ctx, store, store_path: &str, brief: bool) -> Description`. Reuse `concept::list`, `types::list`, `relation::list` with `(0, usize::MAX)`; add small `pub(crate)` helpers rather than copying (`resource::cataloged_count(store)` over the private `subjects()`, `relation::relation_count(store)` over `kinds()`, collecting `(s,p,o)` into a `BTreeSet` across graphs); read the core version with `quads_for_pattern(<https://w3id.org/hali>, owl:versionInfo, None, Some(CORE_GRAPH))`, adding `owl::VERSION_INFO` and an `ONTOLOGY` IRI constant to `src/ontology/mod.rs`. `Description` uses `Option<Vec<…>>` lists with `skip_serializing_if`, so brief is one struct. Do not call `find` to count (it builds a record per Resource and `stat`s paths).
2. `src/core/schemas/describe.json` (an `anyOf` of two branches) and a `SCHEMAS` entry (22 → 23, sorted).
3. `src/cli/mod.rs`: `Command::Describe { #[arg(long)] brief: bool }`; the arm passes `dir.to_string_lossy()` from the already-resolved `dir` (no new `paths` call); JSON with a `schema_version` wrapper or the text lines through `printable`. The existing private `fn describe(record: &Record)` needs another name.
4. Tests the implementer updates (not goal-owned): `core_schema`/`cli_schema` name lists beyond the locked golden, the `schema_contract` arrays follow the existing hide/unhide pattern, and any other hardcoded schema list (`tests/cli_find.rs`, `tests/cli_type.rs`, `tests/cli_relation.rs`: grep for the count 22).
5. Spec: §9.1 **Describe** paragraph, §9.5 list, §9.4 wording (in the goal commit).

## Security call

Not sensitive if row 13 holds: the diff reads through the existing `storage::load` and the existing `paths::data_dir` call, adds no dependency, write, network or SPARQL built from user input. Exposure: it prints the user's own data-dir path and stored labels; hand-edited labels and ids are a terminal-injection vector, covered by `printable` (row 7). No security-reviewer unless the implementer touches `src/paths`, `src/storage` or `Cargo.*` (row 13 forbids it). The ordinary reviewer confirms every text value goes through `printable`.

## Risks

Uncapped lists grow with the vocabulary; `concept list` gives `label ""` for a Concept with no configured-language prefLabel and `describe` inherits that; a relative `HALI_DATA_DIR` prints a relative path; the `schema_contract` hide/unhide arrays must follow the existing pattern or the commit hook goes red; no reader takes the shared lock (§3.3), tracked separately.
