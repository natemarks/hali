# Plan: #123 Fix argv[0] / bin_name in clap usage and help output

Contract: see issue #123 ("Contract (groomed)"). Approved with all planner recommendations (open questions 1-6).

## Goal map

New file `tests/cli_bin_name.rs` (goal-tagged, locked). The runner uses `std::process::Command` with `std::os::unix::process::CommandExt::arg0` and `wait_with_output` with a timeout (`assert_cmd` has no `arg0`); clear the environment and set HOME, XDG_CONFIG_HOME, HALI_DATA_DIR and HALI_TEST_TTY=0; no store needed; nothing is mode-based, so nothing is skipped for root. Copy only small helpers (`is_escaped`, the composite H of #117, the `script -qec` runner adjusted to run a link path from an env var); do NOT change `tests/cli_usage_text.rs` or `tests/usage_text.rs`.

- **Names (N):** `\n`; `x\nerror: forged`; CR; TAB; ESC CSI `ESC[2J`; OSC with BEL and with ST (`ESC]0;PWN ESC\`); U+009B; U+202E; U+2028; U+E0041; #117's composite H; a 4096-character name.
- **Args (A), with exit codes:** `zz` → 2; `concept add` → 2; `--help` → 0; `help concept` → 0; `concept add --help` → 0; `--version` → 0; bare (no args) → 2; `concept` → 2; `schema zz` → 2.
- **Oracle `same(name, args)`:** exit code, stdout and stderr are byte-identical to the run with `arg0("hali")`, the line count is equal, and no character other than `\n` is in the §9.3 set (#117's `is_escaped`).

| # | Criterion | Check | Expected failure |
|---|---|---|---|
| 1 | A hostile `argv[0]` (any N) gives output byte-identical to the `hali` run for every A, with the same exit code and line count, and no raw §9.3 character except newline | `cli_bin_name::hostile_arg0_matches_hali` (every N × every A, `same`) | `assert_eq!` left `"…Usage: x\nerror: forged\\u{1b}]0;PWN… [OPTIONS] <COMMAND>…"`, right `"…Usage: hali [OPTIONS] <COMMAND>…"` (`--version` and `schema zz` already pass) |
| 1a | The oracle is anchored to exact literals | `cli_bin_name::anchors`: under the name `x\nerror: forged`, `zz` stderr equals `"error: unrecognized subcommand 'zz'\n\nUsage: hali [OPTIONS] <COMMAND>\n\nFor more information, try '--help'.\n"`; `concept add` stderr equals `"error: the following required arguments were not provided:\n  <LABEL>\n\nUsage: hali concept add <LABEL>\n\nFor more information, try '--help'.\n"`; `--help` stdout equals `command().styles(Styles::styled()).render_help().to_string()`; `--version` equals `"hali {CARGO_PKG_VERSION}\n"` | the usage literal mismatches, as in row 1 |
| 2 | An ordinary different name (`h`, `/some/dir/h`, `foo/`), or a symlink or hard link named `h` or a hostile name, prints `hali` | `cli_bin_name::renamed_and_linked` (`same` over `zz`, `--help`, bare; links in `tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR"))` so a hard link stays on one filesystem) | `Usage: h [OPTIONS] <COMMAND>` vs `Usage: hali [OPTIONS] <COMMAND>` |
| 3 | Empty, non-UTF-8 (`\xff\xfe`, `x\n\xff`), `..` and `/` `argv[0]` give the `hali` output | `cli_bin_name::degenerate_arg0` | none today (clap already falls back): a guard against a clap upgrade |
| 4 | The shared definition has the fixed name | `cli_bin_name::command_bin_name`: `command().get_bin_name() == Some("hali")`, and `command().try_get_matches_from([H, "zz"])` renders with `Usage: hali [OPTIONS] <COMMAND>` | `assert_eq!(None, Some("hali"))` |
| 5 | On a pty, a hostile name through `--help`, bare `hali` and bare `hali concept` gives bytes (CRLF→LF) equal to the same run through a symlink named `hali`: no live OSC, CR or U+009B; `zz` has no ESC at all | `cli_bin_name::pty::hostile_name_on_a_pty` (`#[ignore = "needs script(1) from util-linux"]`; `script -qec "\"$HALI_LINK\" <args>"` with the link in a temp dir, since dash has no `exec -a`) | byte mismatch, ESC count 57 vs 55: `\x1b[1ma\x1b]0;PWN\x07\x1b[2J\r\xc2\x9bb\x1b[0m` vs `\x1b[1mhali\x1b[0m` |
| 5a | The runner | `.goal/checks/03-pty.sh`: `cargo test --features test-hooks,goal --test cli_bin_name -- --ignored`; prints `PASS (skipped)` without `/usr/bin/script` (copy #117's) | `FAILED: the pty tests of tests/cli_bin_name.rs … do not pass` |
| 6 | Ordinary output under the name `hali` is byte-identical to today's | the existing `cli_usage_text::goal::ordinary_unchanged` and `help_and_version_unchanged` (settled: `cli_usage_text::{ordinary_unchanged, help_and_version_unchanged}`), and `tests/version.rs`, unchanged | none: guards |
| 7 | Nothing else reads the program name, and `usage_text` and `parse` are unchanged | `.goal/checks/01-no-argv0.sh`: (a) `git grep -nE 'args_os\|current_exe\|\barg0\b\|env::args\|/proc/self' -- src` is empty; (b) the bodies of `fn usage_text` and `fn parse` in `src/cli/mod.rs` equal those at `$(git merge-base HEAD origin/main)` | none: a guard; fails with `FOUND: program-name read in src/…` or `CHANGED: usage_text/parse` |
| 8 | The spec states the fixed name | `.goal/checks/02-spec.sh`: `grep -qF` the §9 sentence in `docs/spec.md` (the test-author adds the spec text in the goal commit, so this passes by design) | `MISSING: §9 program-name sentence in docs/spec.md` if absent |

`.goal/locked`: `tests/cli_bin_name.rs`.

## Approach

Not cross-cutting: a one-line change in `src/cli/mod.rs`. The implementer runs on its default model.
1. The test-author writes `tests/cli_bin_name.rs`, the three checks and the spec sentence in a `goal: define #123` commit, and proves satisfiability with a throwaway attribute (run `make goal-tests` and `03-pty.sh`, then revert completely).
2. The implementer adds `bin_name = "hali"` to the derive attribute on `Cli`.

Do NOT escape the program name in `usage_text` or the help path, and do NOT change `display_name`, styles, `usage_text` or `parse`.

## Security call

Sensitive: run the security-reviewer (the issue is labelled `security`; untrusted process input, `argv[0]`, reaches a terminal). No listed path is touched and no dependency added. It should probe: every output path (usage errors at root and in subcommands, `--help`/`-h`, `help <sub>`, `<sub> help <sub>`, bare commands, `--version`, the `schema` error, the `tip:` lines); `exec -a`, symlink and hard-link names (newline, CR, TAB, C0/C1, OSC with BEL and ST, bidi, U+2028, tag characters, a very long name, empty, non-UTF-8, names ending in `/`); that nothing reads `/proc/self/exe` or `current_exe`; pty output against the `hali` baseline; that `Multicall` and `NoBinaryName` stay unset.

## Risks

A clap upgrade could change how `argv[0]` or `bin_name` is handled (rows 3 and 4 guard it). A user who deliberately renames the binary sees `hali` (intended; #4's completions and man page agree: `clap_complete::generate` takes the bin name explicitly, `clap_mangen` uses `name`). Rust std accepts an empty `arg0` on glibc.
