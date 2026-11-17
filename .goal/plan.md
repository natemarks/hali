# Plan for #13 Agent identity (approved)

The approved contract is the "Groomed contract (approved)" section of issue #13. All seven open questions took the planner's default (no attribution for other commands; no orphan Agents; exact match after trim; `--agent ""` is a usage error; invalid `HALI_AGENT` fails every command but `schema`; no special case for `hali`; `show` schema unchanged).

Modestly cross-cutting (shared `catalog_quads` for add and scan): implementer on opus. Input handling is sensitive: security-reviewer as well as reviewer.

## Seams
- `core::config::agent_name(flag: Option<&str>, env: Option<&OsStr>) -> Result<Option<String>, Error>`; `Context::with_agent(Option<String>)`; `resource::add_all` and `scan::scan_with` read the agent from `ctx`; a `find_or_mint_agent` helper beside the Person handling in `core::resource::catalog_quads`; `prov::{SOFTWARE_AGENT, ACTED_ON_BEHALF_OF}` and `foaf::NAME` constants in `src/ontology/mod.rs`.
- CLI: global `--agent`, `HALI_AGENT` via `var_os` in `execute`.

## Goal map
| # | Criterion | Check |
|---|---|---|
| 1 | `--agent claude` attributes the new record to a SoftwareAgent named `claude` that acts on behalf of the Person | `core_agent::add_attributes_to_agent`, `cli_agent::{add_agent_dry_run_json, show_agent_json}` (locked goldens) |
| 2 | `HALI_AGENT` used; `--agent` wins; empty or whitespace env is unset | `cli_agent::{env_used, flag_overrides_env}`, `core_agent::agent_name_rules` |
| 3 | Neither set: the record goes to the Person and no SoftwareAgent exists | `cli_agent::no_agent_attributes_user` (add and scan; regression pin, passes today) |
| 4 | Same name (even with spaces around it) reuses the Agent, mints no new id | `core_agent::same_name_reuses`, `cli_agent::reuse_across_runs` |
| 5 | A different name mints a second Agent; first-run agent also mints the Person, in id order | `core_agent::new_name_mints_with_person` |
| 6 | `scan --agent` attributes newly cataloged Resources | `cli_agent::scan_attributes_agent` |
| 7 | Dry-run `changes` equal the real write; nothing written | `cli_agent::dry_run_matches_real` |
| 8 | `edit`, `refresh`, `relink`, `forget` with `--agent` leave attribution and mint nothing; read-only output identical with or without it | `cli_agent::other_commands_ignore_agent` |
| 9 | Invalid `--agent` or `HALI_AGENT` (empty, control character, non-UTF-8) is exit 2 with the store untouched; invalid env fails read-only commands too, except `schema` | `cli_agent::invalid_names_usage` |
| 10 | Turtle and SPARQL metacharacters in a name are one inert literal; reload and re-serialize byte-identical; no extra triples | `core_agent::name_is_inert_literal` |
| 11 | Spec §9.6 updated; `hali --help` lists `--agent` | `.goal/checks/spec_agent.sh` |
| 12 | §14.1 coverage | rows 1, 2, 9 |

Guards: existing goldens unchanged (no `--agent` means byte-identical output); `hali schema` output unchanged.

Tests set HOME, XDG_STATE_HOME and the data dir to temp dirs and clear `HALI_AGENT` unless a test sets it.
