# Plan: #22 Scan a root

Contract: issue #22 body, including "Contract addendum (re-groomed 2026-11-03)". Approved goal map: issue comment 1 (rows 1, 2, 7-20, 23, 24), with the corrections and additions below. Approved by the human on 2026-11-03, with decisions:

1. Data-dir refusal is a usage error (exit 2).
2. `storage::save` is unchanged; atomic write stays with #5. The process-death guarantee is scoped to "before the write".
3. Deep-tree bound: 15 s, debug build, a hang detector run under a deadline.
4. Process death: FIFO git config + SIGKILL, with a "was blocked" precondition assertion.

## Goal map

| # | Criterion | Checks | Expected red |
|---|---|---|---|
| 1 | No root, two roots, bad `--max-size`/`--media-type`/`--exclude`, or `--prune` exits 2 before the store is read; asserts stderr text | `tests/cli_scan.rs::usage_errors_before_store` | stderr lacks `scan root` / `--max-size` |
| 2 | Relative and symlinked roots resolve to the real path; missing or file root exits 2; non-UTF-8 `NON_UTF8_PATH`; unreadable `FILE_UNREADABLE`; nothing written; asserts stderr text | `cli_scan::{root_resolution, root_errors}` | exit 2, stderr lacks `is not a directory` |
| 2b | Root equal to or inside the data dir exits 2 before the store is read; nothing written | `cli_scan::root_in_data_dir_refused` | stderr lacks `data dir` |
| 7 | FIFO/socket `not_regular` never opened; mode-000 `unreadable`; non-UTF-8 name `non_utf8`; exit 0 | `cli_scan::{special_files_skipped, unreadable_skipped_scan_continues, non_utf8_skipped}` | exit 2 |
| 8 | `too_large` skipped, never opened; sparse 1 GiB+1 file fast; flag > config > default; size grammar | `core_scan::{size_filter, parse_size_grammar}`, `cli_scan::sparse_file_over_default_skipped` | no `scan` in core |
| 9 | `--media-type` narrows new registrations; glob-excluded file not inspected | `core_scan::media_type_filter` | no `scan` in core |
| 10 | New files registered untagged/untyped like `add`; ids in path order | `cli_scan::same_store_as_add_in_path_order`, `core_scan::registers_untyped` | exit 2 |
| 11 | Cataloged under root: Stale re-hashed as `refresh`, others never opened; filters ignored; unhashable skipped, unchanged | `core_scan::{refreshes_stale_under_root_only, filters_gate_only_new}`, `cli_scan::cataloged_ignored_file_refreshed` | no `scan` in core |
| 12 | Missing under root reported, unchanged; outside-root and `..`/`.` paths never stat'ed | `core_scan::missing_and_prefix_rule` | no `scan` in core |
| 13 | One mutation, one SHACL check (two violating files give one `SHACL_VIOLATION` listing both); nothing written on violation; no change means no rewrite and no data dir | `core_scan::one_shacl_check`, `cli_scan::{shacl_violation_nothing_written, nothing_to_do_writes_nothing}` | no `scan` in core; exit 2 |
| 14 | JSON shape and key order; caps (101 files -> 100 listed + `truncated:true`); `media_types` sorted; text escaped | `cli_scan::{scan_json, scan_text_escaped}` (locked golden), `core_scan::caps_and_truncated` | snapshot mismatch; exit 2 |
| 15 | Dry run: same payload + `dry_run`/`changes` equal to real-run diff; writes nothing | `cli_dry_run::scan_dry_run_json` (locked golden), `support::assert_dry_run_matches_real_run` | exit 2 |
| 15b | Dry-run guard has a scan row | `.goal/checks/02-dry_run_guard_scan.sh` | `MISSING: no CASES row for "scan"` |
| 16 | Progress only on stderr TTY and not `--json`; stdout identical; no file names | `cli_scan::progress_only_on_tty` | exit 2 |
| 17 | Nothing written under the root, real and dry run | `cli_scan::root_tree_unchanged` | exit 2 |
| 17b | Data dir inside the root is never entered | `cli_scan::data_dir_under_root_pruned` | exit 2 |
| 18 | `scan` schema (real and dry-run); `SCHEMAS` 24 -> 25; goldens validate | `schema_contract::scan_*`, locked schema-list golden | unknown schema name |
| 19 | `apply` linear (#112): byte-identical changes; 20k add + 20k remove under 2 s | `core_scan::apply_is_linear` | timeout |
| 20 | `[scan]` config parsed; wrong types `CONFIG_INVALID` with new fix text | `core_scan::config_scan_keys`, updated `cli_error_text` | no field `scan` on `Config` |
| 23 | Spec §7, §9.1 **Scan:**, §9.1 Refresh updated | `.goal/checks/04-spec_scan.sh` | `MISSING: **Scan:** paragraph` |
| 24 | Existing goldens unchanged (except schema list) | `.goal/checks/05-existing_goldens_unchanged.sh` | a guard |
| 25 | Walk flags and `[scan]` keys reach the walk; `bad_ignore_file`/`outside_root` in `skipped` | `cli_scan::walk_flags_and_config` | exit 2 |
| 26 | `--yes` accepted and ignored; stdin never read | `cli_scan::yes_ignored_stdin_unread` | exit 2 |
| 27 | Store unchanged if the process dies mid-scan (FIFO git config blocks `walk()`; assert blocked; SIGKILL; data dir byte-identical) | `cli_scan::killed_mid_walk_store_unchanged` | "blocked" precondition fails |
| 28 | 200-level tree with a `.gitignore` per directory under 15 s (debug), under a deadline | `cli_scan::deep_tree_time_bound` | exit 2 |

Test isolation (all `cli_scan` tests): roots under the system temp dir, never `target/`; data dir a sibling of the root; `HOME`, `XDG_CONFIG_HOME`, `GIT_CONFIG_GLOBAL`, `GIT_CONFIG_SYSTEM` at temp paths with `env_clear`; short socket paths; assert no host `.gitignore`/`.ignore` in the temp parent. `tests/support/mod.rs::run_in_fresh_env` gains the two `GIT_CONFIG_*` lines (goal-owned edit).

## Approach

`src/core/scan.rs` with `scan(ctx, store, &ScanOptions{root, max_size, media_types}, entries, stat, inspect, glob_type, progress, user_name) -> Result<Mutation<Scanned>, Error>`; `core::parse_size`; `media_type::glob_type(name) -> Option<String>`; thin CLI `Scan` arm. Reuse `refresh_one`, `touch`, `Record::observe`, `parse_media_pattern` (make `matches` `pub(crate)`), `default_title`, `describe_record`, `printable`, `mutated`. `apply` made linear (hash set); `catalog` split into a pure quad builder used by `add_all` and scan; `add_all` moves to one SHACL check with unchanged output. No second hashing loop, no abort-on-error closure, no per-file SHACL check, no SHACL counter hook in core.

CLI order: clap, config (`CONFIG_INVALID`), exclude globs, root (real path, is-dir, data-dir refusal), store load, eager `walk()` into a Vec on the main thread (never a small-stack thread, never parallel), prune the canonical data dir when it exists, inspect, core, SHACL, save. Progress uses a stderr-only TTY function next to `interactive()`, honouring `HALI_TEST_TTY`. Files touched: `src/core/{scan.rs,mod.rs,config.rs,resource.rs,schema.rs,schemas/scan.json}`, `src/media_type`, `src/cli/mod.rs`, `docs/spec.md`, tests, `tests/support/mod.rs`. Implementer: opus.

## Security

Needs the security-reviewer. Probe: data-dir refusal and pruning (including symlinks), nothing written under the root, `max_size` and output caps (DoS), terminal escapes in `skipped` paths and lossy names, a file that grows after `stat` (#115), abort/hang races from #131, non-atomic `save` (accepted, #5).

## Risks

Dry-run `changes` is about 15 triples per new file (about 15 MB JSON at 10k files); hashing is single-threaded; no store lock (#5); `inspect` has no read bound; SHACL on a large store about 80-90 ms (#48), paid once; `quietly()` swaps the global panic hook; a host `/tmp/.gitignore` can make tests flaky (asserted against).

## Defined (test-author notes)

- Goldens: `cli_scan__scan_json`, `cli_dry_run__scan_dry_run_json` (both tests live in `tests/cli_scan.rs`, with `prepend_module_to_snapshot => false` as in `cli_refresh.rs`) and `cli_scan__schema_list_json`; ids and triples are those `hali add` writes for the same files. `tests/schema_contract.rs` gains `GOAL_SNAPS_22` rows, `scan_schema_accepts_and_rejects` and `scan_schema_is_listed`.
- Seams the tests fix (see the headers of `tests/core_scan.rs` and `tests/cli_scan.rs`): `core::scan::scan` with `inspect -> Result<FileFacts, Unhashable>`, `glob_type(file name)`, `progress(seen, hashed)`; `core::parse_size`; `core::apply` becomes `pub`; `Config.scan: ScanConfig`.
- Older tests that assert the 24-name schema list or the old config fix text are not goal-owned; the implementer updates them (list in the `tests/cli_scan.rs` header).
- Extra tests beyond the map: `errors_come_in_the_documented_order`, `config_invalid_has_the_new_fix_text`, `max_size_precedence`, `scan_text`, `scan_dry_run_json`/`scan_dry_run_matches_real_run`, `scan_caps_lists_but_counts_exactly`, `core_scan::{registers_untyped, new_file_problems_never_abort, walk_skips_pass_through_in_path_order, progress_totals, unhashable_cataloged_file_is_skipped_and_left_alone, nothing_to_do}`.
