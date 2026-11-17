# Plan for #23 Remembered scan roots (approved)

The approved contract is the "Groomed contract (approved)" section of issue #23. Decisions: Q1 bare output = single-scan object with `roots` + `unavailable`; Q2 unknown `--forget-root` = usage error exit 2 with hint `hali scan --list`; Q3 reuse `STORE_IO`/`STORE_UNPARSEABLE`; Q4 state dir never walked (row 18); Q5 relink uses its own walker settings; Q6 vanished roots reported in `unavailable` and kept; Q7 bare scan never rewrites `roots.json`.

Cross-cutting: implementer on opus. Security-sensitive (`src/paths/`, `src/storage/`, new on-disk input): security-reviewer required.

## Seams
- `paths::state_dir(env)`; `core::roots` (pure text in/out: parse, serialise, remember, forget, list); `storage` state I/O (`src/storage/state.rs`, bounded non-blocking read, atomic write 0700/0600); `core::scan::scan_roots` (joint multi-root; `scan_with` = one root); CLI (`assert_cmd`, `insta`); `hali schema scan` + `tests/schema_contract.rs`.

## Goal map
| # | Criterion | Check |
|---|---|---|
| 1 | State dir resolves from XDG_STATE_HOME, else HOME, through `paths` | `tests/paths_state.rs::state_dir_*`, `tests/cli_scan_roots.rs::{state_dir_resolution, roots_json_is_outside_the_store}` |
| 2 | Per-root settings stored; deterministic, sorted; rescan replaces | `tests/core_roots.rs::{serialise_is_canonical, round_trip_byte_identical, odd_paths_round_trip, remember_upserts}` |
| 3 | Atomic write, dir 0700, file 0600; bounded reads; refuses FIFO and symlink | `tests/storage_roots.rs::*` |
| 4 | Real `scan <root>` remembers with the flags given; dry-run, error, declined prune do not | `tests/cli_scan_roots.rs::remembers_after_real_scan_only` |
| 5 | Bare `scan` uses each root's own settings | `tests/core_scan_roots.rs::{one_root_is_scan_with, each_root_uses_its_own_options}`, `tests/cli_scan_roots.rs::{bare_scan_uses_each_roots_settings, bare_scan_applies_every_setting_per_root, bare_scan_dry_run_writes_nothing, bare_scan_text}`, golden `cli_scan_roots__bare_scan_json` (locked) |
| 6 | Bare `scan --prune` relinks a cross-root move instead of pruning | `tests/core_scan_roots.rs::{cross_root_move_is_relinked_not_pruned, two_cross_moves_are_both_relinked, ambiguity_spans_roots, nested_roots_count_each_file_once, ids_follow_path_order_across_roots, one_shacl_check_across_roots}`, `tests/cli_scan_roots.rs::{bare_prune_relinks_a_cross_root_move, bare_scan_relinks_a_cross_root_move_without_prune, bare_prune_prompt_names_the_roots}` |
| 7 | A vanished remembered root is in `unavailable`, its Resources not pruned, still remembered | `tests/core_scan_roots.rs::{unavailable_roots_protect_their_resources, only_unavailable_roots_change_nothing}`, `cli_scan_roots::{vanished_root_never_pruned, all_roots_vanished_prunes_nothing}` |
| 8 | Bare scan with no remembered roots exits 0 with empty results | `core_scan_roots::no_roots_is_an_empty_scan`, `cli_scan_roots::bare_scan_no_roots` |
| 9 | Usage errors: scan flags without a root; `--list`/`--forget-root` with anything else (the three `cli_scan.rs::usage_errors_before_store` cases the contract changes move to `usage_errors`) | `cli_scan_roots::usage_errors` |
| 10 | `scan --list` in JSON and text, sorted | `cli_scan_roots::{list_json, list_text, list_is_sorted_in_byte_order, list_empty, list_writes_nothing}`, golden `cli_scan_roots__scan_list_json` (locked) |
| 11 | `--forget-root` stops remembering; `resources.ttl` byte-identical; `find` total unchanged; dry-run writes nothing | `cli_scan_roots::{forget_root_keeps_resources, forget_root_dry_run, forget_root_matches_the_real_path}`, goldens `cli_scan_roots__forget_root_json`, `cli_dry_run__scan_forget_root_dry_run_json` (locked) |
| 12 | Unknown root = usage error exit 2 with hint; deleted root can be forgotten | `cli_scan_roots::{forget_root_unknown, forget_root_deleted_dir}` |
| 13 | Bare `scan --prune` without terminals gives `CONFIRMATION_REQUIRED`, retry `hali scan --prune --yes` | `cli_scan_roots::bare_prune_needs_yes_without_a_terminal`, golden `cli_scan_roots__bare_prune_confirmation_required_json_envelope` (locked) |
| 14 | Relink searches remembered roots by default; skips unusable silently; `--root` replaces them | `tests/cli_scan_roots.rs::{default_roots_include_remembered, default_roots_order_and_repeats, explicit_root_replaces_remembered, unusable_remembered_roots_are_skipped_silently, relink_ignores_the_remembered_settings}` |
| 15 | Corrupt/oversized/relative/non-UTF-8 `roots.json` exits 3 (scan, `--list`, relink) | `cli_scan_roots::{corrupt_state_file_exits_3, unresolvable_state_dir_exits_3}` |
| 16 | Schema covers bare scan, `--list`, `--forget-root`; every new golden validates | `tests/schema_contract.rs::{scan_schema_covers_remembered_roots, every_golden_validates}` |
| 17 | Spec and agent guide describe remembered roots | `.goal/checks/01-spec_remembered_roots.sh` |
| 18 | State dir never walked or cataloged by scan or relink | `cli_scan_roots::{state_dir_pruned_from_walk, relink_never_searches_the_state_dir}` |
| 19 | A file walked under nested remembered roots is added if any root that walked it would register it (an outer root's media types or size limit do not block the inner root) | `tests/core_scan_roots.rs::nested_roots_with_different_settings_add_what_any_root_registers`, `tests/cli_scan_roots.rs::bare_scan_nested_roots_add_what_the_inner_root_would` |
| 20 | The state dir, or a directory in it, is never a root: `scan` and `relink --root` exit 2; a remembered one is `unavailable` in a bare scan and skipped by relink's defaults | `cli_scan_roots::{scan_refuses_the_state_dir_as_a_root, bare_scan_never_walks_a_state_dir_root, relink_refuses_the_state_dir_as_a_root}`, spec lines in `.goal/checks/01-spec_remembered_roots.sh` |
| 21 | An unwritable state dir fails `scan` (also `--prune --yes`) with STORE_IO before the store is saved | `cli_scan_roots::unwritable_state_dir_leaves_the_store_alone` |
| 22 | `--forget-root <root>/` with a trailing slash works on a deleted root | `cli_scan_roots::forget_root_with_a_trailing_slash` |
| 23 | A bare scan's `roots` is deduplicated by real path and in real-path byte order (mutation-backed: deleting the sort or the dedup in `scan_remembered` fails it) | `cli_scan_roots::bare_scan_roots_are_deduplicated_and_in_real_path_order` |

Guards: `.goal/checks/02-existing_goldens_unchanged.sh` (no existing golden changes; `scan <root>` output unchanged); `forget`, `relink` explicit-root behaviour unchanged.

Tests must set `HOME` and `XDG_STATE_HOME` to temp dirs so real `~/.local/state` is never touched.

## Assumed APIs (fixed by the goal tests)
`paths::state_dir(env)`; `storage::state::{ROOTS_FILE, read_roots(&Path) -> Result<Option<String>, storage::Error>, write_roots(&Path, &str)}`; `core::roots::{RootEntry, parse, serialise, remember, forget}`; `core::scan::{RootScan, scan_roots(ctx, store, Vec<RootScan>, &[String] unavailable, Prune, stat, inspect, glob_type, progress, user_name)}` (see each test file's header). `scan <root>` output, `schema_version` and the schema names are unchanged; the `scan` schema gains the bare, `--list` and `--forget-root` shapes. The old `cli_scan.rs::usage_errors_before_store` cases for a bare `scan`, `scan --forget-root <root>` and `scan <root> --list` moved to `cli_scan_roots.rs`.
