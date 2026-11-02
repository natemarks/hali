# Plan: #117 Escape clap usage errors that echo argv

Contract: see issue #117 ("Contract (groomed)"). Approved with all planner recommendations (open questions 1-6): Q1(b) escape clap's context values in full (a `\n` in an argument cannot add a line); Q2 second parse only for usage errors (help stays coloured and byte-identical); Q3 `bin_name` is a follow-up; Q4 no §9.4/`--json` wording (#84); Q5 `#[ignore]` pty test plus a goal check; Q6 append the §9.3 sentence after whatever is there.

## Goal map

Files (new, goal-tagged, locked): `tests/usage_text.rs` (unit, over `hali::cli::usage_text` and `hali::cli::command`) and `tests/cli_usage_text.rs` (CLI, assert_cmd; pty cases in a `pty` module via `/usr/bin/script -qec`, the pattern of `tests/cli_forget.rs::real_pty_prompts`: pass the hostile arguments as environment variables so `/bin/sh` needs no `$'…'`). No `.snap` files. H = `a ESC ]0;PWN BEL ESC [2J CR U+009B U+202E U+2028 U+200B U+E0041 b`.

| # | Criterion | Check | Expected failure |
|---|---|---|---|
| 1 | On a pty, `hali find --limit <H-with-OSC>` leaves no ESC and no C1 control, shows `'a\u{1b}]0;PWN\u{7}\u{1b}[2Jb'`, exit 2 | `cli_usage_text::pty::hostile_value_on_a_pty` (`#[ignore = "needs script(1) from util-linux"]`); `.goal/checks/03-pty.sh` runs `cargo test --features test-hooks,goal --test cli_usage_text -- --ignored` | `assert!(!out.contains(&0x1b))` fails: raw `033 ] 0 ; P W N` |
| 2 | The same on a pty for an unknown subcommand, an unknown flag (`--H`), and labels rejected by `concept add`, `type add` and `relation add` | `cli_usage_text::pty::every_family_on_a_pty` (ignored, loop) | raw `033` in the output |
| 3 | Without a pty, the §9.3 set in an argument is printed as `\u{..}` for every family: unknown subcommand, unknown flag, invalid value (`find --limit`, `find --media`), `concept add`/`type add`/`relation add`, `schema <x>`, `help <x>`; exit 2, nothing on stdout | `cli_usage_text::families_escape` (exact stderr literals: today's text with the escapes inserted); `usage_text::escape_set` and `usage_text::kept_set` over `usage_text(command().try_get_matches_from(["hali", &x]).unwrap_err())` (the #73 escaped and kept lists, written out independently, including the boundary neighbours of #121) | `assert_eq!` diff: `'zz\r'` vs `'zz\u{d}'`; `cannot find function usage_text in module hali::cli` |
| 4 | The usage block stays multi-line; ordinary usage errors are byte-identical to today's output without a TTY | `cli_usage_text::ordinary_unchanged` (exact literals captured from today's binary for `--bogus`, `fnd` (with tip), `concept add` (missing `<LABEL>`), `edit x` (missing group), `describe extra`, `schema nope`, `refresh`, `refresh --all R1`); `usage_text::matches_stripped_clap` (for ordinary argv, `usage_text(plain err)` equals the stripped `render().to_string()` of the styled error) | none for the CLI test: a guard, green today; compile error for the unit test |
| 4b | An argument holding `\n` cannot add a line: `hali $'zz\nerror: forged'` prints `'zz\u{a}error: forged'` with today's line count | `cli_usage_text::newline_in_argv`, `usage_text::newline_in_value` | the line count is one higher |
| 5 | `--help`, `concept add --help` and `--version` unchanged: stdout, exit 0; `hali` and `hali concept` unchanged: help on stderr, exit 2; every usage error exits 2 | `cli_usage_text::help_and_version_unchanged` (stdout equals `command().render_long_help()` / `render_help()` computed from the styled command, and `hali {CARGO_PKG_VERSION}\n`); the pty variant asserts exit 0 | none: a guard, green today |
| 6 | §9.3 has the residue sentence | `.goal/checks/02-spec.sh`: `grep -F` for the exact sentence in `docs/spec.md` | `MISSING: §9.3 residue sentence` |
| 7 | §9.4 says how clap usage errors are rendered | same script: `grep -F "A usage error that clap reports itself"` | `MISSING: §9.4 usage-error wording` |
| 8 | Under `--json`, a usage error with H prints no raw §9.3 character other than `\n` and exits 2 | `cli_usage_text::json_mode_escaped` (does not assert the exact text) | raw `\r` and U+202E found |
| 9 | Escaped in one place | `.goal/checks/01-one-place.sh`: `git grep` for `Cli::parse()` and `\.exit()` in `src/cli` returns nothing; `'\u{2028}'` occurs exactly once under `src/`; `fn usage_text` exists | `FOUND: Cli::parse() at src/cli/mod.rs:450`, `FOUND: .exit() at …:499` |

`.goal/locked`: `tests/usage_text.rs`, `tests/cli_usage_text.rs`. The test-author writes the §9.3 and §9.4 sentences into the goal commit (as #73 did), so check 02 passes by design.

## Approach

Not cross-cutting. Implementer on its default model. The code changes are in `src/cli/mod.rs` only, plus the spec and tests.

1. Factor the §9.3 list out of `printable` into `fn escaped(c: char) -> bool`; `printable` escapes where `escaped(c)`; a new private `printable_lines` escapes where `escaped(c) && c != '\n'`. Do not copy the list.
2. `pub fn command()` returns `Cli::command().styles(Styles::plain())` (its two existing callers in `tests/core_schema.rs` and `tests/dry_run_guard.rs` only walk subcommands).
3. `pub fn usage_text(mut err: clap::Error) -> String`: replace every context value except `ContextKind::Usage` with its `printable` form (`String`, `Strings`, `StyledStr`, `StyledStrs`; kinds added later escaped by default), then `printable_lines(&err.render().ansi().to_string())`. Confirm the signatures against the vendored clap_builder 4.6.7 source.
4. `fn parse() -> Result<Cli, ExitCode>` called first in `run`: `Cli::try_parse()`; help/version/missing-subcommand display kinds → `err.print()` and the clap exit code; other errors → second parse with `command().try_get_matches().and_then(|mut m| Cli::from_arg_matches_mut(&mut m))`, `usage_text`, `let _ = std::io::stderr().write_all(..)`, `ExitCode::from(2)`; fall back to the first error if the second parse unexpectedly succeeds.
5. `show_schema`: `return usage(command().error(ErrorKind::InvalidValue, msg))`, returning `ExitCode::from(2)`; no `.exit()` left.
6. Spec §9.3 and §9.4 sentences (in the goal commit).

Reuse: `printable`, the `real_pty_prompts` pattern. Do NOT copy: a second character list, or a per-site escape; do NOT use `Error::render().to_string()` (its `Display` strips ESC sequences and would hide the hostile input instead of showing it escaped); no pty crate (Cargo.toml is a guarded path).

## Security call

Sensitive: run the security-reviewer. This closes the last path where attacker-controlled argv reaches a terminal as live escape sequences; the issue is `security`. It touches no listed security-sensitive path and no dependency. The reviewer probes: any clap print path left (`print`/`exit` for kinds that echo argv); `bin_name` from argv[0] in help and usage (a follow-up); non-UTF-8 argv (clap shows lossy U+FFFD, safe); the `tip:` lines; `--json`; that the styled fallback can never print a raw ESC; that the second parse cannot be pushed into a different error.

## Risks

The second parse could diverge from the first (parsing is deterministic and the value parsers are pure core normalisers; the fallback stays safe). A clap upgrade that changes the usage context or error template shows up in the byte-identical guard; `ansi()` needs clap's `color` feature (default). Usage errors lose their colour on a TTY. A literal argv text `\u{1b}` looks the same as an escaped ESC (accepted in #73). The pty test needs `script(1)`, so it cannot run in `make unit`.
