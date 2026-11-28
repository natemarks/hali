# Plan for #27: agent usage guide, `hali help agents` and `--help` examples

Approved by standing instruction on 2026-11-28 (the maintainer asked for #27 to #32 to run unattended), with all ten planner defaults. The contract (decisions, refined acceptance criteria 1-11, out of scope, security) is the "Groomed contract" section of issue #27. The PR closes #27, #169 and #171.

## Current behaviour

`hali help agents` exits 2 (`unrecognized subcommand 'agents'`); clap builds the `help` subcommand itself. `docs/agent-usage.md` and `skill/` do not exist. No `--help` has examples (no `before_help`/`after_help`). These keep working and existing tests depend on them: `hali help`, `hali help find`, `hali help concept add`, `hali help generate`, `hali help nope` (exit 2, `error: unrecognized subcommand 'nope'`). Tests: `tests/cli_generate.rs`, `tests/cli_bin_name.rs`, `tests/cli_usage_text.rs` (asserts `--help` == `command().styles(Styles::styled()).render_help()`). `tests/core_schema.rs` and `tests/dry_run_guard.rs` skip any subcommand named `help`, so a `help agents` leaf needs no schema and no `CASES` row. Spec: §9.1 `hali help agents [--snippet]`, §11.1 `--help` starts with 2-4 real examples (one `--json`, one `--dry-run`), §11.3 `docs/agent-usage.md` single source (embedded for `help agents`, copied by #28 as `references/usage.md`, linked from README, drift test), §11.2 skill pre-approves `Bash(hali --dry-run *)` as a prefix only, §3.3 `help` takes no lock.

## Seams

- `hali::cli::command()` (the clap tree): example extraction via `get_before_help()`, counting and flag rules, parsing examples and guide mentions with `try_get_matches_from` (`DisplayHelp` counts as success).
- `hali::core::guide` (new, pure): `text() -> &'static str` (`include_str!("../../docs/agent-usage.md")`) and `snippet() -> &'static str` (between the markers).
- The real binary (`CARGO_BIN_EXE_hali`, `env_clear` plus scratch HOME/XDG_*/HALI_DATA_DIR, as in `tests/cli_generate.rs`).
- `docs/agent-usage.md` as a file; `README.md` and `docs/spec.md` via temporary checks.

## Example world (goal-owned test helper, locked)

Built once per test binary through the real binary and copied per example. HOME=tmp/home, XDG_* and HALI_DATA_DIR under tmp, cwd=HOME, default `files.dir` (`~/Documents/hali`). Files: `~/papers/ldp.pdf`, `~/notes/ideas.md`, `~/notes/todo.md`. Concepts `computing` and `ml` (broader `computing`); Types `Paper` and `Note`; relation kind `annotates` under `dcterms:relation`; an unused Concept `drafts` and an unused relation kind `supersedes` (under `dcterms:relation`) so `concept remove` / `relation remove` examples can run; `ldp.pdf` typed Paper and tagged `ml`; `ideas.md` annotates `ldp.pdf`; a remembered Scan root `~/notes` (a real `hali scan ~/notes`); one Missing Resource whose file moved to `~/archive/old.pdf`, so `relink` and `scan --prune` examples have work. Runner: strip a trailing `# …`, expand a leading `~/`, split on whitespace respecting single and double quotes, allow only a leading `HALI_AGENT=<name>`, no shell. Document the world in the helper's module header; examples must fit the world, not the reverse. Examples refer to Resources by path, never by UUID.

## Goal map

Goal-tagged (`#[cfg(feature = "goal")]` on its own line); all run under `make goal-tests`.

| # | Criterion | Check | Expected failure today |
|---|---|---|---|
| 1 | `help agents` prints the guide verbatim, exit 0, empty stderr | `tests/cli_help_agents.rs::prints_the_guide` | exit 2, `unrecognized subcommand 'agents'` |
| 2 | `snippet()` is exactly the marked section, non-empty, <= 25 lines, no marker, names `hali help agents`, `HALI_AGENT`, `hali --dry-run` | `tests/core_guide.rs::snippet_is_the_marked_section` | `unresolved import hali::core::guide` |
| 3 | `help agents --snippet` prints `snippet()` plus LF | `cli_help_agents::prints_the_snippet` | exit 2 |
| 4 | `help agents` reads nothing, changes nothing, same bytes under: `HALI_AGENT` with a control char; `--agent x`; `--json`/`--dry-run`/`--yes` before or after; an unparseable config; an unusable data dir; another cwd; `env -i`; no file created | `cli_help_agents::reads_nothing_and_ignores_global_flags` | exit 2 in every case |
| 5 | `help agents extra` is a usage error naming `extra`; `\| head -1` leaves stderr empty | `cli_help_agents::usage_and_closed_stdout` | stderr lacks `unexpected argument 'extra'` |
| 6 | `help` forms preserved: `help` == `--help`; `help find` == `find --help`; `help concept add` == `concept add --help`; `help generate` names `man` and `completions`; `help nope` exit 2 with `error: unrecognized subcommand 'nope'` (hostile word escaped); the root `help` row names `agents` | `cli_help_agents::help_command_forms` | the `help` row lacks `agents` (the rest are guards) |
| 7 | Every visible command's `--help` and `-h` start with `Examples:\n` and 2-4 `  hali …` lines | `tests/cli_help_examples.rs::every_help_starts_with_examples` | `hali --help` starts with the about line |
| 8 | Flag rules: mutating commands >= 1 `--json` and >= 1 `hali --dry-run ` example; read-only >= 1 `--json`; groups and root both; `help`/`help agents` exempt; `--dry-run` straight after `hali` | `cli_help_examples::examples_follow_the_flag_rules` | `add` has 0 examples |
| 9 | Each example parses with the clap tree and names the documented command (or a descendant for root and groups) | `cli_help_examples::examples_parse` | `0 >= 2 * commands` |
| 10 | Each example exits 0 in a fresh copy of the example world | `cli_help_examples::examples_run` | same count assertion |
| 11 | One insta snapshot of every command path and its Examples block (not locked) | `cli_help_examples::help_examples_snapshot` | no `.snap` |
| 12 | Every `hali …` mention in the guide parses with the clap tree | `tests/agent_usage.rs::every_mention_parses` | `docs/agent-usage.md` missing |
| 13 | ```sh lines run in order in one example world, each with its `# exit N` annotation (default 0) | `agent_usage::sh_examples_run` | file missing |
| 14 | Content: `hali describe --json`, `hali find`, `hali show`, `` `path` ``, `hali --dry-run`, `changes`, `UNKNOWN_CONCEPT`, `suggested_command`, `suggested_fix`, `CONFIRMATION_REQUIRED`, `--yes`, `absolute`, `hali query`, exit codes 0-3 | `agent_usage::covers_the_agent_workflow` | file missing |
| 15 | Identity (#171): `HALI_AGENT`, `--agent`, `self-asserted`, only on `add`/`scan`/`new` | `agent_usage::covers_agent_identity` | file missing |
| 16 | Scan roots (#169): `hali scan --list`, `hali scan --forget-root`, bare `hali scan`, `roots.json`, `scan --prune --yes` forgets without asking | `agent_usage::covers_remembered_scan_roots` | file missing |
| 17 | Hygiene: no §9.3-escaped character except LF, <= 300 lines, each snippet marker exactly once | `agent_usage::guide_is_plain_and_bounded` | file missing |
| 18 | README links the guide | `.goal/checks/27-readme-link.sh` | grep exit 1 |
| 19 | Spec: §11.3 (`help agents` prints `docs/agent-usage.md` verbatim, snippet markers), §11.1 (examples rule), §9.6 (`schema`, `help` and `generate` exempt from the `HALI_AGENT` check) | `.goal/checks/27-spec.sh` | grep exit 1 |

Lock rows 1-10 and 12-17 (`tests/cli_help_agents.rs`, `tests/core_guide.rs`, `tests/cli_help_examples.rs`, `tests/agent_usage.rs`, the example-world helper) in `.goal/locked`; do not lock the row 11 snapshot.

## Approach (implementer, opus; cross-cutting: every command definition gets an attribute)

1. `core::guide` (`src/core/guide.rs`): `include_str!` plus snippet extraction; core test first.
2. `help` rework in `src/cli/mod.rs`: `disable_help_subcommand = true`; `Help { topic: Option<HelpTopic> }` with `HelpTopic::Agents { snippet }` and an `external_subcommand` for other words; dispatch before `injected()`/`execute()` like `Schema` and `Generate`; bare `help` and `help <words>` re-parse `["hali", words…, "--help"]` and print clap's `DisplayHelp` like `parse()`; `help agents` writes with `write_all` and handles EPIPE. Keep `cli_usage_text`, `cli_bin_name`, `cli_generate` green.
3. Examples: `src/cli/examples.rs` with one `const` per command, attached with `#[command(before_help = …)]` on every variant and `Cli`.
4. `docs/agent-usage.md` with the snippet markers near the top, an `sh` walkthrough fitted to the world, sections (a)-(i); README link; spec §9.6, §11.1, §11.3.

No `Cargo.toml`/`Cargo.lock` change; no new dependency (tests use their own quote-aware splitter). `help agents` has no JSON output: no `schema_version` bump, no `hali schema` entry.

## Risks

Disabling clap's help subcommand removes nested `hali concept|type|relation help <leaf>` and trims `hali help <TAB>` completion to `agents`; an example that passes in the world but misleads on a real store (the reviewer reads the snapshot); brittle token checks in rows 14-16; about 90 binary runs for row 10 (copy the world); `include_str!` reaches outside `src/` (fine: `publish = false`, `make dist` builds from the checkout).

## Security

Not sensitive by the CLAUDE.md list; the ordinary reviewer (opus) covers it. Review points: the guide must never advise `--yes` or a real mutation without the user's approval and must warn that `scan --prune --yes` is destructive; examples use the `hali --dry-run` prefix form.

## Test-author notes (define, 2026-11-28)

Locked: `tests/cli_help_agents.rs`, `tests/core_guide.rs`, `tests/cli_help_examples.rs`, `tests/agent_usage.rs`, `tests/example_world/mod.rs` (the world, the splitter and the command classification are documented in its header; read it before writing examples). Row 11's test lives in the locked `cli_help_examples.rs`; only its snapshot `tests/snapshots/cli_help_examples__help_examples.snap` is the implementer's to create (review, then `make unit-update-golden`).

- **Snippet** (rows 2, 3): the lines strictly between the line `<!-- hali:snippet:start -->` and the line `<!-- hali:snippet:end -->`, joined by LF, no LF after the last; `help agents --snippet` prints that plus one LF.
- **Command classification** (row 8), in the helper: mutating = add, new, edit, forget, refresh, scan, relate, relink, unrelate, concept add|rename|edit|remove, type add|remove, relation add|remove; read-only = describe, query, schema, find, show, validate, concept show|list, type list, relation list; groups (concept, type, relation) and the root. A new visible command must be added to those lists in the helper (a goal-owned file; ask the test-author).
- **Example block** (rows 7-10): `before_help` is exactly `Examples:\n` then 2-4 lines `  hali …` (nothing else, no trailing text); `--help` and `-h` print it first, then a blank line. An example parses with `try_get_matches_from` (a help request does not count), names its own command or a descendant, uses `~/` paths, never sets `HALI_AGENT`, and exits 0 in the world. In the world `hali forget ~/papers/ldp.pdf` fails without `--force` (ideas.md annotates it); `hali relink ~/notes/old.pdf ~/archive/old.pdf`, `scan --prune` and `find --missing` have work.
- **Mention extraction** (row 12): see the header of `tests/agent_usage.rs`: fenced-block lines and inline single-backtick spans starting `hali ` or `HALI_AGENT=<name> hali `; a mention made only of `--json`/`--dry-run`/`--yes` after `hali` (the `hali --dry-run` prefix) is accepted without parsing; no ellipses, pipes or other non-arguments inside a mention.
- **`sh` blocks** (row 13): info string exactly `sh`; `#` lines skipped; each other line `[HALI_AGENT=<name>] hali …`, run in file order in one world, `# exit N` trailing comment (default 0); at least 5 such lines.
- **Content tokens** (rows 14-17): exactly the lists in the header of `tests/agent_usage.rs`, including `suggested_fix`, a line that says `exit` with each of 0, 1, 2, 3 as a whole word, the word `consent` or `approval`, a paragraph with `` `add` ``, `` `scan` `` and `` `new` ``, the span `` `hali scan` ``, and a paragraph holding `--prune --yes`, `forget` and `without asking`. The guide must not contain `skill install`.
- **Row 18** (`.goal/checks/27-readme-link.sh`): README holds the literal `](docs/agent-usage.md)`.
- **Row 19** (`.goal/checks/27-spec.sh`), each inside its own `###` section: §11.3 has `` prints `docs/agent-usage.md` verbatim ``, `hali:snippet:start` and `hali:snippet:end`; §11.1 has `` start with an `Examples:` block `` and `` `hali --dry-run` ``; §9.6 has `` except `schema`, `help` and `generate` ``.
- **Not a goal fault:** `make static-check` runs `cargo audit`, which is red on `main` today (RUSTSEC-2026-0592, `url` 2.5.8; a `Cargo.lock` bump is outside this PR). `make fmt-check clippy dead-code` is green.

