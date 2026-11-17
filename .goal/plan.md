# Plan for #5 Store locking and concurrent access (approved)

The approved contract is the "Groomed contract (approved)" section of issue #5. All ten open questions took the planner's default (lock held across prompts; long writers block readers for v1; `.lock` file as the issue says; refuse another owner or g+w/o+w data dirs, allow read bits; 256 MiB cap; symlinked `extensions.ttl` refused; absent data dir at first write: `STORE_LOCKED` reason `changed`; no hook to shorten the 5 s wait; `roots.json` races and FIFO `config.toml` out of scope; NFS unsupported).

Cross-cutting and security-sensitive (`src/storage/`, `src/paths/`): implementer on opus; security-reviewer required.

## Seams
- CLI (`assert_cmd`): exit codes, stderr envelopes and timing. The test process holds `<data>/.lock` itself with std `File::lock`/`lock_shared`.
- `storage` (lib): `storage::Writer` (`open(dir)`, `load()`, `save(store, file)`), `storage::load(dir)` (shared lock inside), pure `dir_problem(uid, mode, euid)`.
- `hali schema error`: the `STORE_LOCKED` branch.
- Test hook `HALI_TEST_PUBLISH_STALL=<marker>` (cfg `test-hooks`).

## Goal map
| # | Criterion | Check |
|---|---|---|
| 1 | Writers exclusive, readers shared, on `<data>/.lock` | `cli_lock::writer_blocked_by_shared_lock` (`concept add` exits 3 `STORE_LOCKED`, vocabulary bytes unchanged); `cli_lock::reader_and_dry_run_blocked_by_exclusive_lock` (`find`, `--dry-run concept add`); `cli_lock::waiter_proceeds_when_released` (exclusive dropped after 1 s; writer exits 0, elapsed >= 0.9 s); `cli_lock::lock_file_created_only_by_real_writes` (0600 regular after `concept add`; none after a reader or dry run on an existing dir); `storage_lock::first_write_race_is_changed` (two Writers on an absent dir; the second save fails with reason `changed`; `open` creates nothing) |
| 2 | Each changed file published atomically | `cli_publish::publish_renames` (new inode, mode 0600, no `.tmp` left) |
| 3 | `STORE_LOCKED` after 5 s, exit 3 | `cli_lock::store_locked_json_envelope` (5 s <= elapsed < 10 s; locked `tests/snapshots/cli_lock__store_locked_json_envelope.snap`); `cli_lock::store_locked_text`; `cli_lock::schema_has_store_locked_branch` (an envelope without `details.path` is rejected by `hali schema error`) |
| 4 | §14.4 concurrency: ~8 processes, no parse failure, no lost write | `cli_concurrency::overlapping_readers_and_writers` (4 writers each add a unique Concept, 4 readers loop `concept list --json`, 5 rounds) |
| 5 | Killing a writer mid-publish leaves the old bytes | `cli_publish::kill_mid_publish` (stall hook, SIGKILL, bytes equal, a reader parses, the next writer succeeds); `cli_publish::failed_publish_keeps_old_bytes` (a directory at the temp path gives `STORE_IO`) |
| 6 | Temp opened with O_EXCL; a planted file or symlink never followed | `cli_publish::planted_temp_symlink` (victim unchanged, temp path gone, store updated) |
| 7 | A symlinked store file is refused with `STORE_IO` | `cli_store_safety::symlinked_store_files` (vocabulary/resources on write and read; extensions on read; target and link unchanged) |
| 8 | Looser or foreign data dir is `STORE_IO`; new files private | `cli_store_safety::loose_data_dir` (0777 and 0770 refused, 0755 accepted); `storage_lock::dir_problem_rules` (foreign uid, g+w, o+w); `cli_store_safety::new_files_private_under_umask_000` (`sh -c 'umask 000; exec hali ...'`) |
| 9 | A FIFO is `STORE_IO`, not a hang; oversized file rejected before load | `cli_store_safety::fifo_store_file` (timeout 10 s); `cli_store_safety::oversized_store_file` (sparse 256 MiB + 1) |
| 10 | A prompt holds the lock | `cli_lock::forget_prompt_holds_lock` (`HALI_TEST_TTY=1`, piped stdin; a concurrent `add` exits 3; answer "y"; forget exits 0) |
| 11 | Spec updated | `.goal/checks/01-spec_locking.sh` (§3.3 has `.lock`, `256 MiB`, `reason`; §14.2 has `HALI_TEST_PUBLISH_STALL`) |
| guard | Existing goldens unchanged except where a row says so; existing helpers that list the data dir ignore `.lock` | `.goal/checks/02-existing_goldens_unchanged.sh`; edits in `cli_relink`, `cli_scan`, `cli_scan_roots` helpers (`state`, `tree`, `listing`, `tree_print`) by the test-author |

Tests clear HOME, XDG_* and HALI_* and point them at temp dirs. About three tests wait the real 5 s.
