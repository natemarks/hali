# Plan: #17 Managing Concepts

Contract: see issue #17 ("Contract (groomed)"). Approved with all planner recommendations (open questions 1-12).

## Goal map

New goal-tagged files `tests/core_concept_manage.rs` and `tests/cli_concept_manage.rs`, plus `mod concept_manage` in `tests/cli_dry_run.rs`. Fixture `tests/fixtures/concepts/{vocabulary,resources,extensions}.ttl`: animal; mammal (broader animal); dog ("dog"@en, "chien"@fr, alt "hound"@en and "toutou"@fr, broader mammal and pet); pet; cat (alt "kitty", "chat"@fr, broader mammal); fish; plant; bird; `hali:pet skos:related hali:cat`. resources: R1 tagged dog and fish. extensions: parrot (a Concept, broader bird); `R1 dcterms:subject hali:plant`. The fixture and every hand-written `.snap` are listed in `.goal/locked`. Do not edit `tests/fixtures/store`.

| # | Criterion | Checks |
|---|---|---|
| 1 | Several broader parents; narrower derived, never stored | core: `edit_add_broader_multiple_parents`, `edit_remove_broader`, `edit_never_stores_narrower`, `show_narrower_is_derived_including_extensions` (bird→[parrot], mammal→[cat,dog]), `edit_broader_cycle_is_allowed`, `edit_self_broader_is_usage`, `edit_broader_in_both_is_usage`, `edit_remove_broader_absent_is_noop` |
| 2 | `concept show` | core: `show_lists_labels_broader_narrower_and_tagged` (dog: alt [hound], broader [mammal,pet], narrower [], tagged 1), `show_uses_configured_language`, `show_resolves_by_label_and_alt`, `show_unknown_is_unknown_concept`; cli: `concept_show_json` (locked golden), `show_ignores_dry_run_and_writes_nothing`, `text_summaries` |
| 3 | rename keeps old prefLabel as alt, IRI unchanged | core: `rename_keeps_old_pref_label_as_alt` (exact changes), `rename_keeps_iri`, `rename_to_own_alt_swaps`, `rename_case_only_replaces_without_alt`, `rename_same_label_is_noop`, `rename_leaves_other_languages`, `rename_unknown_is_unknown_concept`; cli: `concept_rename_json` (locked) |
| 4 | Labels unique per language (`LABEL_TAKEN`) | core: `rename_to_taken_label_is_label_taken`, `edit_add_alt_taken_is_label_taken`, `edit_add_alt_same_label_other_language_ok`, `edit_add_alt_own_label_is_noop`; cli: `rename_label_taken_writes_nothing` |
| 5 | edit alt flags and ordering | core: `edit_add_and_remove_alt`, `edit_remove_absent_alt_is_noop`, `edit_remove_alt_never_touches_pref_label`, `edit_alt_in_both_is_usage`, `edit_no_change_is_usage`, `edit_unknown_order`; cli: `concept_edit_json` (locked), `edit_without_flags_exits_2` |
| 6 | `IN_USE` triggered by a test | core: `remove_tagged_is_in_use` (fish: tagged 1, narrower 0), `remove_tagged_in_extensions_is_in_use` (plant), `remove_broader_parent_is_in_use` (mammal: narrower 2; bird: 1 via extensions), `remove_in_use_before_shacl`, `in_use_error`; cli: `in_use_json_envelope` (locked stderr golden), `remove_in_use_before_confirmation` (TTY=1: no prompt, vocabulary bytes unchanged) |
| 7 | remove removes the right triples and nothing else | core: `remove_takes_concept_triples_from_vocabulary_only` (cat: every language, outgoing broader, inbound `pet skos:related cat`; scheme, other Concepts and extensions kept), `remove_last_concept_keeps_scheme`, `remove_changes_are_sorted_removes`, `remove_unknown_is_unknown_concept`; cli: `only_vocabulary_changes` (resources.ttl and extensions.ttl bytes unchanged after rename, edit and remove) |
| 8 | remove uses the confirmation mechanism | cli: `concept_remove_json` (locked, with `--yes`), `remove_confirmation_required_json_envelope` (locked stderr, `command: "concept remove"`, `declined: false`), `remove_no_tty_requires_confirmation` (retry `hali concept remove cat --yes`; vocabulary bytes unchanged), `remove_tty_prompt_yes_no_eof` (exact prompt `remove concept cat "cat"? [y/N] `; n/EOF → `declined: true`, unchanged; y → removed), `remove_dry_run_never_asks`, `remove_never_waits_on_stdin` (held-open pipe, 10 s deadline), `rename_and_edit_never_ask` (TTY=0, no `--yes`, exit 0) |
| 9 | Dry run on every new mutating leaf | `cli_dry_run::concept_manage::{concept_rename_dry_run_matches, concept_edit_dry_run_matches, concept_remove_dry_run_matches}` (remove with `--yes`), locked goldens `cli_dry_run__concept_{rename,edit,remove}_dry_run_json.snap`; `.goal/checks/01-dry_run_guard_concepts.sh`: CASES rows `("concept edit", &["concept","edit","ml","--add-alt","guard probe"])`, `("concept remove", &["concept","remove","a-first"])`, `("concept rename", &["concept","rename","ml","guard probe"])`, `("concept show", &["concept","show","ml"])`, none passing `--yes` |
| 10 | Spec §14.1 | `.goal/checks/rules_triggered.sh` (the #14 script): `IN_USE` constructor in `src/core/mod.rs`, triggered in both core and CLI manage tests |
| 11 | Schemas | `tests/schema_contract.rs` `GOAL_SNAPS_17` / `GOAL_STDOUT_GOLDENS_17` (count floor raised), `concept_manage_schemas_accept_and_reject` (real and dry; reject a missing or extra key, `schema_version` 2, `dry_run` false, non-integer `tagged`), `in_use_branch`; cli: `schema_lists_new_names` |

Dry-run goldens must be named `cli_dry_run__concept_<verb>_dry_run_json` so `dry_run_command()` maps them.

## Approach

1. `src/core/mod.rs`: `Error::in_use(concept, tagged, narrower)`.
2. `src/core/concept.rs`: `show` (returns a `Shown` with `narrower` and `tagged`, helpers read every graph via `quads_for_pattern` with graph `None`), `rename`, `edit` (taking a `ConceptEdit {add_alt, remove_alt, add_broader, remove_broader}`) and `remove`; each builds added/removed quads on the vocabulary graph, then one `validate::guard`, then one `apply`; `remove` checks `IN_USE` before the guard. Reuse `fold`.
3. `src/cli/mod.rs`: four `ConceptCommand` variants; clap ArgGroup over the four `edit` flags; labels via `parse_label`; `mutated()` takes a `storage::File` (existing callers pass `Resources`, concept commands pass `Vocabulary`); a `Confirmation { command: "concept remove", .. }` for remove only; `show` text via `printable`.
4. Schemas: four JSON files, `SCHEMAS` registration (10 → 14, sorted), `IN_USE` branch in `error.json`.
5. Spec edits in the goal commit: §9.1 synopsis (`…`) and a Concepts paragraph, §9.5 read-only list, §9.8 prompt for `concept remove`.
6. The implementer updates unlocked files: `tests/cli_schema.rs`, `tests/core_schema.rs`, `cli_schema__schema_list_json.snap`, `tests/dry_run_guard.rs` rows.

Not cross-cutting: implementer on sonnet, raised to opus after a round without progress.

## Security call

No listed security-sensitive path is touched; `storage::save(File::Vocabulary)` already exists. Security-reviewer recommended as well as the reviewer: `concept remove` is destructive and reads a confirmation from stdin. Risks and covering checks: over-removal (the remove-only-vocabulary, last-concept and only-vocabulary-changes tests); dangling `skos:broader` (IN_USE on narrower, every graph); tagged Resources only in extensions (every-graph read); saving before confirmation or on decline (byte-equality tests); dry run asking or writing (guard rows and `remove_dry_run_never_asks`); stdin hang (`remove_never_waits_on_stdin`); terminal escapes in the prompt (`printable`; #73 covers `LABEL_TAKEN`/`IN_USE` text that echoes ids). `suggested_command` hands agents `--yes` (spec-mandated, as for forget).

## Wording the goal fixes (contract left it open; spec §9.1 Concepts paragraph states it)

- `concept show` text joins list values with `, ` (empty list: nothing after the tab).
- `IN_USE` suggested_fix: `Untag the Resources (hali edit <ref> --remove-tag <id>) and unlink the narrower Concepts (hali concept edit <C> --remove-broader <id>), then retry.`; message keeps "Concepts" at any count.
- `CONFIRMATION_REQUIRED` for remove: `details.command` and the message name `concept remove`.
- Core API (see headers of `tests/core_concept_manage.rs`): `concept::{show, rename, edit, remove}` take Concept references; `concept::Shown`, `concept::ConceptEdit` (`Clone, Debug, Default`), `Error::in_use(&str, usize, usize)`.
- The CLI checks edit's flags-in-both-lists usage error before loading the store (`execute` loads it first today).

## Risks

Label-uniqueness `sh:sparql` cost (#48) per mutation, grows with each rename's extra label. rudof `sh:uniqueLang`: rename must remove the old prefLabel in the same guard call; a prefLabel only in `extensions.ttl` gives `SHACL_VIOLATION` (acceptable). The schema-list snapshot and long dry-run goldens are hand-derived from canonical N-Triples.
