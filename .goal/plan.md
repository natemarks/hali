# Plan for #24: Moves and pruning during scan

Approved by the human with all 14 planner recommendations (D1-D14) and the accepted risks. Contract wording: "Groomed contract (approved)" in issue #24; spec `docs/spec.md` (§7, §9.1 Scan/Forget/Relink, §9.8) is the behaviour source. Implementer: opus. Security-reviewer: not required (not on the AGENTS.md list; the normal reviewer checks the prompt, the no-stdin path, "declining writes nothing").

## Decisions (human-approved)

D1 pool is store-wide (Missing Resources outside the root count; `stat`ed once, only when a new file has their size and SHA-256). D2 ambiguous candidate files are not registered. D3 filters (`--max-size`, `--media-type`, empty files, unhashable) gate move candidates; no extra reads. D4 prune only Missing under the root, never relinked or ambiguous. D5 an inbound relation from another pruned Resource still blocks. D6 `--force` with an inbound relation only in `extensions.ttl` is `SHACL_VIOLATION`, nothing written. D7 output shape below. D8 text wording below. D9 prompt below. D10 retry repeats the flags given. D11 `--force` without `--prune` is a usage error. D12 `schema_version` stays 1. D13 declining writes nothing at all. D14 core API: `pub enum Prune { Off, On, Forced }`, `scan_with(&Context, &mut Store, &ScanOptions, Prune, entries, stat, inspect, glob_type, progress, user_name)`; `scan(...)` keeps its exact signature and calls `scan_with(..., Prune::Off, ...)`.

## Output contract (D7)

Keys in this order: `{"schema_version":1,"root","counts":{"added","refreshed","relinked","ambiguous","unchanged","missing","pruned","skipped"},"media_types","added","refreshed","relinked":[{"resource","from"}],"ambiguous":[{"resource","candidates":[<path>...]}],"missing","pruned":[<record as it was>],"skipped","truncated"}`.
`relinked`, `ambiguous`, `pruned` always present (`[]`, 0 without `--prune`/matches). `missing` = Resources under the root still cataloged and Missing after the run (ambiguous ones and ones `--prune` kept for inbound relations included). `media_types` counts only added Resources. Every list in byte order of the record's path (the new path for relinked), at most 100; `candidates` at most 10, byte order. `truncated` covers every list. Records observed after the change; pruned records keep their old path, `missing: true`.

## Text (D8)

`name<TAB>value` lines in this order: `root`, `added`, `refreshed`, `relinked`, `ambiguous`, `unchanged`, `missing`, `pruned`, `skipped`; then the `media_type` lines; then per entry: `added|would add resource ...`, `refreshed|would refresh resource ...`, `relinked|would relink resource <id> "<title>" <old path> -> <new path>`, `ambiguous resource <id> "<title>" <path>` followed by `candidate <path>` lines, `missing resource ...`, `forgot|would forget resource <id> "<title>" <path>`, `skipped <reason> <path>`; then `truncated<TAB>true`, then the `+`/`-` lines. New `scan_text` expectation starts `root\t/TMP/root\nadded\t2\nrefreshed\t0\nrelinked\t0\nambiguous\t0\nunchanged\t0\nmissing\t0\npruned\t0\nskipped\t1\n`.

## Confirmation (D9, D10, D13)

Prompt: `forget <n> missing resources under <root>? [y/N] ` (root escaped via `printable`, no pluralisation). `CONFIRMATION_REQUIRED`: `details.command` is `"scan"`; message from the shared helper: `scan needs confirmation, but stdin or stderr is not a terminal; nothing was written`. Retry: `hali scan <root> [--include-hidden] [--no-ignore] [--exclude=<glob>]... [--max-size=<bytes>] [--media-type=<pattern>]... --prune [--force] --yes`; repeats only the flags given on the command line, in that order; each value through `core::shell_quote`; the root always quoted; `--max-size` is the parsed byte count; `--media-type` the lowercase pattern (MediaPattern needs a Display). Example: `hali scan '/TMP/root' --exclude='*.tmp' --max-size=1024 --media-type='text/*' --prune --force --yes`. Spec: synopsis `hali scan [<root>] ... [--media-type M]... [--prune [--force]] [--list] [--forget-root <root>]`; `--force` without `--prune` is a usage error `--force is for --prune; a scan without --prune forgets nothing`, raised before config and store. Spec §9.1 "scan is not destructive ... never asks" and "Resources outside the root are not looked at" must be amended; the lead sentence drops "(#24)".

## Approach

1. `src/core/resource.rs`: extract `pub(crate) fn inbound(store, &NamedNode) -> HashSet<(NamedNode, NamedNode)>` and `pub(crate) fn forget_quads(store, &NamedNode, leaving: &HashSet<NamedNode>) -> Vec<Quad>` (format node kept if any Resource not in `leaving` uses it); `forget` uses both with `leaving = {r}`; `with_content` `pub(crate)`; `Display` for `MediaPattern`.
2. `src/core/relink.rs`: `moved` and `MAX_CANDIDATES` `pub(crate)`; reuse `match_moves`, `Relinked`, `Ambiguous`.
3. `src/core/scan.rs` `scan_with`: after walk/classify, `stat` under-root Resources, refresh Stale ones; candidates = non-empty new facts; pool = Missing, non-empty Resources with a checksum for each distinct (size, sha) via `with_content` (under-root: the `missing` set, never `stat`ed twice; others `stat`ed once); `match_moves`; Relinked: `moved` + touch now, drop the new file from registration; Ambiguous: record up to 10 candidate paths, drop them; prune per D4/D5/D6; register the rest (ids in path order, a relinked file takes no id); one `validate::guard`, one `apply`; sort, cap 100, `truncated`.
4. `src/cli/mod.rs`: `--prune` and `--force` on `Scan` (thin), usage error before config/store, `Confirmation` passed only when `counts.pruned > 0`; text output extended.
5. `src/core/schemas/scan.json` both branches; `docs/spec.md` §9.1 synopsis and **Scan:**, §9.8; `tests/dry_run_guard.rs` scan row.

## Tests that must change (test-author, in `goal:` commits)

- `core_scan.rs::refreshes_stale_under_root_only`: `new.txt` currently shares the Missing `gone.txt`'s size and hash; give it its own content (e.g. size 3, `sha(3)`) through a wrapper `inspect`; passes before and after, untagged.
- `cli_scan.rs::usage_errors_before_store`: delete the `--prune` case (green before and after).
- Rewrite and goal-tag: `scan_json`, `scan_dry_run_json`, `scan_text`, `schema_contract::scan_schema_accepts_and_rejects` (its "counts extra key `relinked`" case flips); locked rewrites of `cli_scan__scan_json.snap` and `cli_dry_run__scan_dry_run_json.snap`.
- Unaffected (checked): `filters_gate_only_new_registrations`, `missing_and_prefix_rule`, `size_filter`, `nothing_to_do`, `scan_caps_lists_but_counts_exactly`, `scan_dry_run_matches_real_run`, every other CLI test (the `store` fixture's size matches nothing).

## Goal map (24 rows)

All new tests are goal-tagged inside `#[cfg(feature = "goal")] mod moves_and_prune { use super::*; ... }`, nested in the existing `mod goal` of `tests/core_scan.rs` and `tests/cli_scan.rs`.

| # | Criterion | Check | Expected failure |
|---|---|---|---|
| 1 | One-to-one move relinked exactly as `relink::explicit` writes it; file not added; listed in `relinked` with `from` | `core_scan::moves_and_prune::relinks_a_one_to_one_move` (oracle: `relink::explicit` on a copy) | no `scan_with` / no `relinked` field |
| 2 | Missing Resource outside the root is relinked; outside-root Resources `stat`ed only when a new file has their content | `matches_missing_resources_outside_the_root` (asserts the `stat` log) | compile error; then `added.len()` is 1 |
| 3 | Several matches: nothing changes, no candidate registered, `ambiguous` with `candidates` (at most 10, byte order) | `several_candidates_change_nothing` (11 files gives 10) | compile error |
| 4 | Never candidates: empty files, a Missing Resource recorded empty, `too_large`, media-type-ruled-out, unhashable; content only present Resources have is registered | `what_is_never_a_move_candidate` (`inspect` never called for `too_large`) | compile error |
| 5 | Relinks, refreshes, adds are one mutation; ids only for registered files, in path order | `moves_refreshes_and_adds_in_one_mutation` | compile error |
| 6 | Without `--prune` nothing forgotten; `missing` listed, `pruned` `[]`, `counts.pruned` 0 | `prune_off_forgets_nothing` | `Prune` undefined |
| 7 | With `--prune`, under-root Missing (not ambiguous) forgotten like `forget`; shared format node only when unused; outside-root never pruned; no file opened | `prune_forgets_like_forget` (fixture `forget`, root `/fixtures/forget`; oracle: sequential `resource::forget`) | compile error |
| 8 | Inbound relation in any graph (incl. from another vanished Resource) keeps the Resource unless `Forced`; `Forced` removes `resources.ttl` inbound triples; `extensions.ttl`-only inbound under `Forced` is `SHACL_VIOLATION`, store untouched | `prune_skips_inbound_relations_unless_forced` (fixture `relations`) | compile error |
| 9 | One SHACL check over refresh, relink, prune, add; violation writes nothing, lists every focus node | `one_shacl_check_with_moves_and_prune` (fixture `scan/needs-description`) | compile error |
| 10 | Summary shows relinked counts and entries (JSON) | `cli_scan::moves_and_prune::scan_moves_json`, locked golden `cli_scan__scan_moves_json` | no `"relinked"` key |
| 11 | Text summary: count lines in the new order, `relinked ... -> ...`, `ambiguous`, `candidate` | `scan_moves_text` | no `relinked\t1` line |
| 12 | An ambiguous file stays unregistered so `hali relink <id> <path>` can settle it | `ambiguous_files_stay_unregistered` | file in `added`; relink exits 2 |
| 13 | `--prune` with no terminal and no `--yes`: `CONFIRMATION_REQUIRED` (exit 1, `declined:false`), retry repeats flags; nothing written; stdin not read; stdout empty | `prune_needs_yes_without_a_terminal` (locked golden `cli_scan__prune_confirmation_required_json_envelope`), `retry_repeats_the_given_flags` | exit 2 `unexpected argument '--prune'` |
| 14 | On a terminal the prompt goes to stderr; `y`/`yes` confirms; `n`, empty, EOF decline (`declined:true`); declining writes nothing, adds included | `prune_asks_on_a_terminal` (`HALI_TEST_TTY=1`) | exit 2 |
| 15 | `--prune --yes` forgets and prints (JSON, text); a kept Resource stays in `missing`; `--force` removes inbound relations; files never touched | `prune_json` (locked golden `cli_scan__scan_prune_json`), `prune_text`, `prune_force_removes_inbound_relations`, `prune_never_touches_files` | exit 2; snapshot mismatch |
| 16 | A prune that would forget nothing never asks, even with no TTY and no `--yes` | `nothing_to_forget_never_asks` | exit 2 |
| 17 | `--force` without `--prune` is a usage error (exit 2, the D11 message) before the store is read; `--prune` alone accepted (exit 3 on a garbage store) | `force_needs_prune` | stderr lacks the message; `--prune` gives 2 |
| 18 | `--dry-run --prune`: real payload plus `dry_run` and `changes`, never asks, writes nothing, matches the real run with `--yes` | `prune_dry_run_json` (locked golden `cli_dry_run__scan_prune_dry_run_json`, `HALI_TEST_TTY=0`), `support::assert_dry_run_matches_real_run(store, ["scan", root, "--prune", "--yes"])` | exit 2 |
| 19 | Existing scan outputs gain exactly the new empty keys and lines | rewritten, goal-tagged `scan_json`, `scan_dry_run_json`, `scan_text`; locked rewrites of the two goldens | snapshot mismatch |
| 20 | `hali schema scan` describes the new shape (8 counts keys, `relinked`, `ambiguous` with `candidates` 1..10, `pruned` records, `maxItems` 100, `schema_version` const 1); every new golden validates | rewritten goal-tagged `schema_contract::scan_schema_accepts_and_rejects`; `GOAL_SNAPS_24` / `GOAL_STDOUT_GOLDENS_24` rows | `additionalProperties "relinked"` |
| 21 | Spec states the contract | `.goal/checks/01-spec_scan_moves_prune.sh` | `MISSING: ... relinked` |
| 22 | Dry-run guard covers prune | `.goal/checks/02-dry_run_guard_scan_prune.sh`: the scan row must be `("scan", "store", &["scan", "src/ontology", "--prune", "--force"])`, no `--yes` | `MISSING: ... found ("scan","store",&["scan","src/ontology"])` |
| 23 | No other golden changes (every schema-list golden unchanged: no schema name is added); the two rewritten scan goldens equal main's with only the new keys inserted | `.goal/checks/03-existing_goldens_unchanged.sh` | a guard, passes before |
| 24 | `forget` and `relink` behaviour unchanged by the refactor | existing `core_forget`, `cli_forget`, `core_relink`, `cli_relink` plus guard 23 | green before |

Check 01 must find, in the `^\*\*Scan:\*\*` paragraph: `--prune`, `--force`, `--yes`, `relinked`, `ambiguous`, `pruned`, `candidates`, `from`, `one-to-one`, `CONFIRMATION_REQUIRED`, `inbound relations`, `SHACL_VIOLATION`, `forgot|would forget resource`, `relinked|would relink resource`, `candidate <path>`, `--force is for --prune`, `100`, `10`; the absence of `(#24)` and of `Resources outside the root are not looked at`; the synopsis contains `[--prune [--force]]`; §9.8 contains `forget <n> missing resources under <root>? [y/N] `, `"scan"` and `--prune --yes`.

Check 03: `git diff --name-status main -- 'tests/snapshots/*.snap'`, excluding by exact name only `cli_scan__scan_json.snap`, `cli_dry_run__scan_dry_run_json.snap`, `cli_scan__scan_moves_json`, `cli_scan__scan_prune_json`, `cli_dry_run__scan_prune_dry_run_json`, `cli_scan__prune_confirmation_required_json_envelope`. The two rewritten goldens must equal main's after: `s/"refreshed":0,"unchanged"/"refreshed":0,"relinked":0,"ambiguous":0,"unchanged"/`, `s/"missing":0,"skipped"/"missing":0,"pruned":0,"skipped"/`, `s/"refreshed":\[\],"missing":\[\],/"refreshed":[],"relinked":[],"ambiguous":[],"missing":[],"pruned":[],/`.

`.goal/locked`: `tests/core_scan.rs`, `tests/cli_scan.rs`, `tests/schema_contract.rs`, and the six snapshot files above.
