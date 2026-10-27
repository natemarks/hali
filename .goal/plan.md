# Plan: #6 `hali schema` and golden-contract validation

Approved by the human with all recommended answers: word-path names + `error`; name-list-only no-arg output; bare schema documents; one `concept add` schema with oneOf real/dry-run; `rule` as SCREAMING_SNAKE pattern with per-rule `details` for SHACL_VIOLATION, SHAPES_INVALID, LABEL_TAKEN; `jsonschema` `=0.60.1` dev-dep, default-features off; no insta snapshots of schema documents. Contract wording is in the issue body.

## Goal map

| # | Criterion | Check |
|---|---|---|
| 1 | List schemas (json, text); core `names`/`get` | `tests/cli_schema.rs::schema_lists_names_json` (locked golden `cli_schema__schema_list_json.snap`), `::schema_lists_names_text`, `tests/core_schema.rs::names_and_get` |
| 2 | `schema <name>` is a valid draft 2020-12 doc, no external `$ref` | `tests/cli_schema.rs::each_schema_is_a_valid_document` |
| 3 | Unknown name: exit 2, empty stdout, stderr lists names | `tests/cli_schema.rs::unknown_name_is_usage_error` |
| 4 | Needs no store/config; writes nothing; `--dry-run` ok | `tests/cli_schema.rs::schema_needs_no_store`; `.goal/checks/04-dry_run_guard_row.sh` (the `("schema", &["schema"])` row in `tests/dry_run_guard.rs` CASES; added by the implementer, since it breaks `make unit` until the subcommand exists) |
| 5 | Every clap leaf has a schema | `tests/core_schema.rs::every_command_has_a_schema` |
| 6 | Every golden validates against its schema | `tests/schema_contract.rs::every_golden_validates` |
| 7 | Unmapped snapshot / stale row fails | `schema_contract.rs::every_snap_is_mapped`, `::no_stale_rows` |
| 8 | #8 dry-run goldens map to `concept add` | `schema_contract.rs::dry_run_goldens_named` |
| 9 | Schemas reject wrong shapes | `schema_contract.rs::schemas_reject` |
| 10 | SHAPES_INVALID golden validates against `error` | `tests/cli_validate.rs::shapes_invalid_json_envelope` (locked golden `cli_validate__shapes_invalid_json_envelope.snap`) |
| 11 | Shared `$defs` identical | `tests/core_schema.rs::shared_defs_identical` |
| 12 | Spec §9.1/§9.5 documented | `.goal/checks/12-spec_schema.sh` |
| 13 | `jsonschema` pinned dev-dep | `.goal/checks/13-jsonschema_pin.sh` (the dep is not in `Cargo.toml` yet: goal tests that use it only compile with `--features goal`, so add it in the implementation commit) |

## Approach
1. `core` first: `src/core/schema.rs` (`names()`, `get(name)`), schemas in `src/core/schemas/*.json` plus `defs.json`, embedded with `include_str!`, shared `$defs` spliced in.
2. `cli`: `Command::Schema { name: Vec<String> }`, handled before config/store loading; unknown names via clap error path; compact one-line output; add the `CASES` row in the same commit.
3. `tests/schema_contract.rs` fetches each schema from the real binary and validates golden JSON (stderr snaps map to `error`).
4. Add the SHAPES_INVALID golden; update spec §9.1, §9.5.

## Security
Sensitive: touches `Cargo.toml`/`Cargo.lock`, so security-reviewer runs too. Low exposure (dev-dependency, no HTTP/file `$ref`, embedded constants only). Check `make audit`.

## Risks
- `dry_run_guard` goes red as soon as the subcommand exists unless the CASES row lands in the same commit.
- insta header parsing varies; match `---\n` twice and `expression: out.stderr`.
- `oneOf` + `additionalProperties: false` must reject `dry_run` on the real branch and missing `changes` on the dry-run branch.
