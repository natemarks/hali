# Plan: #20 `hali refresh`

Contract: see issue #20 ("Contract (groomed)"). Approved with all planner recommendations (open questions 1-13); question 14 overridden by the human's approval: a security-reviewer runs (new caller of `storage::inspect` on paths a hand-editable store controls).

## Goal map

Fixture `F` = `tests/fixtures/refresh/resources.ttl` (locked): the user Person and five Resources R1-R5 with ids `0192f0c4-0000-7000-8000-000000000901`-`905` and catalog records `…911`-`915`. Every Resource is recorded as `"hello\n"`: 6 bytes, sha256 `5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03`, `modified 2026-10-20T14:02:11Z`; each catalog record has `issued` and `modified` `2026-10-21T08:00:00Z`. URLs are `file:///fixtures/refresh/<name>`, rewritten to the temp root (the `cli_forget.rs` pattern) and shown as `/TMP`. `NOW` is `2026-10-24T09:30:00Z`. What the test writes on disk:

| Resource | File | Content and mtime | State |
|---|---|---|---|
| R1 | `same.txt` | matches the record | unchanged |
| R2 | `edited.txt` | `"hello world\n"`, 12 bytes, sha `a948904f…a447`, mtime `2026-10-22T10:00:00Z` | Stale |
| R3 | `touched.txt` | `"hello\n"`, mtime `2026-10-22T10:00:00Z` | Stale by mtime only |
| R4 | `gone.txt` | not created | Missing |
| R5 | `sneaky.txt` | `"HELLO\n"`, sha `3b09aeb6…5ba4`, 6 bytes, mtime restored to the recorded one | not Stale |

New goal-tagged files `tests/core_refresh.rs` and `tests/cli_refresh.rs` (both locked). `tests/cli_dry_run.rs` is not locked (the implementer edits it), so the dry-run tests live in `cli_refresh.rs` with `prepend_module_to_snapshot => false` and the golden is named `cli_dry_run__refresh_dry_run_json` so `dry_run_command()` maps it.

| # | Criterion | Checks | Expected failure |
|---|---|---|---|
| 1 | Neither or both of refs/`--all`: exit 2 before the store is read | cli `needs_refs_or_all`, `refs_and_all_conflict` (on an unparseable `resources.ttl`: exit 2 not 3; no data dir created on an absent store; stderr names `--all`) | stderr lacks `--all` (unrecognized subcommand) |
| 2 | R2 refreshed: size, mtime, checksum replaced, catalog `modified` = NOW, record `stale:false`, `show` then not stale | core `refreshes_changed_content` (exact changes: 4 adds, 4 removes); cli `refresh_json` (locked golden `cli_refresh__refresh_json`), `show_after_refresh_not_stale` | unresolved import `hali::core::resource::refresh`; exit 2 |
| 3 | R3 (mtime only) changes only the two `modified` triples | core `touched_changes_only_modified` | unresolved import |
| 4 | R1 is a no-op; `resources.ttl` byte-identical and not rewritten | core `unchanged_is_noop`; cli `unchanged_writes_nothing`, `refresh_text` (exact) | unresolved import; exit 2 |
| 5 | Named refs always hash; `--all` hashes only Stale | core `all_hashes_only_stale` (fake `hash` called exactly for R2 and R3; `stat` once each), `named_ref_always_hashes` (R5 refreshed); cli `sneaky_needs_explicit_ref` | unresolved import |
| 6 | `--all` on `F`: R2 and R3 in id order, R4 in `missing`, `unchanged: 2` | cli `refresh_all_json` (locked golden `cli_refresh__refresh_all_json`) | exit 2 |
| 7 | Missing, directory, FIFO: reported, never opened, no hang | core `missing_never_hashed`; cli `missing_named_ref_exit_0`, `fifo_is_missing_not_opened` (`mkfifo`, 30 s timeout), `directory_is_missing` | unresolved import; exit 2 |
| 8 | Errors abort, nothing written | core `unknown_ref_before_hashing`, `hash_error_aborts_all`, `shacl_guard`; cli `unknown_resource`, `file_unreadable_all_or_nothing` (skipped as root), `symlink_at_path_refused`; `resources.ttl` bytes identical after each | unresolved import; exit 2 ≠ 1 |
| 9 | A Resource named by path and by UUID is processed once | cli `duplicate_refs_once` | exit 2 |
| 10 | `--all` on absent/empty store | cli `all_on_empty_store` (byte-equal stdout; `!data.exists()`) | exit 2 |
| 11 | Only refresh hashes | cli `find_show_do_not_open_files` (mode-000 file with changed size: `show` and `find --stale` exit 0 with `stale:true`, `refresh` gives `FILE_UNREADABLE`); `.goal/checks/01-only_refresh_hashes.sh` (`git grep -n 'inspect' -- src/core` empty; in `src/cli/mod.rs` `storage::inspect` appears only in `inspect_all` and the Refresh arm) | exit 2; `MISSING: no storage::inspect call in the Refresh arm` |
| 12 | `--dry-run` hashes, writes nothing, same payload plus `dry_run`/`changes` equal to the real run's | cli `refresh_dry_run_json` (locked golden `cli_dry_run__refresh_dry_run_json`), `refresh_dry_run_matches_real_run` (`support::assert_dry_run_matches_real_run`), `refresh_dry_run_text`; `dry_run_guard.rs` `CASES` row `("refresh", "store", &["refresh", "--all"])`; `.goal/checks/02-dry_run_guard_refresh.sh` | exit 2; `MISSING: no CASES row for "refresh"` |
| 13 | Never asks | cli `never_asks` (`HALI_TEST_TTY=0`, no `--yes`, stdin closed) | exit 2 |
| 14 | Foreign `spdx:checksum` link replaced, other node left alone | core `foreign_checksum_link_replaced` | unresolved import |
| 15 | Schema `refresh`; `SCHEMAS` 24; goldens validate | `core_schema.rs`/`cli_schema.rs` name lists with `refresh` inserted; locked golden `cli_schema__schema_list_json`; `schema_contract.rs` `GOAL_SNAPS_20`/`GOAL_STDOUT_GOLDENS_20`; `refresh_schema_rejects_unchanged_list` | names mismatch; `schema refresh` exit 2 |
| 16 | Spec and glossary | `.goal/checks/03-spec_refresh.sh`: §9.1 `**Refresh:**` paragraph naming `--all`, `missing`, `unchanged`, `FILE_UNREADABLE`; the §6 bullet says "only Stale"; GLOSSARY.md has `**Refresh**:` (written by the test-author in the goal commit) | a guard, passes by design |
| 17 | No new rule; seams untouched | `.goal/checks/04-seams_untouched.sh`: `git diff --quiet main -- src/storage src/paths src/media_type Cargo.toml Cargo.lock src/core/schemas/error.json`, no new `Error::new("` rule string in `src/core/mod.rs` | a guard |
| 18 | Existing goldens unchanged (proves the `add` refactor) | `.goal/checks/05-existing_goldens_unchanged.sh` (the #18/#19 pattern; `cli_schema__schema_list_json` excepted) | a guard |

`.goal/locked`: `tests/core_refresh.rs`, `tests/cli_refresh.rs`, `tests/fixtures/refresh/resources.ttl`, `tests/snapshots/cli_refresh__refresh_json.snap`, `cli_refresh__refresh_all_json.snap`, `cli_dry_run__refresh_dry_run_json.snap`, `cli_schema__schema_list_json.snap`, `tests/schema_contract.rs`. Per the #19 lesson, if locking a changed schema-list snapshot would turn the settled `cli_schema.rs` red in `make unit`, do not lock it; instead assert the exact 24-name list in a goal-owned test and let the implementer update the snapshot. Goldens are hand-written from the contract and the fixture's known hashes and times.

## Approach

Not cross-cutting: one core function, one CLI arm, one schema. Implementer on its default model.

1. `src/core/resource.rs` (reuses the private `record`, `known`, `checksum_node`, `touch`): `pub enum Target { All, Ids(Vec<String>) }`, `pub struct Hashed { byte_size, modified, sha256 }`, `pub enum Seen { Missing, Hashed(Hashed) }`, `pub struct Refreshed { refreshed, missing, unchanged: usize }` (Serialize, keys in that order), `pub fn refresh(ctx, &mut Store, &Target, stat: Fn(&str)->Option<(u64,i64)>, hash: FnMut(&str)->Result<Seen,Error>) -> Result<Mutation<Refreshed>, Error>`. Extract ONE private `file_quads(resource, byte_size, modified, sha256)` used by both `catalog()` and `refresh` (do not copy the triple code; check 05 and the existing `add` goldens guard it). Build all adds and removes per Resource; call `touch()` only for Resources that change; one `validate::guard`, one `apply`; skip both when nothing changes. Keep a `pub(crate)` per-Resource helper returning `(added, removed)` for #21 and #22.
2. `src/cli/mod.rs`: `Command::Refresh { refs: Vec<String>, #[arg(long)] all: bool }` with a required, non-multiple `ArgGroup`. The arm resolves refs with `resource::resolve_ref(&store, r, paths::ref_path)`, passes `storage::stat` and a closure over `storage::inspect` that maps `Missing`/`Directory` to `Seen::Missing`, `Other` to `not_a_regular_file(path, "other")` and `Unreadable` to `file_unreadable`; no `media_type::detect`; do not reuse `inspect_all` (its missing-is-an-error mapping differs); `observe` each output record; `mutated(…, File::Resources, …, None)`.
3. Schema `src/core/schemas/refresh.json` (`oneOf` real/dry-run, `$defs/resource`, `unchanged` integer ≥ 0, `additionalProperties:false`) and the `SCHEMAS` entry (23 → 24, sorted).
4. Settled tests the implementer updates (not goal-owned): the hardcoded 23 in `tests/cli_relation.rs` and `tests/cli_type.rs` (and any other, grep), the `core_schema`/`cli_schema` name lists, `tests/dry_run_guard.rs` row.

Reuse: `Mutation`, `apply`, `validate::guard`, `mutated()`, `touch()`, `observe`, `resolve_ref`, `printable`. Do NOT copy: the inline triple code in `catalog()` (extract it), `inspect_all`.

## Security call

Not sensitive by the path list if check 04 holds. Behavioural exposure: refresh opens paths taken from `resources.ttl`, which the user can hand-edit. Protections: `stat` first classes every non-regular target as Missing without opening it; `inspect` refuses symlinks and FIFOs swapped in after the stat (`O_NOFOLLOW`, `O_NONBLOCK`, `fstat` re-check); hashing streams in 64 KiB chunks; the only output is the hash and size written to the user's own store; no network. Residual: a regular file on a hung FUSE/NFS mount can block; a huge file takes long to hash. The human approved running the security-reviewer (a new caller of `inspect` on store-controlled paths); the reviewer also checks that stat-before-inspect holds and that nothing else opens files.

## Risks

Second-granularity mtime: a same-size edit in the same second after a hash goes undetected by `--all` (a named ref catches it). A file modified during the hash records the pre-read mtime and heals itself on the next stat. No store lock (#5): a long `--all` can race a concurrent writer and lose its update. The `add` refactor must keep its goldens byte-identical. `--all` lists every Missing Resource, so output grows with their number. The `dry_run_guard` row works only because Missing is reported, not an error.
