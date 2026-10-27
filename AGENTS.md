# AGENTS.md

Guidance for AI agents (and humans) working **on** hali. For using hali as a tool, see `hali help agents`. Its single source is `docs/agent-usage.md`.

## What this is

hali is a Rust CLI that keeps a standards-based RDF ontology (SKOS, OWL 2, SHACL) describing local files in place, plus a read-only Neovim/Telescope plugin in Lua. **Status: pre-implementation.** Read [`docs/spec.md`](docs/spec.md) before changing anything; it is the source of truth for behaviour.

Read on demand (these are not imported here):
- [`docs/spec.md`](docs/spec.md): the v1 specification
- [`GLOSSARY.md`](GLOSSARY.md): domain terms (Resource, Type, Concept, …). Use them exactly in code, docs and output.
- [`docs/adr/`](docs/adr/): decisions that are hard to reverse, and why

## Commands

Specified in `docs/spec.md` §13.2. `make help` lists what exists so far; `make bench`, `make install`, `make lua-test` and the release targets arrive with their tickets. Pre-commit runs betterleaks (a gitleaks-compatible secret scanner) and `make static` (`pre-commit install` once per clone).

| Command | Use |
|---|---|
| `make static` | All checks with auto-format. Run before considering a change done. |
| `make static-check` | What CI runs (check-only). |
| `make unit` | `cargo test --features test-hooks`: every test that needs no outside software. |
| `make unit-update-golden` | Accept changed `insta` snapshots, after reviewing them. |
| `make test-external` | `cargo test --features test-hooks -- --ignored`: the tests that need outside software (the MIME database). CI runs it in its own job. |
| `make lua-test` | Plugin tests (headless Neovim + plenary). |
| `make install` | Install the dev build into `~/.local/bin`. |

## Rules

- **TDD at the `core` seam.** Write the failing test first, confirm it fails, implement, confirm it passes, then wire the operation into `cli` as a thin passthrough.
- **Every command needs** a `core` unit test, a CLI integration test (`assert_cmd`) and an `insta` golden JSON snapshot. **Every error rule needs** a test that triggers it.
- **Tests that need outside software** (such as the shared-mime-info database) are `#[ignore = "<reason>"]`d, so `make unit` stays self-contained; `make test-external` runs them.
- **`core` is transport-agnostic.** It never touches clap, stdout or the process environment. `cli` holds no business logic. Clock and ID generation are injected (`Clock`, `IdGen`).
- **Stay behind the seams.** OS- and storage-specific code lives only in `storage`, `media_type` and `paths`.
- **Pin dependencies exactly** (`=`) in `Cargo.toml`.
- **Never write into cataloged files**, and never move, rename or delete them. hali writes only to its own data, state and cache dirs, plus new files from `hali new`.
- **Tool-managed Turtle is deterministic:** sorted, no blank nodes, fixed prefixes. A load + re-serialize must be byte-identical.
- **The JSON output is a contract** for Neovim and agents. Changing its shape means bumping `schema_version`, updating `hali schema`, and updating the snapshots that the plugin tests consume.
- **Don't add abstractions,** error handling or validation beyond what the spec needs.

## Layout

See `docs/spec.md` §12. In short: `src/{cli,core,ontology,storage,media_type,paths}`, cargo tests in `tests/`, the Lua plugin at the repo root (`plugin/`, `lua/`, `doc/`, `lazy.lua`), plugin tests in `spec/`, and the agent skill in `skill/`.

## Git

Feature branches off `main` and reviewed PRs; no direct pushes to `main`. Releases go through `make release-prepare` / `make release-publish` (spec §13.3).

## Agent skills

### Issue tracker

Issues live in GitHub Issues on `natemarks/hali`, via the `gh` CLI, with native blocked-by dependencies. See `docs/agents/issue-tracker.md`.

### Triage labels

The five default labels: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: a root `GLOSSARY.md` and `docs/adr/`. See `docs/agents/domain.md`.

## AI loop

Each GitHub issue is a contract for one change. The human owns its goal, priority, acceptance criteria and acceptable risk, and merges the PR. Agents draft, implement and review.

1. `/loop-status` (or `make loop-status`): what needs the human, with the next command for each item.
2. `/groom-issue <issue>`: one approval of the contract and goal map, then a draft PR in `.claude/worktrees/<branch>` whose goal is red.
3. `/drive-pr [pr] [rounds]`: agents loop until `make goal` is green, review, settle, and mark the PR ready. Progress shows in the PR's status block.
4. The human reviews the diff and CI, merges, and turns follow-ups into issues.

Each PR works in its own worktree under `.claude/worktrees/`; skills switch into it with `EnterWorktree`.

### Goal

The **goal** is the PR's executable definition of done.

- `make goal`: `static-check`, goal-guard, goal-tagged tests, and `.goal/checks/*.sh`. The last line reads `GOAL GREEN` or `GOAL RED: <gates>`.
- `make goal-tests`: goal-tagged tests only (`cargo test --features test-hooks,goal`, so goal tests can use `HALI_TEST_NOW` / `HALI_TEST_ID_SEED`). Never use `--all-features` in `static`, `static-check`, clippy or `unit`: it would compile the red goal tests and break the commit hook.
- `make goal-settle`: untag the goal's tests so they join `make unit`, delete `.goal/`, commit `goal: settle #<issue>`.
- `make goal-settled`: fails while goal scaffolding remains. A required CI check on ready PRs.
- `make goal-selftest` and `make loop-doctor`: prove the loop works, and check what it depends on.
- **Goal tag**: `#[cfg(feature = "goal")]` on a line of its own above the test or test module (cargo feature `goal`). It keeps a red test out of `make unit`, so the commit hook stays green.
- **BDD**: none. Spec §14.1 already requires a `core` unit test, an `assert_cmd` CLI test and an `insta` golden snapshot per command, and acceptance criteria map onto those, so Gherkin would duplicate them. The goal map ties each criterion to tests and checks only.
- **Golden snapshots**: a goal-tagged `insta` test ships with a hand-written `.snap` whose expected JSON comes from `docs/spec.md`, and that `.snap` is listed in `.goal/locked`. The red is then a real mismatch, and `make unit-update-golden` can't bless the implementer's own output.

`.goal/` exists only on a PR branch and is deleted at settle:

| Path | Holds |
|---|---|
| `ISSUE` | the issue number |
| `plan.md` | the approved plan and goal map (criterion → checks) |
| `locked` | goal-owned paths outside `.goal/`, one per line |
| `checks/*.sh` | temporary completeness checks |
| `last-run.log` | latest `make goal` output (untracked) |

Goal-owned files (`.goal/` and every path in `locked`) change only in `goal: ...` commits; goal-guard enforces it.

### PR status block

`/groom-issue` and `/drive-pr` keep this block in the PR body through `scripts/goal-pr-status.sh <pr>`; `make loop-status` reads its first line.

```markdown
<!-- goal-state: red | green | stopped:<budget|no-progress|scope|guard> | ready -->
**Goal: RED** · round 2 · updated 2026-10-24

| # | Acceptance criterion | Checks | Status |
|---|---|---|---|
| 1 | Given …, when …, then … | `tests/some_test.rs::some_test` | ❌ |

**Stopped:** <reason>. <the decision needed>. Resume: `<command>`

<details><summary>Round log</summary>

| Round | Red before | Agent (model) | Commits | Note |
|---|---|---|---|---|

</details>
```

The **Stopped** line appears only in a `stopped:` state.

### Agents

| Agent | Model | Tools | Role |
|---|---|---|---|
| planner | opus | read-only | contract gaps, goal map, approach, risks |
| test-author | sonnet | edit | writes and corrects the goal |
| implementer | sonnet | edit | makes the goal green |
| reviewer | sonnet | read-only | bugs, acceptance, missing cases |
| security-reviewer | opus | read-only | threat model and scanners |

`/drive-pr` raises the implementer and reviewer to opus for cross-cutting changes and after a round without progress. On Bedrock, the aliases resolve through `ANTHROPIC_DEFAULT_{OPUS,SONNET,HAIKU}_MODEL`; `make loop-doctor` checks they are pinned.

The agent and skill files say `CLAUDE.md`; it reaches this file through its `@AGENTS.md` import.

### Security-sensitive paths

- `src/storage/`, `src/paths/`, `src/media_type/`
- the `hali query` SPARQL path (read-only enforcement, `READ_ONLY_QUERY`)
- `hali skill install`, which writes into `~/.claude`
- `scripts/install.sh`
- `Cargo.toml`, `Cargo.lock`
- `.github/workflows/`

A diff touching these, or an issue labelled `security`, gets the security-reviewer as well as the reviewer. An LLM review is one source of findings: security exceptions and consequential security calls stay with a qualified human.
