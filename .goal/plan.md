# Plan for #25 `hali new` (approved)

The approved contract is the "Groomed contract (approved)" section of issue #25. All eleven open questions took the planner's default (new rule `FILES_DIR_UNUSABLE`, exit 3; umask-default modes 0755/0644; slug cap 128 bytes, `untitled`, no NFC; file mtime set to `now`; relative `[files] dir` is an error for `new` only; check everything first then create, an `EEXIST` race is an error with no retry; JSON `{"schema_version":1,"resource":...}` with schema `new`, verbs `created`/`would create`; `--path-only` wins over `--json`; refuse a `files.dir` that is the data or state dir or inside one; create missing parents and leave the directory after a failure; works with or without #198 merged).

Moderately cross-cutting (core, storage, paths, cli, schema registry, spec) and security-sensitive (`src/storage/`, `src/paths/`; writes a file into the user's documents dir): implementer on opus; security-reviewer required.

## Seams
- Core (pure): `core::resource::new_resource(ctx, &mut store, &NewFile{title, types, tags, dir_real, user_name}, taken: impl Fn(&str)->bool) -> Result<Mutation<Created>, Error>`, `Created{record, path, content, mtime}`; a pure slug/name helper.
- Storage: `storage::create_new(path, bytes, mtime) -> Result<CreatedFile, CreateError>` and `storage::remove_created(&CreatedFile)` appended at the END of `src/storage/mod.rs` (own error enum, function-local `use`).
- Paths: `files_dir` (existing) plus a small check/real-target helper appended at the end of `src/paths/mod.rs`.
- Binary: `assert_cmd` in `tests/cli_new.rs` (HOME, XDG_CONFIG_HOME, HALI_DATA_DIR temp dirs; HALI_TEST_NOW, HALI_TEST_ID_SEED; `/TMP` redaction), `hali schema new`.

## Goal map
Before the change every CLI row fails with `unrecognized subcommand 'new'` and every core/storage row fails to compile (`E0425`).
| # | Criterion | Check |
|---|---|---|
| 1 | Empty store, no files.dir, `new "Weekly review"`: `~/Documents/hali/weekly-review.md` is `# Weekly review\n` (16 B, sha256 `7a74edee...cbd0c`: compute it), dir created, exit 0 | `cli_new::new_json` (locked golden); `core_new::creates_markdown_record` |
| 2 | files.dir chain `[files] dir` (`~`) > user-dirs > `~/Documents/hali`; a symlinked dir records the real path | `paths` tests (extend the existing `files_dir_*`), `cli_new::symlinked_files_dir_records_real_path` |
| 3 | Slug: Unicode kept and lowercased; 128-byte cap; `untitled`; `"../../etc/x"` gives `etc-x.md` inside files.dir | `core_new::slug_rules`, `cli_new::traversal_title_stays_in_dir` |
| 4 | Collisions `-2`, `-3`; an existing file, dir, symlink, dangling symlink or recorded Missing path is taken; pre-existing bytes unchanged; symlink never followed | `core_new::taken_names_skip`, `storage_new::create_new_is_exclusive_nofollow`, `cli_new::collision_suffixes` |
| 5 | Triples = add's + `dcterms:created` + Resource `prov:wasAttributedTo`; `text/markdown`; modified = created = issued = now; file mtime = now; `stale:false`, `editable:true` | `core_new::triples_exact`, `cli_new::file_mtime_is_now` |
| 6 | `--agent claude`/`HALI_AGENT`: SoftwareAgent minted once; record and Resource both attributed to it | `cli_new::new_agent_dry_run_json` (locked golden) |
| 7 | Unknown `--type`/`--tag`: `UNKNOWN_TYPE`/`UNKNOWN_CONCEPT`, exit 1; no dir, no file, store bytes unchanged | `cli_new::unknown_refs_create_nothing` |
| 8 | Empty, whitespace or control (`\n`, `\t`) title, or non-UTF-8 argv: exit 2, nothing created; `#x` gives `# #x\n` | `cli_new::{invalid_titles_usage, hash_title_verbatim}` |
| 9 | `--path-only` prints exactly the path + LF (also with `--json`); with `--dry-run` the would-be path, nothing created | `cli_new::path_only*` |
| 10 | Dry run: no dir, no file, no data dir; payload equals the real run's; `changes` equal the real run's store diff, both in the same env | `cli_new::{new_dry_run_json (locked golden), dry_run_matches_real_same_env}`; `dry_run_guard` CASES row `("new","store",&["new","guard probe"])` |
| 11 | SHACL failure (fixture) gives `SHACL_VIOLATION` and no file; a save failure (`STORE_IO`) removes the created file | `cli_new::{shacl_creates_no_file, store_io_removes_created_file}` |
| 12 | `FILES_DIR_UNUSABLE` for each reason (`unresolved`, `relative`, `not_a_directory`, `own_dir`, `cannot_create`); `NON_UTF8_PATH`; branch in `error.json` | `cli_new::files_dir_unusable_*`, `files_dir_unusable_json_envelope` (locked golden), `schema_contract` rows |
| 13 | Never launches an editor (EDITOR/VISUAL set to a marker script: no marker, stdin never read) | `cli_new::never_launches_editor`; `.goal/checks/01-no-editor.sh` (`git grep -nE 'EDITOR\|VISUAL\|process::Command' src/` is empty) |
| 14 | Text `created\|would create resource ...`, escaped | `cli_new::text_output` |
| 15 | `hali schema new` exists; goldens validate; schema list has `new` | `core_schema` names; updated `*schema_list_json.snap` (three, one added `"new",` each); `schema_contract` rows; `.goal/checks/02-snaps.sh` allows exactly those changes |
| 16 | Modes under umask 022: dir 0755, file 0644 | `cli_new::modes_follow_umask` (`sh -c 'umask 022; exec hali ...'`) |
| 17 | Spec §8, §9.1 synopsis and **New** paragraph, §9.6 scope | `.goal/checks/03-spec-new.sh` |
| 18 | After #198 `new` is in `writes_store` | `.goal/checks/04-writes-store.sh` (if `fn writes_store` exists it must contain `Command::New`; vacuous until #198 lands) |
| guard | Existing goldens unchanged except the three schema-list snaps | `.goal/checks/02-snaps.sh` |

Tests clear HOME, XDG_* and HALI_*. Hand-written locked goldens: `cli_new__new_json`, `cli_new__new_agent_dry_run_json`, `cli_new__new_dry_run_json` (the dry-run one belongs under the `cli_dry_run__` prefix if `schema_contract::dry_run_goldens_named` requires it: follow how earlier goals did it) and `cli_new__files_dir_unusable_json_envelope`.

## Checks as written (define)
- `01-no-editor.sh` (row 13; red until `Command::New` exists, then the grep guard), `02-snaps.sh` (rows 15 and guard), `03-spec-new.sh` (row 17), `04-writes-store.sh` (row 18; vacuous until #198), `05-dry-run-guard-new.sh` (row 10: the `dry_run_guard` CASES row cannot be added before the subcommand exists).
- The two dry-run goldens are `cli_dry_run__new_dry_run_json` and `cli_dry_run__new_agent_dry_run_json` (the `cli_dry_run__` prefix is what `schema_contract::dry_run_goldens_named` needs); `schema_contract` maps `new_agent` to `new`.
- Settled schema-list tests that hard-code 26 names (`core_schema::names_and_get`, `cli_refresh`, `cli_relation`, `cli_type`) and the three `*schema_list_json.snap` are left for the implementer to bump (keeping `make unit` green now); `02-snaps.sh` and `new_is_registered`/`new_schema_is_listed` hold them to it.
- Fixture `tests/fixtures/shacl/new-resource-shape` (an extension shape on `hali:Resource` needing `dcterms:description`) drives the SHACL rows.
