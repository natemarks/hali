# Plan: #10 Catalog a file: `add`, `show` and `edit --title`

Approved by the human with the planner's recommended answers, except where changed below. Contract wording is in the issue body.

**Changes from the planner's draft (human decisions):**
- `make unit` runs only tests with no external dependencies, with `--features test-hooks`; `make static` / `static-check` include it (already do). Tests needing outside software (the MIME database) are `#[ignore = "<reason>"]` and run via a new `make test-external` (`cargo test --features test-hooks -- --ignored`), run manually and by a separate CI job that installs `shared-mime-info`. This replaces the planned `HALI_SKIP_MIME_TESTS=1` mechanism.
- The implementer edits `AGENTS.md` (which `CLAUDE.md` imports): the Commands table row for `make unit`, a row for `make test-external`, and a Rules bullet that tests needing outside software are `#[ignore]`d. The implementer also checks `.claude/agents/*` and `.claude/skills/*` for statements about `make unit`. Spec §13.2 and §14.7 are updated.

## Goal map

| # | Criterion | Check |
|---|---|---|
| 1 | Resource record: IRI, title, downloadURL, media type, size, modified, checksum | `tests/core_resource.rs::{add_writes_resource_triples, add_title_override, x_type_uses_dcterms_format}`, `paths::file_url_percent_encodes`, `tests/cli_resource.rs::add_json` (locked golden `cli_resource__add_json.snap`) |
| 2 | Media type: glob then magic, empty uses glob, fallbacks; `#[ignore]` MIME tests | `tests/media_type.rs::{glob_md, glob_beats_magic, magic_pdf_no_ext, empty_md_is_markdown, no_match_text_is_plain, no_match_binary_is_octet_stream}` (each `#[ignore = "needs shared-mime-info"]`), `.goal/checks/ci-runs-test-external.sh` |
| 3 | CatalogRecord and user Person | `core_resource::add_writes_catalog_record_and_mints_user_once`, `cli_resource::{user_name_from_config, user_name_from_env_user}` |
| 4 | Symlinks; refs path or full UUID | `cli_resource::{add_via_symlink_stores_real_path, show_by_relative_path, show_by_uuid, show_by_urn_uuid, uuid_prefix_is_unknown_resource}` |
| 5 | Idempotent re-add | `core_resource::re_add_returns_existing_no_changes`, `cli_resource::re_add_json_existing` |
| 6 | Error rules triggered | `cli_resource::{not_a_regular_file_directory, _fifo, _missing, non_utf8_path_arg, non_utf8_symlink_target, unknown_resource, file_unreadable}` |
| 7 | Resource SHACL shapes | goal module in `tests/core_shacl.rs`: conforming fixture plus six violating ones |
| 8 | Cataloged file never written | `cli_resource::cataloged_file_is_never_written` |
| 9 | `--path-only`; dry runs via the helper | `cli_resource::show_path_only`, `tests/cli_dry_run.rs::{add_dry_run_matches_real_run, edit_dry_run_matches_real_run, re_add_dry_run_no_changes}`, `dry_run_guard` rows |
| 10 | §14.1 discipline | rows 1-9 together |
| 11 | Schemas for add/show/edit; goldens validate | locked `cli_schema__schema_list_json.snap`, `schema_contract` rows for every new snapshot, `error.json` branches |
| P1 | `unit` is self-contained and uses test-hooks; `test-external` exists | `.goal/checks/unit-and-test-external.sh` (Makefile recipes, AGENTS.md rows and rule, spec §13.2/§14.7) |
| P2 | Hooks compiled into test builds | `cli_resource::test_hooks_are_compiled_in` (green under `make goal-tests` by design; red only under a bare `cargo test`) |
| P3 | Dry-run guard covers add/show/edit | `.goal/checks/12-dry_run_guard_rows.sh` |
| P4 | New deps pinned exactly | `.goal/checks/13-deps_pinned.sh` |
| P5 | Every non-goal `cargo test` Makefile recipe (`unit`, `unit-update-golden`, `test-external`, `integration`) passes `--features test-hooks`, so settled goldens run | `.goal/checks/14-makefile_test_hooks.sh` (expected failure now: `integration` lacks test-hooks) |

## Approach
1. Deps pinned exactly: `xdg-mime =0.4.0`, `sha2 =0.11.1`, `percent-encoding =2.3.2`.
2. `paths` (resolve, UTF-8 and regular-file rules, URL encoding), `storage::{inspect, stat}`, new `media_type`.
3. Shapes: media type "exactly one of dcat:mediaType / dcterms:format" as a named `sh:sparql` constraint (no blank nodes, so no `sh:xone` lists); property shapes for downloadURL, checksum, title. `shacl::validate`/`guard` take a removed-quads parameter so `edit` validates the would-be graph.
4. `core::resource::{add, show, edit_title, resolve_ref}` with injected `FileFacts`; `config` `[user] name`; new rules `UNKNOWN_RESOURCE`, `FILE_UNREADABLE`.
5. `cli`: `add`, `show`, `edit`; schemas `add.json`, `show.json`, `edit.json`, `$defs` `resource`, `catalog_record`.
6. `dry_run_guard` CASES rows; Makefile; CI job; spec; AGENTS.md.

Cross-cutting: run the implementer on opus.

## Hazards for the test-author
- New hand-written snaps trip the permanent `schema_contract` tests and `cli_schema::schema_lists_names_json`: goal-tag them (settle untags). Keep insta expression `out.stdout` / `out.stderr`; use `/TMP` for the canonical temp dir; pin the mtime with `File::set_modified`.
- `schema_contract::dry_run_goldens_named` contradicts this issue: generalise it.
- `tests/fixtures/store/resources.ttl` holds an incomplete Resource; complete it so shapes don't break `validate_json_conforming`, the dry-run guard and `edit` tests.
- `core_shacl::every_core_shape_has_a_violating_fixture`: put new rows in a goal module; the implementer updates `SHAPE_FIXTURES`.
- `#[ignore]` tests are skipped by `make unit`, so MIME tests are verified by running `cargo test --features test-hooks,goal -- --include-ignored` where shared-mime-info is installed.

## Security
Sensitive (`src/storage/`, `src/paths/`, `src/media_type/`, `Cargo.toml`/`Cargo.lock`, `.github/workflows/`): security-reviewer required. Threats: writing to the cataloged file; symlink/TOCTOU (open once, fstat the same fd, require a regular file); FIFO/device blocking (check type, `O_NONBLOCK`); control characters in terminal/JSON; percent-encoding mistakes; `xdg-mime` reading user-writable MIME data (bounded prefix); supply chain for `xdg-mime` (manual review + `cargo audit`).

## Test-author notes (define)
- Oxigraph normalises `xsd:nonNegativeInteger` to `xsd:integer` on insert, so `dcat:byteSize` is written as a bare integer (`dcat:byteSize 6`, an `xsd:integer`); the goal tests and snapshots expect `"6"^^xsd:integer`. Spec §2.1 says nonNegativeInteger; update the spec text and enforce non-negativity in SHACL (`sh:datatype xsd:integer`, `sh:minInclusive 0`) rather than by datatype.
- Shape fixtures and tests are in `tests/core_shacl_resource.rs` (locked), not in `tests/core_shacl.rs`, so the implementer can extend `SHAPE_FIXTURES` there (pointing at `shacl/resource-*`).
- `tests/dry_run_guard.rs` is not locked: the implementer adds the `add`, `show`, `edit` CASES rows (check 12).
- `schema_contract.rs` hides the new snapshots from builds without the `goal` feature (`GOAL_SNAPS`) and maps them only under it; settle removes the tags, after which `GOAL_SNAPS` may be deleted.
