# Plan: #16 Tagging Resources with Concepts

Contract: see issue #16 ("Contract (groomed)"). Approved with all planner recommendations (open questions 1–10).

## Goal map

| # | Criterion | Checks |
|---|---|---|
| 1 | Tags stored as `dcterms:subject`; shape requires `hali:topics` | `tests/core_tag.rs::{add_with_tags_writes_dcterms_subject, edit_adds_and_removes_tags, edit_tag_noops_have_no_changes, edit_requires_some_change}`; `tests/cli_tag.rs::{add_tag_json, edit_add_remove_tag_json}` (locked goldens); `tests/core_shacl_resource.rs::subject_must_be_topics_concept` (fixtures `resource-subject-{conforming,not-concept,other-scheme}`); `tests/cli_dry_run.rs::{add_tag_dry_run_matches, edit_tags_dry_run_matches, concept_add_broader_dry_run_matches}` |
| 2 | `UNKNOWN_CONCEPT` with closest Concepts and suggested command | `tests/core_tag.rs::{unknown_concept_suggests_closest, unknown_concept_suggested_command_is_quoted, ref_slug_wins_over_label, ref_label_case_and_whitespace_insensitive, ref_other_language_label_not_matched}`; `tests/cli_tag.rs::unknown_concept_from_add_edit_find_concept_add` (locked envelope golden) |
| 3 | `concept add --broader`; broader-path SPARQL over a fixture graph | `tests/core_concept.rs::add_with_broader`; `tests/cli_concept.rs::concept_add_broader_json`; `tests/core_find.rs::tag_matches_narrower` over `tests/fixtures/tags/` (chain, second parent, cycle, triple only in extensions.ttl); `tests/cli_find.rs::{find_tag_json, find_tag_exact_json}` |
| 4 | Records include `tags: [{id,label}]` | `tests/core_tag.rs::record_tags_sorted_by_label_with_pref_label` |
| 5 | Spec §14.1 discipline | `.goal/checks/rules_triggered.sh` (every `Error::` rule constructor appears in a test) |
| 6 | Schemas | `tests/schema_contract.rs` mapping of the new goldens; `UNKNOWN_CONCEPT` branch in `error.json`; `tests/core_schema.rs::unknown_concept_details_required` |

Every new `.snap` is hand-written from the spec and locked. No new leaf subcommand, so no new `dry_run_guard.rs` row.

## Approach

1. `concept::resolve` and `concept::suggest` (in-house Levenshtein) plus a POSIX quoting helper; `Error::unknown_concept` and its `error.json` branch.
2. `concept::add(.., broader)` and a repeatable CLI `--broader`.
3. The named shape (e.g. `hali:ResourceSubjectShape`) in `shapes.ttl` with fixtures; fall back to an `sh:sparql` constraint if rudof's `sh:node`/`sh:class` misbehaves.
4. Tags in `resource::add_all`; generalise `edit_title` into `edit(ctx, store, id, Edit{title, add_tags, remove_tags})` with one `validate::guard` and one `apply`; "at least one change" enforced in core.
5. `find`: constant SPARQL `SELECT DISTINCT ?r WHERE { ?r dcterms:subject/skos:broader* ?c }` (plain `dcterms:subject ?c` for `--exact`), `?c` bound per tag with `substitute_variable`, `SparqlEvaluator::new().without_default_http_service_handler()`, union-of-graphs semantics; intersect tag sets, then the existing filters, sort and paging.
6. Wire the CLI, goldens and the `schema_contract` mapping (`GOAL_SNAPS` / `GOAL_STDOUT_GOLDENS`).

Cross-cutting: implementer on opus for round one.

## Security call

Sensitive, low risk. Fixed internal SELECT; the Concept reference is resolved in Rust to an existing IRI and bound, never formatted into query text; not routed through `hali query` / `READ_ONLY_QUERY`. No change to `src/storage`, `src/paths`, `src/media_type` or `Cargo.toml`. `suggested_command` echoes the reference, so it must be shell-quoted. Security-reviewer optional, to check the quoting and that no `format!` builds query text.

## Risks

rudof `sh:node`/`sh:class` support; the label-uniqueness `sh:sparql` cost (#48) grows with each mutation.
