# AGENTS.md

Guidance for AI agents (and humans) working **on** hali. For using hali as a tool, see `hali help agents`. Its single source is `docs/agent-usage.md`.

## What this is

hali is a Rust CLI that keeps a standards-based RDF ontology (SKOS, OWL 2, SHACL) describing local files in place, plus a read-only Neovim/Telescope plugin in Lua. **Status: pre-implementation.** Read [`docs/spec.md`](docs/spec.md) before changing anything; it is the source of truth for behaviour.

Read on demand (these are not imported here):
- [`docs/spec.md`](docs/spec.md): the v1 specification
- [`GLOSSARY.md`](GLOSSARY.md): domain terms (Resource, Type, Concept, …). Use them exactly in code, docs and output.
- [`docs/adr/`](docs/adr/): decisions that are hard to reverse, and why

## Commands

These targets are specified in `docs/spec.md` §13.2 and don't exist until the scaffold lands.

| Command | Use |
|---|---|
| `make static` | All checks with auto-format. Run before considering a change done. |
| `make static-check` | What CI runs (check-only). |
| `make unit` | `cargo test` |
| `make unit-update-golden` | Accept changed `insta` snapshots, after reviewing them. |
| `make lua-test` | Plugin tests (headless Neovim + plenary). |
| `make install` | Install the dev build into `~/.local/bin`. |

## Rules

- **TDD at the `core` seam.** Write the failing test first, confirm it fails, implement, confirm it passes, then wire the operation into `cli` as a thin passthrough.
- **Every command needs** a `core` unit test, a CLI integration test (`assert_cmd`) and an `insta` golden JSON snapshot. **Every error rule needs** a test that triggers it.
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
