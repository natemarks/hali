# Plan for #7: SHACL on every mutation and `hali validate`

Approved by the human with all planner recommendations (open questions 1-13).

## Decisions
1. Accept rudof `shacl` `=0.3.35` (default `sparql` feature) plus `rudof_rdf` `=0.3.35` as a direct dependency. Cost accepted: Cargo.lock 132 -> ~340 packages; rudof_rdf forces oxigraph `http-client-rustls-native` and `rdf-12` and pulls tokio/reqwest/hyper/rustls; `default-features = false` cannot avoid it. `cargo audit` stays clean with the existing `.cargo/audit.toml`. Without `sparql`, `sh:sparql` constraints are silently ignored, so it stays on.
2. `extensions.ttl` containing any `sh:select` or `sh:ask` triple, or shapes that do not compile -> `SHAPES_INVALID` (exit 3). No SPARQL-based shapes from users (network egress via SERVICE and the SPARQL-XML path).
3. A mutation is refused whenever the would-be graph does not conform, including violations already in the store (spec §2.7 wording).
4. `hali validate` with violations: report on stdout AND the `SHACL_VIOLATION` envelope on stderr, exit 1.
5. The JSON Schema criterion moves to #6: out of scope here; #6's "every golden JSON snapshot has a schema" test covers the `validate` and `SHACL_VIOLATION` snapshots whichever PR lands first. If #6 is already on main when this PR is driven, add the schemas here.
6. Any result (including sh:Warning / sh:Info from extension shapes) is non-conforming and blocks. `severity` is in the output.
7. Label uniqueness is across different Concepts, same language tag (compared case-insensitively), folded for case and whitespace like LABEL_TAKEN. A Concept's own prefLabel vs altLabel clash is not checked.
8. `sh:targetClass skos:Concept` covers every skos:Concept in any file (external Concepts typed into extensions.ttl get scheme violations): accepted.
9. Label-uniqueness results point at the scheme (focus `hali:topics`, value = the Concept) for speed (7 ms vs ~570 ms at 500 Concepts).
10. Record the dependency finding in spec §16 (the shacl bullet) and in ADR-0001's consequences.
11. `validate` takes `--limit` / `--offset` (same rules as `concept list`: default 50, 0 is a usage error, reuse `parse_limit`).
12. New rule `SHAPES_INVALID` (exit 3); message gives no reason; rudof parser text is never echoed.
13. Leave the core ontology's `owl:versionInfo` at "0.1.0".

## Contract
- Shapes live in `src/ontology/shapes.ttl`, embedded with `include_str!` as `hali::ontology::SHAPES_TTL`. `storage::load` parses it into the `core` named graph next to `core.ttl` (so `hali query` (#9) can see the shapes). Only named shapes (no blank nodes). Shapes graph = core graph + extensions graph; data graph = all four named graphs. hali never hands Turtle text to rudof's parsers (no `RDFFormat`, `ReaderMode`, `from_path`, `from_str`, `merge_from_*`): triples are always copied from the oxigraph Store into `rudof_rdf::rdf_impl::OxigraphInMemory`, validated with `shacl::validator::store::Graph` -> `GraphValidation` -> `ShaclProcessor::validate(&IRSchema, &ShaclValidationMode::Native, &ShaclConfig::default())`; shapes via `ShaclParser::new(rdf).parse()` then `IRSchema::compile`.
- Shapes shipped now (Concept only): `hali:ConceptShape` (`sh:NodeShape`, `sh:targetClass skos:Concept`) with property shapes `hali:ConceptInSchemeShape` (`sh:path skos:inScheme ; sh:hasValue hali:topics ; sh:message "A Concept must be in the hali:topics scheme"@en`) and `hali:ConceptPrefLabelShape` (`sh:path skos:prefLabel ; sh:minCount 1 ; sh:uniqueLang true ; sh:message "A Concept needs exactly one skos:prefLabel per language"@en`); and `hali:LabelUniqueShape` (`sh:NodeShape ; sh:targetNode hali:topics ; sh:sparql hali:LabelUniqueConstraint`, `sh:message "label \"{?label}\" is used by more than one Concept"@en`), one result per (Concept, label) pair in a clash. The query (load-bearing, probe-verified; inside the Turtle triple-quoted string the regex is written `"\\\\s+"`):
```sparql
SELECT $this ?value ?label WHERE {
  { SELECT ?key ?lg WHERE { ?c a skos:Concept ; skos:inScheme $this ; skos:prefLabel|skos:altLabel ?l .
      BIND(LCASE(REPLACE(REPLACE(STR(?l), "\\s+", " "), "^ | $", "")) AS ?key) BIND(LCASE(LANG(?l)) AS ?lg) }
    GROUP BY ?key ?lg HAVING (COUNT(DISTINCT ?c) > 1) }
  ?value a skos:Concept ; skos:inScheme $this ; skos:prefLabel|skos:altLabel ?label .
  FILTER (LCASE(REPLACE(REPLACE(STR(?label), "\\s+", " "), "^ | $", "")) = ?key && LCASE(LANG(?label)) = ?lg) }
```
- Out of scope: the `dcterms:subject` shape (#16); Resource, relation and custom-property shapes (#10, #18); stale/duplicate-content info in `validate` (needs Resources; can be added later as a new member); JSON Schemas (#6).
- `concept add` order: normalise label (usage error, exit 2) -> LABEL_TAKEN (exit 1, the specific pre-check) -> build new quads -> SHACL on the store plus those quads (SHACL_VIOLATION, exit 1) -> insert. The SHACL label shape is the backstop for duplicates written by hand, so a new duplicate always reports LABEL_TAKEN.
- `SHACL_VIOLATION` (exit 1), stderr, usual envelope, stdout empty, nothing written; on a mutation `core` returns Err and the in-memory store is unchanged. Message on a mutation: `the change would leave 1 SHACL violation in the store; nothing was written` (`N SHACL violations` for more than one); from validate: `the store has 1 SHACL violation` / `the store has N SHACL violations`. `details`: `{"violations":[<at most 10 records, sorted>],"total":N,"truncated":bool}`. `suggested_fix`: `Fix the data or the extensions.ttl shapes the violations point at, then retry.` `suggested_command`: `hali validate` on a mutation, `null` from validate. Text mode: `error: <message>` and `hint: <fix>[ Try: <cmd>]`.
- Violation record keys in this order: `focus_node` (N-Triples form), `shape` (shape IRI or null for blank-node shape), `path` (predicate IRI, null for node-level results, rudof's Display text for complex paths), `constraint` (component IRI), `severity` (sh: local name, e.g. "Violation"), `value` (N-Triples form or null), `message`. Message choice: (1) the result's @en sh:message, (2) the source shape's sh:message from the shapes graph (@en, then untagged), (3) `<Local> constraint not satisfied` where `<Local>` is the component local name without `ConstraintComponent`. rudof's own engine text is never used. Results sorted by (focus_node, shape, path, constraint, value, message) comparing these output strings, null as "".
- `hali validate [--json] [--limit N] [--offset N]`: reads the store, writes nothing (not even a data dir). JSON stdout: `{"schema_version":1,"conforms":bool,"results":[...],"total":N,"truncated":bool,"next_offset":int|null}`. Text when conforming: `the store conforms to the SHACL shapes`. Text with violations: one line per result `<focus>\t<shape|->\t<value|->\t<message>`, IRIs compacted with the fixed prefix table, everything through `printable()`, a truncated page gets the `note:` line on stderr. Exit 0 conforming; 1 when total > 0 (and the SHACL_VIOLATION envelope / `error:`+`hint:` also on stderr); 3 for STORE_UNPARSEABLE, STORE_IO, SHAPES_INVALID.
- `SHAPES_INVALID` (exit 3): message `extensions.ttl has SHACL shapes that hali cannot use`; details `{"file":"extensions.ttl"}`; fix `Fix or remove the shapes in extensions.ttl; hali applies SHACL Core shapes from it.` Raised on validate and on every mutation; `concept list` does not compile shapes and keeps working. When data violates a user shape, validate reports it and mutations are refused.
- Constraints: `shacl` and `rudof_rdf` pinned `=0.3.35`; update the `.cargo/audit.toml` comment (quick-xml is now also reachable via `rudof_rdf`; oxigraph `http-client` is forced on); record the feature choice in spec §16 and ADR-0001 consequences; #41 and #9 are told that `http-client` can no longer be excluded, so `hali query` must use `SparqlEvaluator::without_default_http_service_handler()`.
- Determinism: rudof validates in parallel (rayon) and result order varies; hali sorts. Some engine messages are unordered (uniqueLang values); hali never uses engine text.

## Seams
- `hali::ontology::SHAPES_TTL`; `storage::load` loads it into the core graph; public access to the core and extensions graph IRIs.
- `hali::core::validate::validate(store: &Store, offset: usize, limit: usize) -> Result<Report, Error>`; `Report {conforms, results: Vec<Violation>, total, truncated, next_offset}`; `Violation {focus_node, shape: Option, path: Option, constraint, severity, value: Option, message}`.
- `hali::core::concept::add`: signature unchanged; can return `rule == "SHACL_VIOLATION"` (`exit_code() == 1`) and leaves the store unchanged.
- The `hali` binary: `hali validate`, `hali concept add`.
- rudof-facing code stays inside `src/ontology/` (spec §12). core does paging and error mapping; cli only renders.

## Goal map
Goal tests go in NEW files `tests/core_shacl.rs`, `tests/cli_validate.rs`, `tests/cli_shacl.rs`. EVERY item (use lines, helpers, tests) carries `#[cfg(feature = "goal")]` on its own line above it, following #3's pattern, otherwise `dead-code` fails.

New hand-written fixtures under `tests/fixtures/shacl/<name>/`:
- `not-in-scheme`: a Concept with no inScheme
- `other-scheme`: inScheme hali:other
- `no-pref-label`: a Concept with no prefLabel
- `two-pref-labels`: two @en prefLabels
- `pref-labels-two-languages`: conforming
- `duplicate-label`: "machine learning"@en vs " Machine  Learning"@en, plus an altLabel clashing with another Concept's prefLabel
- `same-label-other-language`: conforming
- `mixed`: vocabulary.ttl = hali:topics a skos:ConceptScheme; hali:ml (in topics, "machine learning"@en); hali:ml2 (in topics, "Machine  Learning"@en); hali:nolabel (in topics, no label); hali:orphan ("orphan"@en, no scheme); hali:twolabels (in topics, "two"@en and "deux"@en)
- `extension-shape`: vocabulary.ttl = hali:ml in topics with prefLabel "machine learning"@en and altLabel "ML"@en; extensions.ttl = ex:NeedsAltLabel (sh:targetClass skos:Concept) with property shape ex:AltLabelRequired (sh:path skos:altLabel ; sh:minCount 1 ; sh:message "Every Concept needs an alias"@en), ex: = <http://example.org/shapes#>
- `extension-invalid`: `sh:minCount "abc"` plus an `sh:message "SECRET-CANARY-4242"`
- `extension-sparql`: a valid user sh:sparql constraint
- `many-violations`: 12 Concepts c01..c12 with prefLabels and no inScheme
- `hostile-duplicate`: two Concepts whose duplicate labels contain ESC, BEL and \n

Hand-written locked snapshots:
- `tests/snapshots/cli_validate__validate_json_conforming.snap` (fixture tests/fixtures/store): `{"schema_version":1,"conforms":true,"results":[],"total":0,"truncated":false,"next_offset":null}`
- `tests/snapshots/cli_validate__validate_json_violations.snap` (fixture `mixed`, stdout): conforms false, total 5, truncated false, next_offset null, results in this order (full IRIs, N-Triples focus nodes, "severity":"Violation"): (1) nolabel / ConceptPrefLabelShape / skos:prefLabel / MinCount / value null / "A Concept needs exactly one skos:prefLabel per language"; (2) orphan / ConceptInSchemeShape / skos:inScheme / HasValue / value null / "A Concept must be in the hali:topics scheme"; (3) topics / LabelUniqueShape / path null / SPARQLConstraintComponent / value "<https://w3id.org/hali#ml>" / `label \"machine learning\" is used by more than one Concept`; (4) the same with value "<https://w3id.org/hali#ml2>" / `label \"Machine  Learning\" is used by more than one Concept`; (5) twolabels / ConceptPrefLabelShape / skos:prefLabel / UniqueLang / value null / the prefLabel message
- `tests/snapshots/cli_validate__validate_json_violations_error.snap` (stderr of the same run): the SHACL_VIOLATION envelope with message `the store has 5 SHACL violations`, the same 5 records in details.violations, total 5, truncated false, suggested_command null
- `tests/snapshots/cli_shacl__shacl_violation_json_envelope.snap` (`hali concept add "deep learning" --json` on `extension-shape`): `{"schema_version":1,"error":{"rule":"SHACL_VIOLATION","message":"the change would leave 1 SHACL violation in the store; nothing was written","details":{"violations":[{"focus_node":"<https://w3id.org/hali#deep-learning>","shape":"http://example.org/shapes#AltLabelRequired","path":"http://www.w3.org/2004/02/skos/core#altLabel","constraint":"http://www.w3.org/ns/shacl#MinCountConstraintComponent","severity":"Violation","value":null,"message":"Every Concept needs an alias"}],"total":1,"truncated":false},"suggested_fix":"Fix the data or the extensions.ttl shapes the violations point at, then retry.","suggested_command":"hali validate"}}`

| # | Criterion | Check | Expected failure before implementation |
|---|---|---|---|
| 1 | Core shapes embedded, named, in the core graph | `tests/core_shacl.rs::shapes_are_embedded_and_loaded_into_the_core_graph`, `::core_shapes_have_no_blank_nodes` | `cannot find value SHAPES_TTL in module hali::ontology` |
| 2 | Extension shapes applied; a mutation that breaks one is refused | `core_shacl.rs::extension_shapes_are_applied`, `::extension_shape_conforming_store_conforms`; `cli_shacl.rs::concept_add_refused_by_extension_shape` | `unresolved import hali::core::validate`; `assert Some(0) == Some(1)` |
| 3 | Uncompilable or sh:select/sh:ask extension shapes -> exit 3 SHAPES_INVALID, nothing written, no file text echoed, `concept list` still works | `core_shacl.rs::uncompilable_extension_shapes_are_shapes_invalid`, `::sparql_in_extension_shapes_is_shapes_invalid`; `cli_validate.rs::invalid_extension_shapes_exit_3` | compile error; `assert Some(2) == Some(3)` |
| 4 | Every Concept in hali:topics | `core_shacl.rs::concept_not_in_topics_violates_in_scheme_shape`, `::concept_in_other_scheme_violates_in_scheme_shape` | compile error |
| 5 | Exactly one prefLabel per language | `core_shacl.rs::concept_without_pref_label_violates_pref_label_shape`, `::two_pref_labels_in_one_language_violate_pref_label_shape`, `::pref_labels_in_two_languages_conform` | compile error |
| 6 | Labels unique across Concepts per language (case- and space-folded) | `core_shacl.rs::duplicate_labels_violate_label_unique_shape`, `::same_label_in_other_language_conforms` | compile error |
| 7 | Every shape has a conforming and a violating fixture (§14.3) | `core_shacl.rs::conforming_fixtures_have_no_violations` (store, first-add, three-adds, noncanonical), `::every_core_shape_has_a_violating_fixture` (the set of named shapes in SHAPES_TTL equals the test's shape->fixture table; stays as a guard for #10, #16, #18) | compile error |
| 8 | Deterministic order and paging | `core_shacl.rs::report_order_is_deterministic` (5 runs identical and sorted), `::report_pages` | compile error |
| 9 | `validate --json` conformance, exit 0 | `cli_validate.rs::validate_json_conforming` (snap) | `assert Some(2) == Some(0)` |
| 10 | `validate` exits 1 on violations: report on stdout, SHACL_VIOLATION on stderr | `cli_validate.rs::validate_json_violations` (2 snaps), `::validate_text_violations` (exact lines), `::validate_text_conforming`, `::validate_json_paged` (`--limit 2 --offset 1`: total 5, truncated, next_offset 3, exit 1), `::validate_limit_zero_is_a_usage_error` | `assert Some(2) == Some(1)` |
| 11 | `validate` read-only; store problems exit 3 | `cli_validate.rs::validate_on_missing_store_conforms_and_creates_nothing`, `::validate_writes_nothing`, `::validate_unparseable_store_exits_3` | `assert Some(2) == Some(0)` / `Some(3)` |
| 12 | Text output escapes stored control characters | `cli_validate.rs::validate_text_escapes_control_characters` (`hostile-duplicate`) | `assert Some(2) == Some(1)` |
| 13 | A mutation that would not conform is refused with SHACL_VIOLATION (exit 1), nothing written, in-memory store unchanged | `core_shacl.rs::concept_add_refused_by_shacl_leaves_store_unchanged`; `cli_shacl.rs::shacl_violation_json_envelope` (snap, plus vocabulary bytes unchanged), `::shacl_violation_text_two_lines` | compile error; `assert Some(0) == Some(1)` |
| 14 | A violation already in the store also refuses a mutation | `core_shacl.rs::concept_add_refused_on_pre_existing_violation` (`duplicate-label` plus `add "quantum computing"`) | compile error |
| 15 | LABEL_TAKEN takes precedence over SHACL | `core_shacl.rs::label_taken_is_checked_before_shacl` (`extension-shape` plus a duplicate of "ML") | passes before the change (a guard); fails with `"SHACL_VIOLATION" != "LABEL_TAKEN"` if SHACL runs first |
| 16 | SHACL_VIOLATION errors stay bounded | `cli_shacl.rs::shacl_violation_details_are_capped` (`many-violations`: 10 listed, total 12, truncated, stderr < 10 000 chars) | `assert Some(0) == Some(1)` |
| 17 | Spec §14.1 | `.goal/checks/rules-have-tests.sh`: SHACL_VIOLATION and SHAPES_INVALID are in `src/` and triggered in `tests/cli_*.rs` or the snaps; `validate::validate` is in `core_shacl.rs`; `"validate"` is in `cli_validate.rs`; the 4 snaps exist | `rule SHACL_VIOLATION is not implemented in src/` |
| 18 | Dependency hygiene | `.goal/checks/shacl-dependency.sh`: `shacl` and `rudof_rdf` pinned `=0.3.35` in Cargo.toml; `cargo tree -d -e normal --depth 0` lists no oxigraph, oxrdf, oxttl or oxrdfio duplicates; `src/` has none of `RDFFormat`, `ReaderMode`, `from_path(`, `merge_from_`; `.cargo/audit.toml` mentions `rudof` | `shacl is not pinned in Cargo.toml` |
| 19 | Docs record the feature choice | `.goal/checks/docs.sh`: the shacl bullet in `docs/spec.md` §16 names the `sparql` feature and the forced oxigraph `http-client`/`rdf-12`; ADR-0001 consequences mention it | `spec §16 does not record the shacl features` |
| - | JSON Schema criterion | moved to #6 | - |

Locked paths (`.goal/locked`): `tests/core_shacl.rs`, `tests/cli_validate.rs`, `tests/cli_shacl.rs`, `tests/fixtures/`, `tests/snapshots/`. (Locking the two directories whole protects the existing goldens from being re-blessed.)

## Approach
Cross-cutting: implementer and reviewer start at opus. Order: (1) dependencies `shacl = "=0.3.35"`, `rudof_rdf = "=0.3.35"` and the `audit.toml` comment; (2) `src/ontology/shapes.ttl`, `SHAPES_TTL`, one line in `storage::load`, public graph IRIs; (3) `src/ontology/shacl.rs`: copy quads of all graphs into an `OxigraphInMemory` (and core + extensions into a second one for the shapes), reject `sh:select`/`sh:ask` in the extensions graph, parse/compile shapes, run Native validation, map results to hali's `Violation`; accept extra quads so the would-be graph can be validated without touching the store (#8's dry run reuses this); (4) `core::validate` (sorting, paging, error constructors) and the call from `concept::add` before inserting; (5) `cli` for `validate`. Risks: ~80-90 ms added to every mutation at 10k Resources (#26 `make bench` should measure it); rayon reads `RAYON_NUM_THREADS`; `rdf-12` is on, so hand-edited files may contain RDF 1.2 triple terms (#39 should know); rudof 0.3.x moves fast (pinned).

## Security
Sensitive: Cargo.toml/lock (large new supply chain incl. an HTTP stack), `src/storage/`, new input handling (shapes in the hand-written `extensions.ttl`). The security-reviewer runs. The Turtle files are same-user trusted (maintainer is the only writer), but SHACL-SPARQL in extensions.ttl would turn a data file into network egress plus SPARQL-XML parsing via the quick-xml exception: hence `sh:select`/`sh:ask` are refused. Messages derived from stored labels are escaped in text mode; error details never echo rudof's parser text; `validate` writes nothing. The consequential call (accepting the dependency) stays with the human.
