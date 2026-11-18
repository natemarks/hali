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

## Update: reviewer rows 19-23
Rows 19, 20, 21 and the spec check fail today; 22 and 23 pass today and fail under the named mutations. Tests are appended to `tests/cli_new.rs` and `tests/core_new.rs`, plus the new locked `tests/paths_new.rs`.
| # | Criterion | Check | Fails today / mutation |
|---|---|---|---|
| 19 | `[files] dir` whose deepest existing prefix is not a directory (`<file>/sub`, `<file>/a/b`, a dangling symlink at files.dir) is `FILES_DIR_UNUSABLE` `not_a_directory` (exit 3), identically with and without `--dry-run`, with `--path-only`, JSON and text; nothing created | `cli_new::a_files_dir_that_cannot_exist_is_not_a_directory_with_and_without_dry_run`; `paths_new::the_deepest_existing_prefix_must_be_a_directory`; `03-spec-new.sh` (**New:** says `deepest existing`) | Fails: real run gives `cannot_create`, `paths` gives `Ok(<file>/sub)` |
| 20 | `..` after a missing component is resolved against the symlink-resolved prefix: `<home>/nope/../link/a` records the REAL `<target>/a` (chosen: resolve, not refuse), dry run and real; spec **New:** names `..` | `cli_new::dotdot_after_a_missing_component_resolves_through_symlinks`; `paths_new::dotdot_after_a_missing_component_uses_the_real_prefix`; `03-spec-new.sh` | Fails: records the lexical `<home>/link/a` |
| 21 | The `new` file slug (`core::resource::file_slug`, not `core::slug`) holds only `char::is_alphanumeric()` or `-`, never starts or ends with `-`, falls back to `untitled`: `İSTANBUL` gives no combining dot (U+0307) | `core_new::{the_slug_has_only_alphanumerics_and_hyphens, a_title_of_only_combining_marks_is_untitled}` | Fails: `"İSTANBUL" gives "i\u{307}stanbul"` |
| 22 | A symlink to a dir in the data or state dir is `own_dir`; so is a real dir behind a symlinked data or state dir; an existing `0555` files.dir is `cannot_create`, nothing created, store bytes unchanged (skipped as root) | `cli_new::{a_symlink_to_an_own_dir_is_own_dir, an_own_dir_that_is_itself_a_symlink_is_resolved, a_read_only_existing_files_dir_is_cannot_create_and_changes_nothing}` | Mutations: `real_target(d)` to `d` in the own-dir test of `src/paths/mod.rs` fails the second; `let real = real_target(&dir)` to `dir.clone()` fails the first; saving the store before `create_new_file` in `src/cli/mod.rs` fails the third |
| 23 | Slug of an NFD title drops the accent: `Re` + U+0301 + `sume` + U+0301 gives `re-sume.md` (pinned; a combining mark is a separator, nothing is normalised); NFC keeps `résumé.md`; `ß`, `ẞ` give `ß`; sigma is always medial | `core_new::{nfd_titles_drop_the_combining_accent_and_split_the_word, sharp_s_and_final_sigma_are_pinned}` | Mutation: `c.to_lowercase()` to `c.to_uppercase()` in `core::slug` fails both |

Not testable, skipped on purpose (no seam, none invented): the post-`create_dirs` re-check in `create_new_file` (`new_files_dir(config)? != files_dir`) and the post-write `is_at_path()` check in `storage::create_new`. `create_dirs` gets an already-canonical path, so the re-check differs only if another process swaps a path component in between; `is_at_path` fails only if the path is swapped after `open`. Both are races a single-process test cannot trigger deterministically, and `storage_new` has no hook between open and the check.
