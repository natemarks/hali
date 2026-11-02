# Plan: #73 Escape terminal control characters in text-mode errors

Contract: see issue #73 ("Contract (groomed)"). Approved with all planner recommendations (open questions 1-7); question 1 corrects the triage brief: only U+200B of U+200B-U+200D is escaped (ZWNJ and ZWJ are kept).

## Goal map

Goal files (new, goal-tagged, locked): `tests/error_text.rs` (unit, over `hali::cli::error_text`) and `tests/cli_error_text.rs` (CLI, with its own small `Env` like the other `cli_*.rs` files, hand-edited Turtle written inline with IRIs such as `hali:bi‮di`). There are no locked `.snap` files: no text-mode goldens exist and the hostile cases are exact-string or no-raw-character asserts. H means `"a\u{1b}]0;x\u{7}b\nerror: y\u{202e}\u{2028}\u{61c}\u{feff}\u{200b}c"`.

| # | Criterion | Check | Expected failure |
|---|---|---|---|
| 1 | Given any `Error` constructor fed H in every text argument, text mode renders no character of the escaped set, exactly one `error:` and one `hint:` line, and holds `\u{1b}` | `tests/error_text.rs::every_constructor_escapes_outside_text` (table: usage, label_taken, in_use, type_in_use, type_label_taken, relation_in_use, relation_label_taken, forget_blocked_by_relations, store_io, store_unparseable, config_invalid, confirmation_required, not_a_regular_file, non_utf8_path, file_unreadable, unknown_resource, unknown_concept, unknown_type, unknown_relation, shacl_violation, shapes_invalid) | `error[E0425]: cannot find function error_text in module hali::cli` |
| 2 | Ordinary text (`a;b`, `it's`, `café 日本`, `👨‍👩‍👧`) renders byte for byte as `error: {message}\nhint: {fix} Try: {cmd}\n` | `tests/error_text.rs::ordinary_text_is_unchanged`; the existing text asserts in `cli_concept.rs`, `cli_type.rs`, `cli_relation.rs`, `cli_tag.rs`, `cli_shacl.rs`, `cli_validate.rs` stay green | same compile error |
| 3 | Each character of the escaped set is shown as `\u{hex}`; letters, CJK, emoji, combining marks, VS16, U+FFFD, NBSP, U+200C and U+200D are kept | `tests/error_text.rs::escape_set` (one `unknown_resource` per character); the implementer also extends `printable_escapes_controls_and_keeps_letters` | compile error, then `assertion failed: escaped "\u{2028}"` for U+2028, U+2029, U+061C, U+FEFF, U+200B, U+00AD, U+2060, U+E0041 |
| 4 | A typed reference with ESC, OSC or newline fails with no raw control and exactly 2 stderr lines | `tests/cli_error_text.rs::{unknown_concept,unknown_type,unknown_resource,unknown_relation}_text_escapes` (`find --tag H`, `find --type H`, `show H`, `relate R1 H R2`) | `raw ESC on stderr`; 3 lines instead of 2 |
| 5 | `hali refresh <uncataloged name with ESC and newline>`: exactly two lines, no raw ESC | `refresh_unknown_ref_two_lines` | 3 lines instead of 2 |
| 6 | A hand-edited Concept `hali:bi‮di`: `concept remove` (IN_USE, and CONFIRMATION_REQUIRED with `HALI_TEST_TTY=0`), `concept add`, `concept rename`, `concept edit --add-alt` (LABEL_TAKEN) show `\u{202e}` and no raw U+202E | `concept_stored_id_escapes` (5 cases) | `raw U+202E on stderr` |
| 7 | The same for a Type `hali:Bi‮di` (IN_USE via a subtype, LABEL_TAKEN via `type add`) and a relation kind `hali:bi‮di` (IN_USE via a sub-relation, LABEL_TAKEN via `relation add`) | `{type,relation}_stored_id_escapes` | `raw U+202E on stderr` |
| 8 | Outside paths with ESC: `HALI_DATA_DIR` naming a file (STORE_IO), `XDG_CONFIG_HOME` with bad TOML (CONFIG_INVALID), `add` of a missing path (NOT_A_REGULAR_FILE), `add` of a non-UTF-8 path (NON_UTF8_PATH) | `outside_paths_escape` | `raw ESC on stderr` |
| 9 | Refresh's store-path errors stay escaped after the per-site escape is removed; JSON keeps the raw path | the existing `tests/cli_refresh.rs` three tests, unchanged | none: a regression guard, green before and after |
| 10 | No per-site escape is left | `.goal/checks/01-central_escape.sh`: fails if `src/cli/mod.rs` matches `err.message = printable`, or if `fn report` does not call `error_text` | `FOUND: per-site escape at src/cli/mod.rs:685` |
| 11 | `--json` stays byte-identical: for H, `message`, `details.*` and `suggested_command` hold the raw values on one line | `tests/cli_error_text.rs::json_envelope_keeps_raw_values`; all `*_json_envelope.snap` unchanged | none: a guard, green before the change |
| 12 | A fix's example command never has a bare `-id` argument | `tests/error_text.rs::fix_prose_dash_ids`: `relation_in_use("-evil",1,0)` fix holds `hali unrelate <ref> -- -evil <ref>`; `in_use("-evil",1,0)` holds `--remove-tag=-evil` and `--remove-broader=-evil`; `forget_blocked_by_relations("-evil",1,1)` holds `<relation> -- -evil` | `assertion failed: fix contains "-- -evil"` |
| 13 | The spec states the escaping | `.goal/checks/02-spec_text_errors.sh`: greps for the two agreed §9.3 and §9.4 sentences | `MISSING: §9.4 does not say the message, the suggested fix and the suggested command are each escaped for the terminal` |

`.goal/locked`: `tests/error_text.rs`, `tests/cli_error_text.rs`. The test-author writes the §9.3 and §9.4 spec text into the goal commit (as #15 to #20 did), so check 02 passes by design.

## Approach

Small, not cross-cutting. Implementer on its default model.

1. `src/cli/mod.rs`: add `pub fn error_text(err: &Error) -> String` and make `report()`'s text branch `eprint!("{}", error_text(err))`.
2. Widen `printable`'s `matches!` to the escaped set as one `matches!` with ranges and a comment per group; extend `printable_escapes_controls_and_keeps_letters`.
3. Delete the refresh `.map_err` (`src/cli/mod.rs` ~681-688).
4. `src/core/mod.rs`: switch three fix strings to `positional` / `flag_value`: `in_use` (~337-340), `relation_in_use` (~396-399), `forget_blocked_by_relations` (~429-432). Nothing else in `core` changes.
5. Spec §9.3 and §9.4 (in the goal commit).

Reuse: `printable`, `positional`, `flag_value`. Do NOT copy: a per-constructor or per-call-site escape (the refresh `.map_err` pattern), or escaping inside `core::Error` (it would change JSON and break transport-agnosticism).

## Security call

Sensitive: run the security-reviewer. This is output hardening against terminal injection, display spoofing and line forgery from untrusted argv, store and env text; agents that forward file names are the likely victims. `src/cli` is not a guarded path and the diff touches no guarded path or dependency. Residual risk a human accepted: clap usage errors still echo argv (tracked in a follow-up issue labelled `security` that blocks v0.1.0).

## Risks

The wider set changes stdout success lines for labels or paths that contain the characters (only the U+202E dry-run test touches this, and it already expects escaping). Tag characters make subdivision-flag emoji display as escapes. A hostile `suggested_command` is no longer copy-paste-runnable (decided; the exact command is in `--json`).
