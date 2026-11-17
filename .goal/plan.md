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
