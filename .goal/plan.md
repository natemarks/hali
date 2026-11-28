# Plan for #28: agent skill and `hali skill install`

Approved by standing instruction on 2026-11-28 (unattended run for #27-#32), with all planner defaults. The contract (decisions 1-9, behaviour, criteria, out of scope, security) is the "Groomed contract" section of issue #28. **Stacked on PR #223 (issue #27, branch 27-agent-usage-guide)**: `docs/agent-usage.md`, `hali help agents`, `core::guide` and the `--help` examples already exist on this branch.

## Where things stand

`skill/` does not exist. `hali skill install` is exit 2 (`unrecognized subcommand 'skill'`). `hali describe --brief --json` prints `{"schema_version":1,"core_version","store","counts"}` with no `skill` key. Crate version is `0.0.0`. `scripts/install.sh` already runs `hali skill install` once `hali skill --help` succeeds. Existing tests that must change: `tests/agent_usage.rs` (asserts the guide does NOT mention `skill install`); `tests/schema_contract.rs` (expects a `skill` key in `describe` to be invalid); `tests/example_world/mod.rs` (panics on an unclassified command: add `skill install` to `MUTATING`); `tests/dry_run_guard.rs` (needs a `CASES` row); `tests/cli_schema.rs` NAMES and `cli_schema__schema_list_json.snap`; the three `tests/snapshots/cli_describe__*.snap`.

`scripts/release-prepare.sh` refuses a bump that changes anything except `Cargo.toml` and `Cargo.lock`, so `metadata.hali-version` is a placeholder (`hali-version: "{{HALI_VERSION}}"`) replaced by `env!("CARGO_PKG_VERSION")` at render time.

## Frontmatter and body (what the tests parse, from `hali skill install --print`)

Starts with `---`; first fields `name: hali` and `description:` (at most 1024 chars, third person, starting "Find, open, tag or catalog local files and documents;" and containing `ontology`, `concepts`, `types`, `related files` and `hali`); then `metadata:` with `  hali-version: "<version>"`; then `allowed-tools:` on one line, comma-separated, exactly: `Bash(hali describe *), Bash(hali find *), Bash(hali show *), Bash(hali query *), Bash(hali schema *), Bash(hali validate *), Bash(hali help *), Bash(hali --dry-run *)`. No rule names a writing command (`skill` included); `--dry-run` appears only as the `hali --dry-run *` prefix. Body under 200 lines after the closing `---`, covering the eight §11.2 points (words: `HALI_AGENT`, `hali describe --json`, `hali find`, `hali show`, `path`, `hali --dry-run`, `changes`, user approval, `UNKNOWN_CONCEPT`, `concept add`, `suggested_command`, `absolute`, `hali query`), linking `references/usage.md`; every `hali …` command line parses with the clap tree; `--dry-run` only straight after `hali`; `--yes` only in a "never on your own / only with the user's consent" sentence.

## Seams

- `hali skill install --print` stdout (the rendered `SKILL.md`): the highest seam for content.
- `hali skill install [--agents] [--dry-run] [--json]` with a temp `HOME` (env cleared): files, modes, actions, errors, nothing written outside the two directories.
- `hali describe [--brief] --json` with a temp `HOME`: the `skill` key.
- `hali schema skill install|describe|error`.
- `hali::core::skill` (new, pure): `skill_md()`, `files()`, `installed_version(text)`, `state(installed_text: Option<&str>)`, `action(current: Option<&[u8]>, new: &[u8])`.
- `hali::paths::skill_dirs(env)` (new): the two directories from `HOME`.
- `hali::storage::skill` (new): the fd-based check, read and write.
- The clap tree (`hali::cli::command()`): examples, classification, the `dry_run_guard` leaf list.

## Goal map

All goal-tagged (`#[cfg(feature = "goal")]` on its own line). Tests use `CARGO_BIN_EXE_hali` with `env_clear` and a temp `HOME`/XDG/`HALI_DATA_DIR`; **never the real `~/.claude` or `~/.agents`**. Snapshot tests redact the binary version at JSON level (assert `skill.binary` and `version` equal `env!("CARGO_PKG_VERSION")`, then replace them with `"<VERSION>"` before snapshotting). Expected failures today: (a) E0432 unresolved import `hali::core::skill` / `hali::paths::skill_dirs`; (b) binary exit 2 `unrecognized subcommand 'skill'`; (c) the `describe` snapshot has no `skill` key.

| # | Criterion | Check | Expected failure |
|---|---|---|---|
| 1 | `core::skill`: rendered `SKILL.md` has the binary version; `installed_version` reads it back; `state` gives (null, v, false) for no file, (v, v, false) current, ("9.9.9", v, true) other version, (null, v, true) no readable version; `action` gives created/updated/unchanged | `tests/core_skill.rs::{render_has_binary_version, installed_version_parse, state_cases, action_cases}` | (a) |
| 2 | Frontmatter as above | `tests/skill_doc.rs::frontmatter` | (b) |
| 3 | `allowed-tools` is exactly the eight rules; every `--dry-run` rule is `Bash(hali --dry-run *)`; every other rule names a READ_ONLY command or `help` | `skill_doc::allowed_tools_read_only_prefix_only` | (b) |
| 4 | `hali concept add -- --dry-run` is a real run (adds a Concept labelled `--dry-run`, no `dry_run` key) | `tests/cli_skill.rs::suffix_dry_run_is_a_real_run` | none (green today; a lasting guard for the prefix-only rationale) |
| 5 | Body rules above | `skill_doc::body` | (b) |
| 6 | Empty temp HOME: `skill install --json` exits 0; `~/.claude/skills/hali/SKILL.md` equals `--print`; `references/usage.md` is byte-identical to `docs/agent-usage.md`; new dirs 0700, files 0600; `~/.agents` absent | `cli_skill::install_claude_location` | (b) |
| 7 | The JSON of row 6 matches a hand-written golden (`/TMP`, `<VERSION>` redacted) and validates against `hali schema skill install` | `cli_skill::install_json` + locked `tests/snapshots/cli_skill__install_json.snap`; `tests/schema_contract.rs` | (b) then (c) |
| 8 | `--agents` also writes `~/.agents/skills/hali/{SKILL.md,references/usage.md}`; both appear in `files`, in order | `cli_skill::agents_location` | (b) |
| 9 | `--print` prints `SKILL.md` exactly, writes nothing, ignores `--json`; `--print --agents` exits 2 | `cli_skill::{print_writes_nothing, print_conflicts_with_agents}` | (b) |
| 10 | Rerun is `unchanged` and keeps mtimes; a modified file is `updated`; a user's extra file in the directory is untouched | `cli_skill::{rerun_unchanged, modified_updated, foreign_file_untouched}` | (b) |
| 11 | `--dry-run`: same payload plus `dry_run:true, changes:[]`, creates nothing; text `would create skill file …` plus the stderr note; `hali --dry-run skill install --agents` leaves the temp HOME byte-identical | `cli_skill::dry_run_json` (locked `.snap`), `dry_run_text`; a `("skill install", "store", &["skill","install","--agents"])` row in `tests/dry_run_guard.rs` | (b); the guard fails "no CASES entry" once the command exists |
| 12 | `SKILL_DIR_UNUSABLE` (exit 3) for each reason: `unresolved` (no HOME; relative HOME), `not_a_directory` (`~/.claude` is a file), `symlink` (`hali/` a symlink; `SKILL.md` a symlink whose target stays unchanged), `not_a_file` (a directory at `SKILL.md`), `unsafe` (`hali/` is g+w), `cannot_create` (`skills/` is 0500; skipped as root); the same errors under `--dry-run`, nothing written; the envelope validates against `hali schema error` | `cli_skill::error_<reason>` ×6, `error_json_envelope` (locked `.snap`) | (b) |
| 13 | `paths::skill_dirs` returns `$HOME/.claude/skills/hali` and `$HOME/.agents/skills/hali`, `Unresolved` for unset, empty or relative HOME | `tests/paths_skill.rs::skill_dirs` | (a) |
| 14 | A control character in `HALI_AGENT` makes `skill install` exit 2; a broken `config.toml` does not stop it | `cli_skill::{agent_checked, config_not_read}` | (b) |
| 15 | `describe` (full and `--brief`) ends with `skill` in four situations: not installed, current, another version (stale), no HOME; text gains `skill_installed`, `skill_binary`, `skill_stale` after `relations` | `cli_skill::describe_skill_{absent,current,stale,unresolved}`, `describe_skill_text`; hand-edit and lock the three `tests/snapshots/cli_describe__*.snap` | (c) |
| 16 | Schemas: `hali schema skill install` exists; `describe.json` requires `skill` (both forms); `error.json` has a `SKILL_DIR_UNUSABLE` branch; the schema list includes `skill install` (28 → 29); the `schema_contract` case "extra key in full: skill" becomes valid | `cli_skill::schema_skill_install`; edits to `tests/schema_contract.rs`, `tests/cli_schema.rs`, `cli_schema__schema_list_json.snap` | `hali schema skill install`: unknown schema, exit 2 |
| 17 | The `skill` group and `skill install` have valid Examples blocks that pass in the example world (a `--json` and a `hali --dry-run` example); `hali skill --help` exits 0 (install.sh probes it) | `tests/cli_help_examples.rs` after `"skill install"` is added to `MUTATING`; `cli_skill::skill_help_ok` | (b); the classification walk panics once the command exists without examples |
| 18 | The guide may mention `hali skill install`: drop the ban in `tests/agent_usage.rs`; require the mention | edit `tests/agent_usage.rs` | `assert text.contains("hali skill install")` |
| 19 | Spec: §9.1 **Skill:** paragraph, `describe`'s `skill` key and `SKILL_DIR_UNUSABLE` in §9.4; "arrives with #28" gone | `.goal/checks/spec_skill.sh` greps `docs/spec.md` for `SKILL_DIR_UNUSABLE`, `"skill":{"installed"` and `**Skill:**` | grep exit 1 |
| 20 | (BLOCKING, review round 1) Every directory the install writes into (`~/.claude`, `~/.agents` with `--agents`, `skills`, `hali`, `references`) must be owned by the euid and not group- or world-writable; HOME must be owned by the euid but its mode is NOT checked. A violation is `SKILL_DIR_UNUSABLE` exit 3 `unsafe`, `details.path` the directory, found BEFORE anything is written and under `--dry-run`. `~/.claude/skills` 0777, `~/.claude` 0775, `~/.agents/skills` 0777 with `--agents` (nothing written in the claude location either), `HOME=/` (non-root, dry run: path `/`); a user-owned 0755 dir and a 0777 HOME are accepted (guard) | `cli_skill::{error_unsafe_every_written_directory, error_unsafe_agents_location_writes_nothing_anywhere, error_unsafe_home_not_owned, unsafe_check_accepts_user_owned_0755}` | exit 0 where 3 `unsafe` is expected; the 0755 guard passes today |
| 21 | An existing file with identical bytes but `mode & 0o022` set (or not owned by the euid) is `updated`, ends 0600; a rerun is `unchanged`. Ownership is not tested (needs root) | `cli_skill::right_bytes_wrong_mode_is_updated` | both files `unchanged`, expected `updated` |
| 22 | `~/.agents` a symlink to `~/.claude` (and `~/.agents/skills` a symlink to `~/.claude/skills`): `skill install --agents --json` exits 0, the second location reports `unchanged` | `cli_skill::{agents_symlinked_to_claude, agents_skills_symlinked_to_claude_skills}` | exit 3 `cannot_create` (EEXIST) |
| 23 | A `SKILL_DIR_UNUSABLE` found in the check phase (`~/.claude` 0000, dry run and real, non-root) ends "; nothing was written", even `cannot_create`. After a write began (`~/.agents` 0500 with `--agents`: claude files written first) the message does not say "nothing was written" and the claude files exist | `cli_skill::{cannot_create_found_before_writing_says_nothing_was_written, cannot_create_after_a_write_began_does_not_say_nothing_was_written}` | message `... cannot be created or written` lacks "nothing was written"; the second passes today (guard) |
| 24 | Rendered `SKILL.md`: no read-only command line starts with an env-var assignment; the body shows `--agent <name>` after the subcommand of write commands (a `hali --dry-run … --agent` and a `hali <write> … --agent` example); a Concept is never a "tag" except in a flag name; never "only its own store" | `skill_doc::body_agent_naming_and_glossary` | body line 15 `HALI_AGENT=claude hali describe --json` |
| 25 | `describe` with a symlinked or FIFO `SKILL.md`, or a symlinked `hali/` directory holding a valid SKILL.md: `installed` null AND `stale` true | `cli_skill::{describe_skill_is_not_read_through_a_symlink, describe_skill_fifo_does_not_block, describe_skill_is_not_read_through_a_symlinked_hali_dir}` | none today (the stronger assertions already hold); lasting guards |
| 26 | `hali --dry-run skill install --agents` over a pre-existing differing `SKILL.md` says "would update" (JSON `updated`) and leaves the whole HOME identical (a separate test, since `CASES` rows have no setup) | `tests/dry_run_guard.rs::dry_run_skill_install_over_a_differing_file_writes_nothing` | none today; lasting guard |

Rows 20-26 are review round 1 (decisions 20-26 of the human's reply; the numbering of rows 27-28 is unused). Tests needing non-root permissions print a skip reason and return when running as root.

Lock every goal-owned path outside `.goal/` in `.goal/locked` (the four new test files, the locked `.snap` files, the edited `tests/schema_contract.rs`/`dry_run_guard.rs`/`agent_usage.rs`/`cli_schema.rs`/`example_world/mod.rs` and the three describe snapshots).

## Approach (implementer, opus; cross-cutting and security-sensitive)

1. `skill/SKILL.md` with the placeholder.
2. `core::skill` (TDD, row 1), `Error::skill_dir_unusable` in `core/mod.rs`.
3. `paths::skill_dirs`.
4. `storage/skill.rs`: `check(dir)` with `lstat` (dry runs), then `install` through `libc::open`/`openat` with `O_DIRECTORY|O_NOFOLLOW`, `fstat` uid/mode (reuse `storage::store_file::dir_problem`), `mkdirat` 0700, temp file `O_CREAT|O_EXCL|O_NOFOLLOW|O_CLOEXEC`, `fchmod` 0600, `fsync`, `renameat`, `unlinkat` on failure; plus a bounded, non-blocking `read_installed(path)` for `describe`.
5. `cli`: `Skill(SkillCommand::Install{agents, print})` with `conflicts_with`; dispatch after the agent check and before the config is read; `--print` prints and returns before `HOME` is resolved; examples in `src/cli/examples.rs`; `skill` as the last key of `describe`.
6. Schemas and lists; spec; one line in the guide.

No new dependency (`libc` is already pinned). File I/O only in `storage`, `HOME` only in `paths`.

## Risks

Hand-rolled `openat`/`renameat` FFI (keep small, confined to `storage`); a partial write across four files (the error names the path); the three settled describe snapshots change; Claude Code's real `allowed-tools` matching cannot be verified in CI; rebase if #223 changes before it merges.

## Security (required security-reviewer)

Path traversal (only fixed names joined to an absolute `HOME`); symlinks (`hali/`, `references/` opened `O_NOFOLLOW|O_DIRECTORY`; files never opened for writing by name; a symlinked `SKILL.md`/`usage.md` refused); TOCTOU (the `lstat` pre-check is advisory, the guarantee is the fd-based ownership check and no path-based write after it); root or sudo with another user's `HOME`; modes and umask (dirs 0700, files 0600 by explicit `fchmod`); a hostile existing `~/.claude` (FIFOs and devices opened `O_NONBLOCK`, bounded reads); content is embedded at build time and must never advise `--yes` or a real write without approval; `allowed-tools` only the eight rules; `hali --dry-run skill install` must write nothing (otherwise the prefix rule would let an agent rewrite its own permission file); no network, no subprocess. Accepting the `allowed-tools` matcher risk stays with a qualified human.
