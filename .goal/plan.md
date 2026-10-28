# Plan: #14 `forget` and confirmation for destructive commands

Contract: see issue #14 ("Contract (groomed)"). Approved with all planner recommendations (open questions 1-11).

## Goal map

New files `tests/core_forget.rs` and `tests/cli_forget.rs`, each with `#[cfg(feature = "goal")]` on its own line above `mod goal`. Fixture `tests/fixtures/forget/`: R1 (IANA, tagged, with a catalog record); R2 and R3 sharing one `hali` media-types format node; a hand-written `R3 dcterms:relation R2`; the Person. Hand-written `.snap`s and the fixture are listed in `.goal/locked`.

| # | Criterion | Checks |
|---|---|---|
| 1 | Forget removes the Resource, checksum node, catalog record and inbound triples; keeps the Person, other Resources and a still-used format node | `tests/core_forget.rs::{forget_removes_resource_record_checksum_and_inbound, forget_keeps_person_and_other_resources, forget_drops_orphan_format_only, forget_changes_are_sorted_removes, forget_unknown_is_unknown_resource, forget_guard_sees_removed_quads}`; `tests/cli_forget.rs::forget_twice_is_unknown_resource` |
| 2 | File on disk untouched | `tests/cli_forget.rs::{forget_leaves_file_untouched, forget_missing_file_by_path_and_uuid}` (bytes, size, mtime ns, mode, inode equal; also a mode-0444 file in a 0555 dir) |
| 3 | TTY prompts; without a TTY `CONFIRMATION_REQUIRED` unless `--yes`; nothing waits on stdin | `tests/cli_forget.rs::{no_tty_requires_confirmation, yes_is_global, never_waits_on_stdin, tty_prompt_yes_no_eof, yes_skips_prompt_on_tty}`; `tests/core_forget.rs::confirmation_required_error`; locked golden `cli_forget__confirmation_required_json_envelope.snap` (stderr) |
| 4 | `--dry-run` supported | `tests/cli_forget.rs::dry_run_never_asks`; `tests/cli_dry_run.rs::forget_dry_run_matches`; locked golden `cli_dry_run__forget_dry_run_json.snap`; `.goal/checks/01-dry_run_guard_forget.sh` (CASES row `("forget", &["forget", STORED])`, no `--yes`) |
| 5 | Spec §14.1 | locked golden `cli_forget__forget_json.snap`; `.goal/checks/rules_triggered.sh` (`CONFIRMATION_REQUIRED` has a constructor and a triggering test in core and CLI tests) |
| 6 | Schemas | `tests/schema_contract.rs` (`GOAL_SNAPS_14` / `GOAL_STDOUT_GOLDENS_14`, count floor raised under goal), `forget_schema_and_confirmation_branch`, `cli_forget::schema_lists_forget` |
| opt | A real pty counts as interactive | `cli_forget::real_pty_prompts`, `#[ignore = "needs script(1) from util-linux"]` |

`never_waits_on_stdin`: `std::process::Command` with `Stdio::piped()` stdin held open and never written; `forget` (no `--yes`), `forget --yes` and `--dry-run forget` each exit within 10 s (child killed on deadline, via a `try_wait` loop).

## Approach

1. core: `Error::confirmation_required(command, retry, declined)` in `src/core/mod.rs`; `resource::forget(&Context, &mut Store, id) -> Result<Mutation<Record>, Error>` in `src/core/resource.rs` (`known()` first, collect quads with `quads_for_pattern` on the resources graph, read the record before applying, one `validate::guard(store, &[], &removed)`, one `apply`).
2. cli (`src/cli/mod.rs`): global `--yes`; `Forget { reference }`; `interactive()` (real, plus a `test-hooks` variant reading `HALI_TEST_TTY`); `confirm()` and `Confirmation`; `mutated()` takes `Option<&Confirmation>`, checked after the dry-run branch when `changes` is non-empty; text via `describe()`.
3. Schemas: `src/core/schemas/forget.json` (`oneOf` real/dry, like `edit.json`), registered in `SCHEMAS`; `CONFIRMATION_REQUIRED` branch in `error.json`.
4. Implementer updates the unlocked tests: `tests/cli_schema.rs` text list, `tests/core_schema.rs::names_and_get`, `cli_schema__schema_list_json.snap`, `tests/dry_run_guard.rs` CASES row.
5. Spec edits in the goal commit: §9.8 (dry run never asks; both-terminals rule; prompt on stderr; decline behaviour; never asks without changes), the §9.5 errors bullet, §14.2 `HALI_TEST_TTY`, a §9.1 note that `--force` arrives with #18.

Not cross-cutting: implementer on sonnet, raised to opus after a round without progress.

## Security call

Moderately sensitive; no listed security-sensitive path is touched (`src/storage/`, `src/paths/`, `Cargo.toml` unchanged). Security-reviewer recommended as well as the reviewer, given a destructive command and a stdin read. Risks and covering checks: irreversible loss (by spec); over-removal (core keep-tests); writing the file (file-untouched test; forget must never call `storage::inspect`); saving before confirmation or on decline (byte-identical `resources.ttl`); dry run that confirms or writes (dry-run guard); hang on stdin (held-open pipe test); terminal escapes in the prompt title (`printable()`); `HALI_TEST_TTY` steering a release binary (behind `cfg(feature = "test-hooks")`); `suggested_command` hands agents `--yes` (spec-mandated; the skill pre-approves only the `hali --dry-run` prefix).

## Risks

`shacl::run` uses a linear `removed.contains`; quadratic for #24's batch prune (note on #24). The dry-run `changes` golden is long and order-sensitive: derive it from the canonical N-Triples.
