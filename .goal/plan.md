# Plan for #11 Editable rule and `[open]` config (approved)

The approved contract is the "Groomed contract (approved)" section of issue #11. All six open questions took the planner's default (config entries match exactly or by `type/*`, not subclasses; built-in exceptions match exactly; no `schema_version` bump; `*/*` rejected; `CONFIG_INVALID` hint lists `[open]`; `.desktop` follow-up for the plugin).

Not cross-cutting (one `record()` path): implementer on its default model. Security-sensitive (`src/media_type/`): security-reviewer required.

## Seams
- `hali::media_type::is_text(&str) -> bool`: iterative BFS over `get_parents` with a visited set and a node cap; never `xdg_mime::mime_type_subclass`.
- `core::resource::editable(media_type, &OpenConfig, is_text: impl Fn(&str) -> bool) -> bool` (pure precedence logic).
- `core::config::parse` -> `Config.open: OpenConfig { editor, system: Vec<MediaPattern> }`; `Context::with_open`/`open()`.
- CLI JSON (`assert_cmd` + `insta`) with a hermetic fixture MIME database: `XDG_DATA_DIRS=tests/fixtures/mime-db`, `HOME` a temp dir; fixture `tests/fixtures/mime-db/mime/{globs2,subclasses,aliases}` with sh, json -> json5 -> text/javascript, toml, xml, svg, rtf, ps, desktop, pdf, html, md, txt, alias `application/x-yaml` -> `application/yaml`, and a cycle `application/x-loop-a` <-> `x-loop-b` (glob `*.loop`).

## Goal map
| # | Criterion | Check |
|---|---|---|
| 1 | is_a over the subclass tree, implicit `text/*` at every ancestor, aliases resolved | `core_open::is_a_rule` (fake `is_text`), `cli_open::subclasses_of_text_are_editable` (fixture: sh, json, toml, yaml alias true); `#[ignore]` external: `media_type::is_text_real_db` (sh, json, toml, xml true; pdf false) and `tests/cli_resource.rs` ~463 `editable == false` becomes `true` for the type it covers |
| 1b | Cycle-safe | `cli_open::cyclic_subclasses_do_not_crash` (`add x.loop`: exit 0, editable false; guard, passes today) |
| 2 | The five built-ins are never editable | `core_open::builtins_never_editable`; golden row `page.html` |
| 3 | Precedence `open.system` > `open.editor` > built-ins > is_a; `type/*`; case-insensitive | `core_open::precedence_matrix` (editor `text/html` T; editor `image/*` svg T; system `text/*` md F; csv in both F; editor pdf T; `Text/CSV` matches); `cli_open::find_open_config_json` + locked `cli_open__find_open_config_json.snap` |
| 4 | md and txt editable, PDF not | locked `cli_open__add_editable_json.snap`: `add --json` of notes.txt, README.md, page.html, run.sh, data.json, cfg.toml, paper.pdf, icon.svg, app.desktop, doc.rtf, x.ps, blob.bin |
| 6 | `[open]` validated, `CONFIG_INVALID` | `core_open::{open_config_parses, bad_open_config_is_config_invalid}` (each bad case in the contract), `cli_open::bad_open_config_exits_3` (`find --json`: exit 3, nothing written); update expected fix text in `tests/cli_scan.rs` ~485 and `tests/cli_error_text.rs` ~436 |
| 7 | Every record honours `[open]` (add, show with nested relation records, find, scan, dry run) | `cli_open::config_reaches_every_record` (system=`["text/plain"]`: every editable false) |
| 8 | No database / unknown / empty type: implicit rule only, never panics | `cli_open::no_mime_db` (`XDG_DATA_DIRS` at an empty dir; hand-written store with `text/x-foo`, `application/x-shellscript`, `""` gives T, F, F); `core_open::empty_type_not_editable` |
| 9 | Shape unchanged; existing goldens unchanged | `.goal/checks/03-existing_goldens_unchanged.sh` (no `tests/snapshots/*.snap` modified or deleted vs `origin/main`; no existing snap legitimately changes: they only hold `text/plain`, `text/x-forget`, `application/octet-stream`) |
| 10 | Spec documents the rules | `.goal/checks/01-spec_open.sh` (§5.2: ancestors, aliases, cycles, no database, exact or `type/*`, `CONFIG_INVALID`; §4.1 drops `[open]` from "come from their deciding tickets") |
| 5 | §14.1 discipline | covered by rows 1-8 |

Locked: `tests/fixtures/mime-db/`, both new `.snap` files, `tests/core_open.rs`, `tests/cli_open.rs`.

Tests set HOME, XDG_STATE_HOME, XDG_DATA_DIRS and the data dir to temp or fixture dirs.

## Review rows (update, reviewer + security-reviewer)
| # | Criterion | Check | Expected failure today |
|---|---|---|---|
| 11 | A FIFO, a symlink (to a FIFO or a file) or a directory at `$XDG_DATA_HOME/mime/{subclasses,aliases}` is skipped as absent, never opened blocking; `find` exits 0 within 5 s and falls back to the next directory | `cli_open::non_regular_database_files_are_skipped` | FIFO: `find` killed after 5 s (hang); symlink-to-file: pdf editable |
| 12 | A relative `XDG_DATA_HOME` or `HOME` is ignored (like `XDG_DATA_DIRS` entries and `paths::xdg_dir`) | `cli_open::relative_data_home_and_home_are_ignored` (absolute controls pass) | "relative XDG_DATA_HOME was read" |
| 13 | A hostile database has bounded cost: (a) a huge fan-out is cheap per Resource (memo or edge cap), (b) the same directory 50 times is read once, (c) 50 hard-linked directories stay inside a total byte budget | `cli_open::{fanout_database_is_cheap_per_resource, duplicate_data_dirs_are_read_once, total_database_bytes_are_budgeted}` (time proxies: 11.6 s, 8.4 s, 8.4 s today on a debug build; bound 3 s) | `find took ...` |
| 14 | `config_reaches_every_record` also covers `--dry-run` of edit, refresh, forget, relink | `cli_open::config_reaches_every_record` | passes today |
| 15 | Fixture cases: alias parent, merged dirs (first wins aliases, merged parents), HOME fallback, malformed lines / non-UTF-8 / case folding, the 4 MiB file cap, the 256-type cap | `cli_open::{database_parent_that_is_an_alias_is_resolved, database_directories_are_merged_user_first, database_falls_back_to_home_local_share, malformed_database_lines_are_ignored_and_case_is_folded, database_file_is_read_up_to_a_cap, long_subclass_chains_stop_at_256_types}` | pass today; each fails under its mutation |
| 16 | The `CONFIG_INVALID` fix text is exact | `tests/cli_scan.rs` (two), `tests/cli_error_text.rs` (the `*` prefix mechanism is gone) | passes today |
| 10+ | Spec: the database is read from absolute directories and regular files only; the pointer reads "§7 and §9.1 (**Scan:**)" | `.goal/checks/01-spec_open.sh` | MISSING / WRONG lines |
