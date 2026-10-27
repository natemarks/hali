# Plan: #12 `hali find` (core filters and paging)

Approved by the human with all of the planner's recommended answers. Contract wording is in the issue body.

## Goal map

| # | Criterion | Check |
|---|---|---|
| 1 | Paging, `--fields`, `--path-only` | `tests/core_find.rs::{pages_with_limit_and_offset, offset_past_end_is_empty_page, default_limit_is_50, fields_narrow_in_record_order, unknown_field_is_usage}`; `tests/cli_find.rs::{find_json, find_paged_json, limit_zero_is_usage_error, find_fields_json, unknown_field_exit_2, path_only_lists_page_paths}` (locked goldens `cli_find__find_json.snap`, `cli_find__find_json_fields.snap`, from spec §9.3) |
| 2 | `--title`, `--media-type`, filters AND | `core_find::{title_is_case_insensitive_substring, media_type_exact_and_wildcard, bad_media_pattern_is_usage, filters_and}`; `cli_find::{title_and_media_type_and, bad_media_type_exit_2}` |
| 3 | Sort | `core_find::{default_sort_is_modified_desc, sort_added_uses_issued, sort_title_case_insensitive, ties_break_on_id}`; `cli_find::sort_flag` |
| 4 | `missing`/`stale` at output time; `--missing`/`--stale` | `core_find::{missing_and_stale_from_injected_stat, missing_filter, stale_filter, missing_and_stale_together_empty}`; `cli_find::missing_and_stale_from_stat` |
| 5 | Exit 0 on empty; never returns content; reads never hash | `cli_find::{empty_store_exit_0, no_match_exit_0, never_prints_file_content, reads_never_hash}` (locked golden `cli_find__find_json_empty.snap`). `reads_never_hash`: (a) a mode-000 cataloged file gives exit 0 with `missing:false` (skipped as root); (b) content rewritten with the same size and a pinned mtime gives `stale:false` |
| 6 | §14.1 discipline; `--dry-run` ignored | rows 1-5; `.goal/checks/01-dry_run_guard_find.sh` (CASES has a `find` row); `cli_find::dry_run_is_ignored` |
| 7 | Schema and golden validation | `cli_find::{schema_lists_find, find_schema_is_document}` (exact new list; the existing unlocked `tests/cli_schema.rs` `NAMES`/text list and `cli_schema__schema_list_json.snap` must be updated by the implementer to include `find`, since they pass today and cannot be locked); `schema_contract` rows mapping the three `cli_find__*` snaps to `find`; `schema_contract::find_schema_accepts_and_rejects` (full record ok, `--fields` subset ok, unknown key rejected, `content` key rejected) |
| P1 | Optional lazy stat: only the page is stat'ed without `--missing`/`--stale` | `core_find::stat_only_the_page_without_missing_or_stale_filters` (counts closure calls) |

## Approach
1. `core::resource`: `Query`, `Sort`, `Found`, `find(ctx, store, &Query, stat: impl Fn(&str) -> Option<(u64, i64)>) -> Found`; helpers `parse_media_pattern`, `parse_fields`, `narrow`. Reuse `record()` and `Record::observe`. Collect every `hali:Resource`, filter on title and media type, stat per candidate (only the page when no `--missing`/`--stale`), filter, sort, page.
2. `cli`: `Find` command; the `stat` closure is `|p| storage::stat(Path::new(p))`. Reuse `parse_limit` and `printable`. Thin passthrough.
3. `src/core/schemas/find.json`, registered in `SCHEMAS`; `resource_fields` def in `defs.json`; `$defs/resource` unchanged.
4. `tests/dry_run_guard.rs` CASES row for `find` (implementer adds it; the file is unlocked).

Not cross-cutting: implementer on sonnet; opus only if a round stalls.

## Hazards for the test-author
- Goal-tag the new snaps through `GOAL_SNAPS` / `GOAL_STDOUT_GOLDENS` in `tests/schema_contract.rs`; raise the `every_golden_validates` count only under the goal feature.
- `/TMP` redaction and `pin_mtime`; vary `HALI_TEST_NOW` per `add` for `added` ordering.
- The fixture store's Resource (`/home/nmarks/...`) does not exist on test machines, which gives a `missing:true` case.
- `make unit` runs with `--features test-hooks`; new tests needing outside software must be `#[ignore]` (none expected).

## Security
Not sensitive provided `src/storage/` is unchanged (`find` reuses the read-only `storage::stat`). If the implementer edits `src/storage/` (e.g. a batch stat), the security-reviewer runs.
