//! Permanent-in-spirit contract test (#6, spec §14.6): every golden JSON snapshot in
//! tests/snapshots conforms to the schema `hali schema <command>` prints. A stdout snapshot
//! must be mapped to its command in `STDOUT_GOLDENS`; a stderr snapshot is the error envelope
//! and is validated against `error`. Adding a snapshot without mapping it fails the test.

mod goal {
    use assert_cmd::Command;
    use serde_json::{Value, json};
    use std::path::{Path, PathBuf};

    /// Snapshot file stem -> the `hali schema` name its stdout JSON must satisfy.
    const STDOUT_GOLDENS: &[(&str, &str)] = &[
        ("cli_concept__concept_add_json", "concept add"),
        ("cli_concept__concept_list_json", "concept list"),
        ("cli_concept__concept_list_json_empty", "concept list"),
        ("cli_concept__concept_list_json_paged", "concept list"),
        ("cli_dry_run__concept_add_dry_run_json", "concept add"),
        (
            "cli_dry_run__concept_add_dry_run_json_existing_scheme",
            "concept add",
        ),
        (
            "cli_dry_run__concept_add_dry_run_json_suffix_order",
            "concept add",
        ),
        ("cli_schema__schema_list_json", "schema"),
        ("cli_validate__validate_json_conforming", "validate"),
        ("cli_validate__validate_json_violations", "validate"),
    ];

    /// Snapshots written by the active goal (#10). A build without the goal feature does not
    /// see them, because the schemas they name do not exist until the goal is met; settle
    /// removes the tags below, which makes every one of them visible and mapped.
    const GOAL_SNAPS: &[&str] = &[
        "cli_dry_run__add_dry_run_json",
        "cli_dry_run__edit_dry_run_json",
        "cli_resource__add_json",
        "cli_resource__edit_json",
        "cli_resource__re_add_json_existing",
        "cli_resource__show_json",
    ];

    const GOAL_STDOUT_GOLDENS: &[(&str, &str)] = &[
        ("cli_dry_run__add_dry_run_json", "add"),
        ("cli_dry_run__edit_dry_run_json", "edit"),
        ("cli_resource__add_json", "add"),
        ("cli_resource__edit_json", "edit"),
        ("cli_resource__re_add_json_existing", "add"),
        ("cli_resource__show_json", "show"),
    ];

    /// Snapshots written by the active goal (#12): hidden from a build without the goal
    /// feature, because the `find` schema they name does not exist until the goal is met.
    const GOAL_SNAPS_12: &[&str] = &[
        "cli_find__find_json",
        "cli_find__find_json_empty",
        "cli_find__find_json_fields",
    ];

    const GOAL_STDOUT_GOLDENS_12: &[(&str, &str)] = &[
        ("cli_find__find_json", "find"),
        ("cli_find__find_json_empty", "find"),
        ("cli_find__find_json_fields", "find"),
    ];

    /// Snapshots written by the active goal (#16): hidden from a build without the goal
    /// feature (the `UNKNOWN_CONCEPT` envelope only validates once the `error` schema has its
    /// branch, and the others only once the commands take the new flags).
    const GOAL_SNAPS_16: &[&str] = &[
        "cli_concept__concept_add_broader_json",
        "cli_find__find_tag_exact_json",
        "cli_find__find_tag_json",
        "cli_tag__add_tag_json",
        "cli_tag__edit_add_remove_tag_json",
        "cli_tag__unknown_concept_json_envelope",
    ];

    const GOAL_STDOUT_GOLDENS_16: &[(&str, &str)] = &[
        ("cli_concept__concept_add_broader_json", "concept add"),
        ("cli_find__find_tag_exact_json", "find"),
        ("cli_find__find_tag_json", "find"),
        ("cli_tag__add_tag_json", "add"),
        ("cli_tag__edit_add_remove_tag_json", "edit"),
    ];

    /// Snapshots written by the active goal (#14): hidden from a build without the goal
    /// feature (the `forget` schema and the `CONFIRMATION_REQUIRED` branch they name do not
    /// exist until the goal is met).
    const GOAL_SNAPS_14: &[&str] = &[
        "cli_dry_run__forget_dry_run_json",
        "cli_forget__confirmation_required_json_envelope",
        "cli_forget__forget_json",
    ];

    const GOAL_STDOUT_GOLDENS_14: &[(&str, &str)] = &[
        ("cli_dry_run__forget_dry_run_json", "forget"),
        ("cli_forget__forget_json", "forget"),
    ];

    /// Snapshots written by the active goal (#17): hidden from a build without the goal
    /// feature (the `concept show|rename|edit|remove` schemas and the `IN_USE` branch they
    /// name do not exist until the goal is met).
    const GOAL_SNAPS_17: &[&str] = &[
        "cli_concept_manage__concept_edit_json",
        "cli_concept_manage__concept_remove_json",
        "cli_concept_manage__concept_rename_json",
        "cli_concept_manage__concept_show_json",
        "cli_concept_manage__in_use_json_envelope",
        "cli_concept_manage__remove_confirmation_required_json_envelope",
        "cli_dry_run__concept_edit_dry_run_json",
        "cli_dry_run__concept_remove_dry_run_json",
        "cli_dry_run__concept_rename_dry_run_json",
    ];

    const GOAL_STDOUT_GOLDENS_17: &[(&str, &str)] = &[
        ("cli_concept_manage__concept_edit_json", "concept edit"),
        ("cli_concept_manage__concept_remove_json", "concept remove"),
        ("cli_concept_manage__concept_rename_json", "concept rename"),
        ("cli_concept_manage__concept_show_json", "concept show"),
        ("cli_dry_run__concept_edit_dry_run_json", "concept edit"),
        ("cli_dry_run__concept_remove_dry_run_json", "concept remove"),
        ("cli_dry_run__concept_rename_dry_run_json", "concept rename"),
    ];

    /// Snapshots written by the active goal (#15): hidden from a build without the goal
    /// feature (the `type add|list|remove` schemas, the `UNKNOWN_TYPE` branch and the Type
    /// shape of `IN_USE` they name do not exist until the goal is met, and `types` in the
    /// `add`, `edit` and `find` goldens only validates once the commands take the new flags).
    const GOAL_SNAPS_15: &[&str] = &[
        "cli_dry_run__type_add_dry_run_json",
        "cli_dry_run__type_remove_dry_run_json",
        "cli_type__add_type_json",
        "cli_type__edit_add_remove_type_json",
        "cli_type__find_type_json",
        "cli_type__type_add_json",
        "cli_type__type_in_use_json_envelope",
        "cli_type__type_label_taken_json_envelope",
        "cli_type__type_list_json",
        "cli_type__type_remove_confirmation_required_json_envelope",
        "cli_type__type_remove_json",
        "cli_type__unknown_type_json_envelope",
    ];

    const GOAL_STDOUT_GOLDENS_15: &[(&str, &str)] = &[
        ("cli_dry_run__type_add_dry_run_json", "type add"),
        ("cli_dry_run__type_remove_dry_run_json", "type remove"),
        ("cli_type__add_type_json", "add"),
        ("cli_type__edit_add_remove_type_json", "edit"),
        ("cli_type__find_type_json", "find"),
        ("cli_type__type_add_json", "type add"),
        ("cli_type__type_list_json", "type list"),
        ("cli_type__type_remove_json", "type remove"),
    ];

    /// Snapshots written by the active goal (#18): hidden from a build without the goal
    /// feature (the `relate`, `unrelate` and `relation add|list|remove` schemas, the
    /// `UNKNOWN_RELATION` and `FORGET_BLOCKED_BY_RELATIONS` branches and the relation shapes of
    /// `IN_USE` and `LABEL_TAKEN` they name do not exist until the goal is met).
    const GOAL_SNAPS_18: &[&str] = &[
        "cli_dry_run__relate_dry_run_json",
        "cli_dry_run__relation_add_dry_run_json",
        "cli_dry_run__relation_remove_dry_run_json",
        "cli_dry_run__unrelate_dry_run_json",
        "cli_relation__find_related_to_json",
        "cli_relation__find_related_via_json",
        "cli_relation__forget_blocked_json_envelope",
        "cli_relation__relate_json",
        "cli_relation__relation_add_json",
        "cli_relation__relation_in_use_json_envelope",
        "cli_relation__relation_label_taken_json_envelope",
        "cli_relation__relation_list_json",
        "cli_relation__relation_remove_confirmation_required_json_envelope",
        "cli_relation__relation_remove_json",
        "cli_relation__show_relations_json",
        "cli_relation__unknown_relation_json_envelope",
        "cli_relation__unrelate_json",
    ];

    const GOAL_STDOUT_GOLDENS_18: &[(&str, &str)] = &[
        ("cli_dry_run__relate_dry_run_json", "relate"),
        ("cli_dry_run__relation_add_dry_run_json", "relation add"),
        (
            "cli_dry_run__relation_remove_dry_run_json",
            "relation remove",
        ),
        ("cli_dry_run__unrelate_dry_run_json", "unrelate"),
        ("cli_relation__find_related_to_json", "find"),
        ("cli_relation__find_related_via_json", "find"),
        ("cli_relation__relate_json", "relate"),
        ("cli_relation__relation_add_json", "relation add"),
        ("cli_relation__relation_list_json", "relation list"),
        ("cli_relation__relation_remove_json", "relation remove"),
        ("cli_relation__show_relations_json", "show"),
        ("cli_relation__unrelate_json", "unrelate"),
    ];

    /// Snapshots written by the active goal (#19): hidden from a build without the goal
    /// feature (the `describe` schema they name does not exist until the goal is met).
    const GOAL_SNAPS_19: &[&str] = &[
        "cli_describe__describe_brief_json",
        "cli_describe__describe_json",
        "cli_describe__describe_json_empty",
    ];

    #[cfg(feature = "goal")]
    const GOAL_STDOUT_GOLDENS_19: &[(&str, &str)] = &[
        ("cli_describe__describe_brief_json", "describe"),
        ("cli_describe__describe_json", "describe"),
        ("cli_describe__describe_json_empty", "describe"),
    ];

    /// Every stdout snapshot's row.
    #[allow(unused_mut)]
    fn stdout_goldens() -> Vec<(&'static str, &'static str)> {
        let mut rows = STDOUT_GOLDENS.to_vec();
        rows.extend_from_slice(GOAL_STDOUT_GOLDENS);
        rows.extend_from_slice(GOAL_STDOUT_GOLDENS_12);
        rows.extend_from_slice(GOAL_STDOUT_GOLDENS_16);
        rows.extend_from_slice(GOAL_STDOUT_GOLDENS_14);
        rows.extend_from_slice(GOAL_STDOUT_GOLDENS_17);
        rows.extend_from_slice(GOAL_STDOUT_GOLDENS_15);
        rows.extend_from_slice(GOAL_STDOUT_GOLDENS_18);
        #[cfg(feature = "goal")]
        rows.extend_from_slice(GOAL_STDOUT_GOLDENS_19);
        rows
    }

    struct Golden {
        stem: String,
        stream: String,
        body: String,
    }

    fn snapshots_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots")
    }

    #[allow(unused_assignments, unused_mut)]
    fn goldens() -> Vec<Golden> {
        let mut hidden: &[&str] = GOAL_SNAPS;
        {
            hidden = &[];
        }
        let mut pending: &[&str] = GOAL_SNAPS_12;
        {
            pending = &[];
        }
        let mut pending_16: &[&str] = GOAL_SNAPS_16;
        {
            pending_16 = &[];
        }
        let mut pending_14: &[&str] = GOAL_SNAPS_14;
        {
            pending_14 = &[];
        }
        let mut pending_17: &[&str] = GOAL_SNAPS_17;
        {
            pending_17 = &[];
        }
        let mut pending_15: &[&str] = GOAL_SNAPS_15;
        {
            pending_15 = &[];
        }
        let mut pending_18: &[&str] = GOAL_SNAPS_18;
        {
            pending_18 = &[];
        }
        let mut pending_19: &[&str] = GOAL_SNAPS_19;
        #[cfg(feature = "goal")]
        {
            pending_19 = &[];
        }
        let mut out = Vec::new();
        for entry in std::fs::read_dir(snapshots_dir()).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) != Some("snap") {
                continue;
            }
            let stem = path.file_stem().unwrap().to_str().unwrap().to_string();
            if hidden.contains(&stem.as_str())
                || pending.contains(&stem.as_str())
                || pending_16.contains(&stem.as_str())
                || pending_14.contains(&stem.as_str())
                || pending_17.contains(&stem.as_str())
                || pending_15.contains(&stem.as_str())
                || pending_18.contains(&stem.as_str())
                || pending_19.contains(&stem.as_str())
            {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let rest = text
                .strip_prefix("---\n")
                .unwrap_or_else(|| panic!("{stem}: no insta header"));
            let (header, body) = rest
                .split_once("\n---\n")
                .unwrap_or_else(|| panic!("{stem}: unterminated insta header"));
            let expression = header
                .lines()
                .find_map(|l| l.strip_prefix("expression: "))
                .unwrap_or_else(|| panic!("{stem}: no expression in header"));
            let stream = match expression.trim() {
                "out.stdout" => "stdout",
                "out.stderr" => "stderr",
                other => panic!("{stem}: unsupported snapshot expression `{other}`"),
            };
            out.push(Golden {
                stem,
                stream: stream.to_string(),
                body: body.trim_end().to_string(),
            });
        }
        out.sort_by(|a, b| a.stem.cmp(&b.stem));
        out
    }

    fn schema_name(g: &Golden) -> Option<&'static str> {
        if g.stream == "stderr" {
            return Some("error");
        }
        stdout_goldens()
            .into_iter()
            .find(|(stem, _)| *stem == g.stem)
            .map(|(_, name)| name)
    }

    /// `hali schema <name>` through the real binary, so the test sees what an agent sees.
    fn fetch(name: &str) -> Value {
        let root = tempfile::tempdir().unwrap();
        let out = Command::cargo_bin("hali")
            .unwrap()
            .env_clear()
            .env("XDG_CONFIG_HOME", root.path().join("cfg"))
            .env("HOME", root.path().join("home"))
            .env("HALI_DATA_DIR", root.path().join("data"))
            .arg("schema")
            .args(name.split(' '))
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "`hali schema {name}` failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("schema {name}: {e}"))
    }

    /// The names `hali schema --json` lists.
    fn fetch_list() -> Vec<String> {
        let root = tempfile::tempdir().unwrap();
        let out = Command::cargo_bin("hali")
            .unwrap()
            .env_clear()
            .env("XDG_CONFIG_HOME", root.path().join("cfg"))
            .env("HOME", root.path().join("home"))
            .env("HALI_DATA_DIR", root.path().join("data"))
            .args(["schema", "--json"])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "`hali schema --json` failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let v: Value = serde_json::from_slice(&out.stdout).unwrap();
        v["schemas"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_str().unwrap().to_string())
            .collect()
    }

    fn validator(name: &str) -> jsonschema::Validator {
        jsonschema::draft202012::new(&fetch(name))
            .unwrap_or_else(|e| panic!("schema `{name}` is not a valid draft 2020-12 schema: {e}"))
    }

    fn problems(v: &jsonschema::Validator, instance: &Value) -> Vec<String> {
        v.iter_errors(instance)
            .map(|e| format!("{} at {}", e, e.instance_path()))
            .collect()
    }

    #[test]
    fn every_golden_validates() {
        let goldens = goldens();
        assert!(goldens.len() >= 14, "found only {} goldens", goldens.len());
        assert!(goldens.len() >= 23, "found only {} goldens", goldens.len());
        assert!(goldens.len() >= 29, "found only {} goldens", goldens.len());
        assert!(goldens.len() >= 32, "found only {} goldens", goldens.len());
        assert!(goldens.len() >= 41, "found only {} goldens", goldens.len());
        assert!(goldens.len() >= 53, "found only {} goldens", goldens.len());
        assert!(goldens.len() >= 70, "found only {} goldens", goldens.len());
        #[cfg(feature = "goal")]
        assert!(goldens.len() >= 73, "found only {} goldens", goldens.len());
        for g in &goldens {
            let name = schema_name(g)
                .unwrap_or_else(|| panic!("golden `{}` is not mapped to a schema", g.stem));
            let instance: Value = serde_json::from_str(&g.body)
                .unwrap_or_else(|e| panic!("golden `{}` is not JSON: {e}", g.stem));
            let errs = problems(&validator(name), &instance);
            assert!(
                errs.is_empty(),
                "golden `{}` does not satisfy `hali schema {name}`:\n{}",
                g.stem,
                errs.join("\n")
            );
        }
    }

    #[test]
    fn every_snap_is_mapped() {
        for g in goldens() {
            assert!(
                schema_name(&g).is_some(),
                "snapshot `{}` (stdout) has no row in STDOUT_GOLDENS in tests/schema_contract.rs: \
                 map it to the `hali schema` name its JSON must satisfy",
                g.stem
            );
            // ...and that name must be a schema the binary actually serves.
            fetch(schema_name(&g).unwrap());
        }
    }

    #[test]
    fn no_stale_rows() {
        let stems: Vec<String> = goldens().into_iter().map(|g| g.stem).collect();
        for (stem, name) in stdout_goldens() {
            assert!(
                stems.iter().any(|s| s == stem),
                "STDOUT_GOLDENS row `{stem}` -> `{name}` has no snapshot file"
            );
            assert!(
                goldens()
                    .iter()
                    .any(|g| g.stem == stem && g.stream == "stdout"),
                "STDOUT_GOLDENS row `{stem}` is not a stdout snapshot"
            );
            fetch(name);
        }
        // The list a row's name comes from is itself live output.
        let listed = fetch_list();
        for (stem, name) in stdout_goldens() {
            assert!(
                listed.iter().any(|n| n == name),
                "row `{stem}` names `{name}`, which `hali schema` does not list: {listed:?}"
            );
        }
    }

    /// The command a dry-run golden belongs to, from its stem:
    /// `cli_dry_run__concept_add_dry_run_json` -> `concept add`.
    fn dry_run_command(stem: &str) -> String {
        let rest = stem.strip_prefix("cli_dry_run__").unwrap_or_else(|| {
            panic!("{stem}: a dry-run golden is named cli_dry_run__<command>_dry_run...")
        });
        let command = rest.split("_dry_run").next().unwrap();
        command.replace('_', " ")
    }

    /// Every `*dry_run*` golden maps to its own command's schema and says `dry_run: true`.
    #[test]
    fn dry_run_goldens_named() {
        let dry: Vec<Golden> = goldens()
            .into_iter()
            .filter(|g| g.stem.contains("dry_run"))
            .collect();
        assert!(dry.len() >= 3, "expected the three #8 dry-run goldens");
        for g in dry {
            assert_eq!(g.stream, "stdout", "{}", g.stem);
            let command = dry_run_command(&g.stem);
            assert_eq!(
                schema_name(&g),
                Some(command.as_str()),
                "{} must map to the schema of its own command",
                g.stem
            );
            assert!(g.body.contains("\"dry_run\":true"), "{}", g.stem);
            let instance: Value = serde_json::from_str(&g.body).unwrap();
            let errs = problems(&validator(&command), &instance);
            assert!(errs.is_empty(), "{}: {errs:?}", g.stem);
        }
    }

    fn concept() -> Value {
        json!({"id": "ml", "label": "machine learning", "alt_labels": [], "broader": []})
    }

    fn violation() -> Value {
        json!({
            "focus_node": "<https://w3id.org/hali#nolabel>",
            "shape": "https://w3id.org/hali#ConceptPrefLabelShape",
            "path": "http://www.w3.org/2004/02/skos/core#prefLabel",
            "constraint": "http://www.w3.org/ns/shacl#MinCountConstraintComponent",
            "severity": "Violation",
            "value": null,
            "message": "A Concept needs exactly one skos:prefLabel per language"
        })
    }

    fn error(rule: &str, details: Value) -> Value {
        json!({"schema_version": 1, "error": {
            "rule": rule, "message": "m", "details": details,
            "suggested_fix": "f", "suggested_command": null}})
    }

    #[test]
    fn schemas_reject() {
        let change = json!({"op": "add", "triple": "<a:b> <a:c> <a:d> ."});
        let real = json!({"schema_version": 1, "concept": concept()});
        let mut dry = real.clone();
        dry["dry_run"] = json!(true);
        dry["changes"] = json!([change]);

        let mut cases: Vec<(&str, Value, bool)> = Vec::new();
        let mut case = |name: &'static str, instance: Value, valid: bool| {
            cases.push((name, instance, valid));
        };

        // concept add: the real payload xor the dry-run payload.
        case("concept add", real.clone(), true);
        case("concept add", dry.clone(), true);
        let mut empty_changes = dry.clone();
        empty_changes["changes"] = json!([]);
        case("concept add", empty_changes, true);
        let mut v = dry.clone();
        v["dry_run"] = json!(false);
        case("concept add", v, false);
        let mut v = dry.clone();
        v.as_object_mut().unwrap().remove("changes");
        case("concept add", v, false);
        let mut v = real.clone();
        v["changes"] = json!([]);
        case("concept add", v, false);
        let mut v = real.clone();
        v["dry_run"] = json!(true);
        case("concept add", v, false);
        let mut v = dry.clone();
        v["changes"] = json!([{"op": "replace", "triple": "<a:b> <a:c> <a:d> ."}]);
        case("concept add", v, false);
        let mut v = dry.clone();
        v["changes"] = json!([{"op": "add", "triple": 7}]);
        case("concept add", v, false);
        let mut v = dry.clone();
        v["changes"] = json!([{"op": "add", "triple": "<a:b> <a:c> <a:d> .", "extra": 1}]);
        case("concept add", v, false);
        let mut v = real.clone();
        v["schema_version"] = json!(2);
        case("concept add", v, false);
        let mut v = real.clone();
        v["concept"].as_object_mut().unwrap().remove("label");
        case("concept add", v, false);
        let mut v = real.clone();
        v["surprise"] = json!(true);
        case("concept add", v, false);

        // concept list.
        let list = json!({"schema_version": 1, "results": [concept()], "total": 1,
            "truncated": false, "next_offset": null});
        case("concept list", list.clone(), true);
        let mut v = list.clone();
        v["next_offset"] = json!(3);
        case("concept list", v, true);
        let mut v = list.clone();
        v.as_object_mut().unwrap().remove("total");
        case("concept list", v, false);
        let mut v = list.clone();
        v["next_offset"] = json!("3");
        case("concept list", v, false);
        let mut v = list.clone();
        v["results"][0].as_object_mut().unwrap().remove("id");
        case("concept list", v, false);
        case("concept list", real.clone(), false);

        // validate.
        let report = json!({"schema_version": 1, "conforms": false, "results": [violation()],
            "total": 1, "truncated": false, "next_offset": null});
        case("validate", report.clone(), true);
        let mut v = report.clone();
        v["conforms"] = json!("no");
        case("validate", v, false);
        let mut v = report.clone();
        v["results"][0].as_object_mut().unwrap().remove("shape");
        case("validate", v, false);
        case("validate", list.clone(), false);

        // schema (the no-argument list output).
        case(
            "schema",
            json!({"schema_version": 1, "schemas": ["concept add", "error"]}),
            true,
        );
        case(
            "schema",
            json!({"schema_version": 1, "schemas": [1]}),
            false,
        );
        case("schema", json!({"schema_version": 1}), false);

        // error: the envelope, the rule pattern, and per-rule details.
        case("error", error("SOME_RULE", json!({})), true);
        case("error", error("unknown_concept", json!({})), false);
        case("error", error("", json!({})), false);
        let mut v = error("SOME_RULE", json!({}));
        v.as_object_mut().unwrap().remove("schema_version");
        case("error", v, false);
        let mut v = error("SOME_RULE", json!({}));
        v["error"].as_object_mut().unwrap().remove("message");
        case("error", v, false);
        case(
            "error",
            error(
                "SHACL_VIOLATION",
                json!({"violations": [violation()], "total": 1, "truncated": false}),
            ),
            true,
        );
        case("error", error("SHACL_VIOLATION", json!({})), false);
        case(
            "error",
            error(
                "SHACL_VIOLATION",
                json!({"violations": [{"focus_node": "x"}], "total": 1, "truncated": false}),
            ),
            false,
        );
        case(
            "error",
            error("SHAPES_INVALID", json!({"file": "extensions.ttl"})),
            true,
        );
        case("error", error("SHAPES_INVALID", json!({})), false);
        case(
            "error",
            error(
                "LABEL_TAKEN",
                json!({"label": "ml", "language": "en", "concept": "ml"}),
            ),
            true,
        );
        case(
            "error",
            error("LABEL_TAKEN", json!({"label": "ml", "language": "en"})),
            false,
        );
        case("error", real.clone(), false);

        for (name, instance, valid) in cases {
            let errs = problems(&validator(name), &instance);
            assert_eq!(
                errs.is_empty(),
                valid,
                "`hali schema {name}` should {} {instance}; errors: {errs:?}",
                if valid { "accept" } else { "reject" }
            );
        }
    }

    fn record() -> Value {
        json!({"id": "018bcfe5-6801-710a-adec-89025cc1beeb", "path": "/a/notes.txt",
            "title": "notes", "media_type": "text/plain", "editable": true, "missing": false,
            "stale": false, "types": [], "tags": [{"id": "ml", "label": "machine learning"}],
            "byte_size": 6, "modified": "2026-10-20T14:02:11Z"})
    }

    fn shown() -> Value {
        let mut r = record();
        r["relations"] = json!([]);
        r["catalog_record"] = json!({"id": "c", "issued": "2026-10-24T09:30:00Z",
            "modified": "2026-10-24T09:30:00Z",
            "attributed_to": {"id": "p", "name": "nate"}});
        r
    }

    /// #10: what the `add`, `show` and `edit` schemas accept and refuse.
    #[test]
    fn resource_schemas_accept_and_reject() {
        let change = json!({"op": "add", "triple": "<a:b> <a:c> <a:d> ."});
        let add = json!({"schema_version": 1, "added": [record()], "existing": []});
        let mut add_dry = add.clone();
        add_dry["dry_run"] = json!(true);
        add_dry["changes"] = json!([change]);
        let edit = json!({"schema_version": 1, "resource": record()});
        let mut edit_dry = edit.clone();
        edit_dry["dry_run"] = json!(true);
        edit_dry["changes"] = json!([]);
        let show = json!({"schema_version": 1, "resource": shown()});

        let mut cases: Vec<(&str, Value, bool)> = vec![
            ("add", add.clone(), true),
            ("add", add_dry.clone(), true),
            ("edit", edit.clone(), true),
            ("edit", edit_dry.clone(), true),
            ("show", show.clone(), true),
            // Each command's schema refuses another command's payload.
            ("add", edit.clone(), false),
            ("edit", add.clone(), false),
            ("show", edit.clone(), false),
            ("show", add.clone(), false),
        ];
        let mut v = add.clone();
        v["existing"] = json!("none");
        cases.push(("add", v, false));
        let mut v = add.clone();
        v.as_object_mut().unwrap().remove("added");
        cases.push(("add", v, false));
        let mut v = add.clone();
        v["dry_run"] = json!(true);
        cases.push(("add", v, false));
        let mut v = add.clone();
        v["added"][0].as_object_mut().unwrap().remove("byte_size");
        cases.push(("add", v, false));
        let mut v = add.clone();
        v["added"][0]["editable"] = json!("yes");
        cases.push(("add", v, false));
        let mut v = add.clone();
        v["added"][0]["surprise"] = json!(1);
        cases.push(("add", v, false));
        let mut v = edit.clone();
        v["resource"]["modified"] = json!(5);
        cases.push(("edit", v, false));
        let mut v = show.clone();
        v["resource"]
            .as_object_mut()
            .unwrap()
            .remove("catalog_record");
        cases.push(("show", v, false));
        let mut v = show.clone();
        v["resource"]["catalog_record"]["attributed_to"]
            .as_object_mut()
            .unwrap()
            .remove("name");
        cases.push(("show", v, false));
        let mut v = show.clone();
        v["resource"]["relations"] = json!("none");
        cases.push(("show", v, false));
        let mut v = show.clone();
        v["schema_version"] = json!(2);
        cases.push(("show", v, false));

        for (name, instance, valid) in cases {
            let errs = problems(&validator(name), &instance);
            assert_eq!(
                errs.is_empty(),
                valid,
                "`hali schema {name}` should {} {instance}; errors: {errs:?}",
                if valid { "accept" } else { "reject" }
            );
        }
    }

    /// #10: the new error rules have conditional branches in `error.json`.
    #[test]
    fn new_error_rules_have_branches() {
        let mut cases: Vec<(Value, bool)> = Vec::new();
        for kind in ["missing", "directory", "other"] {
            cases.push((
                error("NOT_A_REGULAR_FILE", json!({"path": "/a/b", "kind": kind})),
                true,
            ));
        }
        cases.push((
            error(
                "NOT_A_REGULAR_FILE",
                json!({"path": "/a/b", "kind": "socket"}),
            ),
            false,
        ));
        cases.push((error("NOT_A_REGULAR_FILE", json!({"path": "/a/b"})), false));
        cases.push((
            error("NOT_A_REGULAR_FILE", json!({"kind": "missing"})),
            false,
        ));
        cases.push((error("NON_UTF8_PATH", json!({"path": "/a/\u{fffd}"})), true));
        cases.push((error("NON_UTF8_PATH", json!({})), false));
        cases.push((error("NON_UTF8_PATH", json!({"path": 5})), false));
        cases.push((error("UNKNOWN_RESOURCE", json!({"ref": "0192f0c4"})), true));
        cases.push((error("UNKNOWN_RESOURCE", json!({})), false));
        cases.push((error("UNKNOWN_RESOURCE", json!({"ref": 5})), false));
        cases.push((error("FILE_UNREADABLE", json!({"path": "/a/b"})), true));
        cases.push((error("FILE_UNREADABLE", json!({})), false));
        cases.push((error("FILE_UNREADABLE", json!({"path": null})), false));
        let v = validator("error");
        for (instance, valid) in cases {
            let errs = problems(&v, &instance);
            assert_eq!(
                errs.is_empty(),
                valid,
                "`hali schema error` should {} {instance}; errors: {errs:?}",
                if valid { "accept" } else { "reject" }
            );
        }
    }

    /// #12: what the `find` schema accepts and refuses.
    #[test]
    fn find_schema_accepts_and_rejects() {
        let page = |results: Vec<Value>| {
            let total = results.len();
            json!({"schema_version": 1, "results": results, "total": total,
                "truncated": false, "next_offset": null})
        };
        let full = page(vec![record(), record()]);
        let narrowed = page(vec![
            json!({"title": "notes"}),
            json!({"id": "a", "modified": "2026-10-20T14:02:11Z"}),
        ]);
        let empty = page(vec![]);
        let mut truncated = page(vec![record()]);
        truncated["total"] = json!(5);
        truncated["truncated"] = json!(true);
        truncated["next_offset"] = json!(1);

        let mut cases: Vec<(Value, bool)> = vec![
            (full.clone(), true),
            (narrowed.clone(), true),
            (empty.clone(), true),
            (truncated, true),
        ];
        // `--fields` can narrow to any one field of the record.
        for (name, value) in record().as_object().unwrap() {
            cases.push((page(vec![json!({ name: value })]), true));
        }
        // A key outside the record, `content` above all, is refused, narrowed or not.
        cases.push((page(vec![json!({"title": "notes", "content": "x"})]), false));
        cases.push((page(vec![json!({"content": "x"})]), false));
        let mut v = full.clone();
        v["results"][0]["content"] = json!("hello");
        cases.push((v, false));
        let mut v = full.clone();
        v["results"][0]["surprise"] = json!(1);
        cases.push((v, false));
        // A narrowed record has at least one field.
        cases.push((page(vec![json!({})]), false));
        // A field keeps its type, narrowed or not.
        cases.push((page(vec![json!({"editable": "yes"})]), false));
        cases.push((page(vec![json!({"byte_size": -1})]), false));
        let mut v = full.clone();
        v["results"][0]["stale"] = json!("no");
        cases.push((v, false));
        // The envelope.
        for key in [
            "schema_version",
            "results",
            "total",
            "truncated",
            "next_offset",
        ] {
            let mut v = full.clone();
            v.as_object_mut().unwrap().remove(key);
            cases.push((v, false));
        }
        let mut v = full.clone();
        v["schema_version"] = json!(2);
        cases.push((v, false));
        let mut v = full.clone();
        v["extra"] = json!(1);
        cases.push((v, false));
        let mut v = full.clone();
        v["dry_run"] = json!(true);
        cases.push((v, false));
        let mut v = full.clone();
        v["next_offset"] = json!("1");
        cases.push((v, false));
        // Another command's payload.
        cases.push((json!({"schema_version": 1, "resource": record()}), false));

        let v = validator("find");
        for (instance, valid) in cases {
            let errs = problems(&v, &instance);
            assert_eq!(
                errs.is_empty(),
                valid,
                "`hali schema find` should {} {instance}; errors: {errs:?}",
                if valid { "accept" } else { "reject" }
            );
        }
    }

    /// #14: what the `forget` schema accepts and refuses, and the `CONFIRMATION_REQUIRED`
    /// branch of `error`.
    #[test]
    fn forget_schema_and_confirmation_branch() {
        let real = json!({"schema_version": 1, "resource": record()});
        let mut dry = real.clone();
        dry["dry_run"] = json!(true);
        dry["changes"] = json!([{"op": "remove", "triple": "<a:b> <a:c> <a:d> ."}]);
        let mut cases: Vec<(&str, Value, bool)> = vec![
            ("forget", real.clone(), true),
            ("forget", dry.clone(), true),
            // Another command's payload.
            (
                "forget",
                json!({"schema_version": 1, "added": [record()], "existing": []}),
                false,
            ),
            ("edit", real.clone(), true),
        ];
        let mut v = real.clone();
        v.as_object_mut().unwrap().remove("resource");
        cases.push(("forget", v, false));
        let mut v = real.clone();
        v["resource"]["missing"] = json!("no");
        cases.push(("forget", v, false));
        let mut v = real.clone();
        v["resource"]["surprise"] = json!(1);
        cases.push(("forget", v, false));
        let mut v = real.clone();
        v["schema_version"] = json!(2);
        cases.push(("forget", v, false));
        let mut v = real.clone();
        v["dry_run"] = json!(true);
        cases.push(("forget", v, false));
        let mut v = real.clone();
        v["changes"] = json!([]);
        cases.push(("forget", v, false));
        let mut v = dry.clone();
        v["dry_run"] = json!(false);
        cases.push(("forget", v, false));
        let mut v = dry.clone();
        v.as_object_mut().unwrap().remove("changes");
        cases.push(("forget", v, false));
        let mut v = dry.clone();
        v["changes"] = json!([{"op": "replace", "triple": "<a:b> <a:c> <a:d> ."}]);
        cases.push(("forget", v, false));
        let mut v = real.clone();
        v["extra"] = json!(1);
        cases.push(("forget", v, false));

        for (name, instance, valid) in cases {
            let errs = problems(&validator(name), &instance);
            assert_eq!(
                errs.is_empty(),
                valid,
                "`hali schema {name}` should {} {instance}; errors: {errs:?}",
                if valid { "accept" } else { "reject" }
            );
        }

        // The error branch: `command` (string) and `declined` (boolean) are required.
        let confirmation = |details: Value| error("CONFIRMATION_REQUIRED", details);
        let v = validator("error");
        for (instance, valid) in [
            (
                confirmation(json!({"command": "forget", "declined": false})),
                true,
            ),
            (
                confirmation(json!({"command": "forget", "declined": true})),
                true,
            ),
            (confirmation(json!({"command": "forget"})), false),
            (confirmation(json!({"declined": false})), false),
            (confirmation(json!({})), false),
            (
                confirmation(json!({"command": "forget", "declined": "no"})),
                false,
            ),
            (
                confirmation(json!({"command": 5, "declined": false})),
                false,
            ),
        ] {
            let errs = problems(&v, &instance);
            assert_eq!(
                errs.is_empty(),
                valid,
                "`hali schema error` should {} {instance}; errors: {errs:?}",
                if valid { "accept" } else { "reject" }
            );
        }
    }

    /// #17: what the `concept show`, `concept rename`, `concept edit` and `concept remove`
    /// schemas accept and refuse.
    #[test]
    fn concept_manage_schemas_accept_and_reject() {
        let shown_concept = || {
            json!({"id": "dog", "label": "dog", "alt_labels": ["hound"],
                "broader": ["mammal", "pet"], "narrower": [], "tagged": 1})
        };
        let real = json!({"schema_version": 1, "concept": concept()});
        let mut dry = real.clone();
        dry["dry_run"] = json!(true);
        dry["changes"] = json!([{"op": "remove", "triple": "<a:b> <a:c> <a:d> ."}]);
        let show = json!({"schema_version": 1, "concept": shown_concept()});

        let mut cases: Vec<(&str, Value, bool)> = Vec::new();
        for name in ["concept rename", "concept edit", "concept remove"] {
            cases.push((name, real.clone(), true));
            cases.push((name, dry.clone(), true));
            let mut v = dry.clone();
            v["changes"] = json!([]);
            cases.push((name, v, true));
            // Another command's payload: show's has the derived fields; add's list has none.
            cases.push((name, show.clone(), false));
            cases.push((
                name,
                json!({"schema_version": 1, "results": [], "total": 0,
                    "truncated": false, "next_offset": null}),
                false,
            ));
            // A missing or an extra key, in the payload and in the record.
            let mut v = real.clone();
            v.as_object_mut().unwrap().remove("concept");
            cases.push((name, v, false));
            let mut v = real.clone();
            v["concept"].as_object_mut().unwrap().remove("broader");
            cases.push((name, v, false));
            let mut v = real.clone();
            v["concept"]["surprise"] = json!(1);
            cases.push((name, v, false));
            let mut v = real.clone();
            v["extra"] = json!(1);
            cases.push((name, v, false));
            let mut v = real.clone();
            v["concept"]["alt_labels"] = json!("none");
            cases.push((name, v, false));
            // The version, and the dry-run keys.
            let mut v = real.clone();
            v["schema_version"] = json!(2);
            cases.push((name, v, false));
            let mut v = real.clone();
            v["dry_run"] = json!(true);
            cases.push((name, v, false));
            let mut v = real.clone();
            v["changes"] = json!([]);
            cases.push((name, v, false));
            let mut v = dry.clone();
            v["dry_run"] = json!(false);
            cases.push((name, v, false));
            let mut v = dry.clone();
            v.as_object_mut().unwrap().remove("changes");
            cases.push((name, v, false));
            let mut v = dry.clone();
            v["changes"] = json!([{"op": "replace", "triple": "<a:b> <a:c> <a:d> ."}]);
            cases.push((name, v, false));
        }

        cases.push(("concept show", show.clone(), true));
        // Another command's payload, and a dry run: show writes nothing.
        cases.push(("concept show", real.clone(), false));
        let mut v = show.clone();
        v["dry_run"] = json!(true);
        v["changes"] = json!([]);
        cases.push(("concept show", v, false));
        let mut v = show.clone();
        v["dry_run"] = json!(false);
        cases.push(("concept show", v, false));
        // Missing keys, one at a time.
        for key in ["id", "label", "alt_labels", "broader", "narrower", "tagged"] {
            let mut v = show.clone();
            v["concept"].as_object_mut().unwrap().remove(key);
            cases.push(("concept show", v, false));
        }
        let mut v = show.clone();
        v.as_object_mut().unwrap().remove("concept");
        cases.push(("concept show", v, false));
        // Extra keys.
        let mut v = show.clone();
        v["concept"]["surprise"] = json!(1);
        cases.push(("concept show", v, false));
        let mut v = show.clone();
        v["extra"] = json!(1);
        cases.push(("concept show", v, false));
        // Types: `narrower` is an array of strings, `tagged` an integer of at least 0.
        for tagged in [json!("1"), json!(1.5), json!(-1), json!(null), json!(true)] {
            let mut v = show.clone();
            v["concept"]["tagged"] = tagged;
            cases.push(("concept show", v, false));
        }
        let mut v = show.clone();
        v["concept"]["tagged"] = json!(0);
        cases.push(("concept show", v, true));
        let mut v = show.clone();
        v["concept"]["narrower"] = json!(["cat", "dog"]);
        cases.push(("concept show", v, true));
        let mut v = show.clone();
        v["concept"]["narrower"] = json!("cat");
        cases.push(("concept show", v, false));
        let mut v = show.clone();
        v["concept"]["narrower"] = json!([1]);
        cases.push(("concept show", v, false));
        let mut v = show.clone();
        v["concept"]["alt_labels"] = json!([1]);
        cases.push(("concept show", v, false));
        let mut v = show.clone();
        v["schema_version"] = json!(2);
        cases.push(("concept show", v, false));

        for (name, instance, valid) in cases {
            let errs = problems(&validator(name), &instance);
            assert_eq!(
                errs.is_empty(),
                valid,
                "`hali schema {name}` should {} {instance}; errors: {errs:?}",
                if valid { "accept" } else { "reject" }
            );
        }
    }

    /// #17: the `IN_USE` branch of `error` requires the Concept and both counts.
    #[test]
    fn in_use_branch() {
        let in_use = |details: Value| error("IN_USE", details);
        let v = validator("error");
        for (instance, valid) in [
            (
                in_use(json!({"concept": "fish", "tagged": 1, "narrower": 0})),
                true,
            ),
            (
                in_use(json!({"concept": "mammal", "tagged": 0, "narrower": 2})),
                true,
            ),
            (in_use(json!({"tagged": 1, "narrower": 0})), false),
            (in_use(json!({"concept": "fish", "narrower": 0})), false),
            (in_use(json!({"concept": "fish", "tagged": 1})), false),
            (in_use(json!({})), false),
            (
                in_use(json!({"concept": 5, "tagged": 1, "narrower": 0})),
                false,
            ),
            (
                in_use(json!({"concept": "fish", "tagged": -1, "narrower": 0})),
                false,
            ),
            (
                in_use(json!({"concept": "fish", "tagged": 1.5, "narrower": 0})),
                false,
            ),
            (
                in_use(json!({"concept": "fish", "tagged": "1", "narrower": 0})),
                false,
            ),
            (
                in_use(json!({"concept": "fish", "tagged": 1, "narrower": -2})),
                false,
            ),
            (
                in_use(json!({"concept": "fish", "tagged": 1, "narrower": null})),
                false,
            ),
        ] {
            let errs = problems(&v, &instance);
            assert_eq!(
                errs.is_empty(),
                valid,
                "`hali schema error` should {} {instance}; errors: {errs:?}",
                if valid { "accept" } else { "reject" }
            );
        }
    }

    /// #15: what the `type add`, `type remove` and `type list` schemas accept and refuse.
    #[test]
    fn type_schemas_accept_and_reject() {
        let record = || json!({"id": "Report", "label": "Report", "parents": ["Document"]});
        let real = json!({"schema_version": 1, "type": record()});
        let mut dry = real.clone();
        dry["dry_run"] = json!(true);
        dry["changes"] = json!([{"op": "add", "triple": "<a:b> <a:c> <a:d> ."}]);
        let page = json!({"schema_version": 1, "results": [record()], "total": 1,
            "truncated": false, "next_offset": null});

        let mut cases: Vec<(&str, Value, bool)> = Vec::new();
        for name in ["type add", "type remove"] {
            cases.push((name, real.clone(), true));
            cases.push((name, dry.clone(), true));
            let mut v = dry.clone();
            v["changes"] = json!([]);
            cases.push((name, v, true));
            let mut v = real.clone();
            v["type"]["parents"] = json!([]);
            cases.push((name, v, true));
            // Another command's payload.
            cases.push((name, page.clone(), false));
            cases.push((
                name,
                json!({"schema_version": 1, "concept": {"id": "ml", "label": "ml",
                    "alt_labels": [], "broader": []}}),
                false,
            ));
            // A missing or an extra key, in the payload and in the record.
            let mut v = real.clone();
            v.as_object_mut().unwrap().remove("type");
            cases.push((name, v, false));
            for key in ["id", "label", "parents"] {
                let mut v = real.clone();
                v["type"].as_object_mut().unwrap().remove(key);
                cases.push((name, v, false));
            }
            let mut v = real.clone();
            v["type"]["surprise"] = json!(1);
            cases.push((name, v, false));
            let mut v = real.clone();
            v["extra"] = json!(1);
            cases.push((name, v, false));
            // Types of the values: `parents` is an array of strings.
            let mut v = real.clone();
            v["type"]["parents"] = json!("Document");
            cases.push((name, v, false));
            let mut v = real.clone();
            v["type"]["parents"] = json!([1]);
            cases.push((name, v, false));
            let mut v = real.clone();
            v["type"]["label"] = json!(null);
            cases.push((name, v, false));
            // The version, and the dry-run keys.
            let mut v = real.clone();
            v["schema_version"] = json!(2);
            cases.push((name, v, false));
            let mut v = real.clone();
            v["dry_run"] = json!(true);
            cases.push((name, v, false));
            let mut v = real.clone();
            v["changes"] = json!([]);
            cases.push((name, v, false));
            let mut v = dry.clone();
            v["dry_run"] = json!(false);
            cases.push((name, v, false));
            let mut v = dry.clone();
            v.as_object_mut().unwrap().remove("changes");
            cases.push((name, v, false));
            let mut v = dry.clone();
            v["changes"] = json!([{"op": "replace", "triple": "<a:b> <a:c> <a:d> ."}]);
            cases.push((name, v, false));
        }

        // `type list` is read-only: the list envelope, and no dry-run keys.
        cases.push(("type list", page.clone(), true));
        let mut v = page.clone();
        v["results"] = json!([]);
        v["total"] = json!(0);
        cases.push(("type list", v, true));
        let mut v = page.clone();
        v["truncated"] = json!(true);
        v["next_offset"] = json!(1);
        cases.push(("type list", v, true));
        cases.push(("type list", real.clone(), false));
        for key in ["results", "total", "truncated", "next_offset"] {
            let mut v = page.clone();
            v.as_object_mut().unwrap().remove(key);
            cases.push(("type list", v, false));
        }
        let mut v = page.clone();
        v["results"][0].as_object_mut().unwrap().remove("parents");
        cases.push(("type list", v, false));
        let mut v = page.clone();
        v["results"][0]["surprise"] = json!(1);
        cases.push(("type list", v, false));
        let mut v = page.clone();
        v["results"] = json!([{"id": "ml", "label": "ml", "alt_labels": [], "broader": []}]);
        cases.push(("type list", v, false));
        let mut v = page.clone();
        v["dry_run"] = json!(true);
        v["changes"] = json!([]);
        cases.push(("type list", v, false));
        let mut v = page.clone();
        v["total"] = json!(-1);
        cases.push(("type list", v, false));
        let mut v = page.clone();
        v["schema_version"] = json!(2);
        cases.push(("type list", v, false));

        for (name, instance, valid) in cases {
            let errs = problems(&validator(name), &instance);
            assert_eq!(
                errs.is_empty(),
                valid,
                "`hali schema {name}` should {} {instance}; errors: {errs:?}",
                if valid { "accept" } else { "reject" }
            );
        }
    }

    /// #15: the `UNKNOWN_TYPE` branch of `error`.
    #[test]
    fn unknown_type_branch() {
        let unknown = |details: Value| error("UNKNOWN_TYPE", details);
        let suggestion = |id: &str| json!({"id": id, "label": id});
        let v = validator("error");
        for (instance, valid) in [
            (unknown(json!({"type": "Papr", "suggestions": []})), true),
            (
                unknown(json!({"type": "Papr", "suggestions": [suggestion("Paper")]})),
                true,
            ),
            (
                unknown(json!({"type": "e", "suggestions": [
                    suggestion("Memo"), suggestion("Note"), suggestion("Paper")]})),
                true,
            ),
            (
                unknown(json!({"type": "e", "suggestions": [
                    suggestion("A"), suggestion("B"), suggestion("C"), suggestion("D")]})),
                false,
            ),
            (unknown(json!({"suggestions": []})), false),
            (unknown(json!({"type": "Papr"})), false),
            (unknown(json!({})), false),
            (unknown(json!({"type": 5, "suggestions": []})), false),
            (
                unknown(json!({"type": "Papr", "suggestions": [{"id": "Paper"}]})),
                false,
            ),
            (
                unknown(json!({"type": "Papr", "suggestions": [
                    {"id": "Paper", "label": "Paper", "extra": 1}]})),
                false,
            ),
            // The Concept key does not satisfy the Type rule.
            (
                unknown(json!({"concept": "Papr", "suggestions": []})),
                false,
            ),
        ] {
            let errs = problems(&v, &instance);
            assert_eq!(
                errs.is_empty(),
                valid,
                "`hali schema error` should {} {instance}; errors: {errs:?}",
                if valid { "accept" } else { "reject" }
            );
        }
    }

    /// #15: `IN_USE` is either the Concept shape or the Type shape, each with every key.
    #[test]
    fn in_use_type_branch() {
        let in_use = |details: Value| error("IN_USE", details);
        let v = validator("error");
        for (instance, valid) in [
            (
                in_use(json!({"type": "Paper", "typed": 1, "subtypes": 0})),
                true,
            ),
            (
                in_use(json!({"type": "Document", "typed": 0, "subtypes": 3})),
                true,
            ),
            // The Concept shape still holds.
            (
                in_use(json!({"concept": "fish", "tagged": 1, "narrower": 0})),
                true,
            ),
            // Partial and mixed shapes.
            (in_use(json!({"typed": 1, "subtypes": 0})), false),
            (in_use(json!({"type": "Paper", "subtypes": 0})), false),
            (in_use(json!({"type": "Paper", "typed": 1})), false),
            (
                in_use(json!({"type": "Paper", "tagged": 1, "narrower": 0})),
                false,
            ),
            (
                in_use(json!({"concept": "fish", "typed": 1, "subtypes": 0})),
                false,
            ),
            (in_use(json!({"type": "Paper"})), false),
            (in_use(json!({})), false),
            // Types of the values.
            (in_use(json!({"type": 5, "typed": 1, "subtypes": 0})), false),
            (
                in_use(json!({"type": "Paper", "typed": -1, "subtypes": 0})),
                false,
            ),
            (
                in_use(json!({"type": "Paper", "typed": 1.5, "subtypes": 0})),
                false,
            ),
            (
                in_use(json!({"type": "Paper", "typed": 1, "subtypes": null})),
                false,
            ),
            (
                in_use(json!({"type": "Paper", "typed": "1", "subtypes": 0})),
                false,
            ),
        ] {
            let errs = problems(&v, &instance);
            assert_eq!(
                errs.is_empty(),
                valid,
                "`hali schema error` should {} {instance}; errors: {errs:?}",
                if valid { "accept" } else { "reject" }
            );
        }
    }

    /// #15: `LABEL_TAKEN` is either the Concept shape or the Type shape, each with every key.
    #[test]
    fn label_taken_type_branch() {
        let taken = |details: Value| error("LABEL_TAKEN", details);
        let v = validator("error");
        for (instance, valid) in [
            (
                taken(json!({"label": "paper", "language": "en", "type": "Paper"})),
                true,
            ),
            // The Concept shape still holds.
            (
                taken(json!({"label": "ml", "language": "en", "concept": "ml"})),
                true,
            ),
            (taken(json!({"label": "paper", "language": "en"})), false),
            (taken(json!({"language": "en", "type": "Paper"})), false),
            (taken(json!({"label": "paper", "type": "Paper"})), false),
            (
                taken(json!({"label": "paper", "language": "en", "type": 5})),
                false,
            ),
            (
                taken(json!({"label": 1, "language": "en", "type": "Paper"})),
                false,
            ),
            (taken(json!({})), false),
        ] {
            let errs = problems(&v, &instance);
            assert_eq!(
                errs.is_empty(),
                valid,
                "`hali schema error` should {} {instance}; errors: {errs:?}",
                if valid { "accept" } else { "reject" }
            );
        }
    }

    /// #19: what the `describe` schema accepts and refuses: a full and a brief branch, every
    /// key required, nothing else allowed.
    #[cfg(feature = "goal")]
    #[test]
    fn describe_schema_accepts_and_rejects() {
        let counts = || json!({"resources": 1, "concepts": 1, "types": 1, "relation_kinds": 1, "relations": 0});
        let brief = || {
            json!({"schema_version": 1, "core_version": "0.1.0", "store": "/data/hali",
                "counts": counts()})
        };
        let full = || {
            let mut v = brief();
            v["concepts"] = json!([{"id": "ml", "label": "ml", "alt_labels": [], "broader": []}]);
            v["types"] = json!([{"id": "Note", "label": "Note", "parents": []}]);
            v["relation_kinds"] = json!([{"id": "dcterms:relation", "label": "relation",
                "parents": [], "standard": true}]);
            v
        };
        let with = |base: Value, key: &str, value: Value| {
            let mut v = base;
            v[key] = value;
            v
        };
        let without = |base: Value, key: &str| {
            let mut v = base;
            v.as_object_mut().unwrap().remove(key);
            v
        };
        let v = validator("describe");
        let mut cases: Vec<(String, Value, bool)> = vec![
            ("full".into(), full(), true),
            ("brief".into(), brief(), true),
            (
                "empty lists".into(),
                with(
                    with(with(full(), "concepts", json!([])), "types", json!([])),
                    "relation_kinds",
                    json!([]),
                ),
                true,
            ),
            // Partial lists: one or two of the three, in either direction.
            ("full minus types".into(), without(full(), "types"), false),
            (
                "full minus concepts".into(),
                without(full(), "concepts"),
                false,
            ),
            (
                "full minus relation_kinds".into(),
                without(full(), "relation_kinds"),
                false,
            ),
            (
                "brief plus types".into(),
                with(brief(), "types", json!([])),
                false,
            ),
            (
                "brief plus concepts".into(),
                with(brief(), "concepts", json!([])),
                false,
            ),
            // Missing or extra keys, wrong types.
            (
                "no schema_version".into(),
                without(brief(), "schema_version"),
                false,
            ),
            (
                "version 2".into(),
                with(brief(), "schema_version", json!(2)),
                false,
            ),
            (
                "no core_version".into(),
                without(brief(), "core_version"),
                false,
            ),
            ("no store".into(), without(brief(), "store"), false),
            ("no counts".into(), without(brief(), "counts"), false),
            (
                "store null".into(),
                with(brief(), "store", json!(null)),
                false,
            ),
            (
                "extra key".into(),
                with(brief(), "exists", json!(true)),
                false,
            ),
            (
                "extra key in full".into(),
                with(full(), "skill", json!({})),
                false,
            ),
            (
                "paging key".into(),
                with(full(), "truncated", json!(false)),
                false,
            ),
            (
                "dry run".into(),
                with(brief(), "dry_run", json!(true)),
                false,
            ),
        ];
        for key in [
            "resources",
            "concepts",
            "types",
            "relation_kinds",
            "relations",
        ] {
            let mut c = counts();
            c.as_object_mut().unwrap().remove(key);
            cases.push((
                format!("counts minus {key}"),
                with(brief(), "counts", c),
                false,
            ));
            let mut c = counts();
            c[key] = json!(-1);
            cases.push((
                format!("counts {key} -1"),
                with(brief(), "counts", c),
                false,
            ));
            let mut c = counts();
            c[key] = json!("1");
            cases.push((
                format!("counts {key} string"),
                with(brief(), "counts", c),
                false,
            ));
        }
        let mut c = counts();
        c["exists"] = json!(true);
        cases.push(("counts extra key".into(), with(brief(), "counts", c), false));
        // The records are the existing shapes, strictly.
        let mut f = full();
        f["concepts"][0]["extra"] = json!(1);
        cases.push(("concept extra key".into(), f, false));
        let mut f = full();
        f["types"][0].as_object_mut().unwrap().remove("parents");
        cases.push(("type minus parents".into(), f, false));
        let mut f = full();
        f["relation_kinds"][0]
            .as_object_mut()
            .unwrap()
            .remove("standard");
        cases.push(("kind minus standard".into(), f, false));
        // Another command's payload.
        cases.push((
            "relation list".into(),
            json!({"schema_version": 1, "results": [], "total": 0, "truncated": false,
                "next_offset": null}),
            false,
        ));
        for (name, instance, valid) in cases {
            let errs = problems(&v, &instance);
            assert_eq!(
                errs.is_empty(),
                valid,
                "`hali schema describe` should {} `{name}`: {instance}; errors: {errs:?}",
                if valid { "accept" } else { "reject" }
            );
        }
    }

    /// #18: what the `relate`, `unrelate` and `relation add|list|remove` schemas accept and
    /// refuse.
    #[test]
    fn relation_schemas_accept_and_reject() {
        let change = json!({"op": "add", "triple": "<a:b> <a:c> <a:d> ."});
        let link = || json!({"from": "a", "relation": "annotates", "to": "b"});
        let linked = json!({"schema_version": 1, "relation": link()});
        let mut linked_dry = linked.clone();
        linked_dry["dry_run"] = json!(true);
        linked_dry["changes"] = json!([change]);
        let kind = || {
            json!({"id": "cites", "label": "cites", "parents": ["dcterms:references"],
                "standard": false})
        };
        let real = json!({"schema_version": 1, "relation_kind": kind()});
        let mut dry = real.clone();
        dry["dry_run"] = json!(true);
        dry["changes"] = json!([change]);
        let page = json!({"schema_version": 1, "results": [kind()], "total": 1,
            "truncated": false, "next_offset": null});

        let mut cases: Vec<(&str, Value, bool)> = Vec::new();
        for name in ["relate", "unrelate"] {
            cases.push((name, linked.clone(), true));
            cases.push((name, linked_dry.clone(), true));
            let mut v = linked_dry.clone();
            v["changes"] = json!([]);
            cases.push((name, v, true));
            // Another command's payload.
            cases.push((name, real.clone(), false));
            cases.push((name, page.clone(), false));
            cases.push((
                name,
                json!({"schema_version": 1, "resource": {"id": "a"}}),
                false,
            ));
            // A missing or an extra key, in the payload and in the link.
            let mut v = linked.clone();
            v.as_object_mut().unwrap().remove("relation");
            cases.push((name, v, false));
            for key in ["from", "relation", "to"] {
                let mut v = linked.clone();
                v["relation"].as_object_mut().unwrap().remove(key);
                cases.push((name, v, false));
                let mut v = linked.clone();
                v["relation"][key] = json!(5);
                cases.push((name, v, false));
            }
            let mut v = linked.clone();
            v["relation"]["surprise"] = json!(1);
            cases.push((name, v, false));
            let mut v = linked.clone();
            v["extra"] = json!(1);
            cases.push((name, v, false));
            // The version, and the dry-run keys.
            let mut v = linked.clone();
            v["schema_version"] = json!(2);
            cases.push((name, v, false));
            let mut v = linked.clone();
            v["dry_run"] = json!(true);
            cases.push((name, v, false));
            let mut v = linked.clone();
            v["changes"] = json!([]);
            cases.push((name, v, false));
            let mut v = linked_dry.clone();
            v["dry_run"] = json!(false);
            cases.push((name, v, false));
            let mut v = linked_dry.clone();
            v.as_object_mut().unwrap().remove("changes");
            cases.push((name, v, false));
            let mut v = linked_dry.clone();
            v["changes"] = json!([{"op": "replace", "triple": "<a:b> <a:c> <a:d> ."}]);
            cases.push((name, v, false));
        }

        for name in ["relation add", "relation remove"] {
            cases.push((name, real.clone(), true));
            cases.push((name, dry.clone(), true));
            let mut v = dry.clone();
            v["changes"] = json!([]);
            cases.push((name, v, true));
            let mut v = real.clone();
            v["relation_kind"]["parents"] = json!([]);
            v["relation_kind"]["standard"] = json!(true);
            cases.push((name, v, true));
            // Another command's payload.
            cases.push((name, linked.clone(), false));
            cases.push((name, page.clone(), false));
            cases.push((
                name,
                json!({"schema_version": 1, "type": {"id": "Report", "label": "Report",
                    "parents": []}}),
                false,
            ));
            // A missing or an extra key, in the payload and in the record.
            let mut v = real.clone();
            v.as_object_mut().unwrap().remove("relation_kind");
            cases.push((name, v, false));
            for key in ["id", "label", "parents", "standard"] {
                let mut v = real.clone();
                v["relation_kind"].as_object_mut().unwrap().remove(key);
                cases.push((name, v, false));
            }
            let mut v = real.clone();
            v["relation_kind"]["surprise"] = json!(1);
            cases.push((name, v, false));
            let mut v = real.clone();
            v["extra"] = json!(1);
            cases.push((name, v, false));
            // Types of the values.
            let mut v = real.clone();
            v["relation_kind"]["parents"] = json!("dcterms:relation");
            cases.push((name, v, false));
            let mut v = real.clone();
            v["relation_kind"]["parents"] = json!([1]);
            cases.push((name, v, false));
            let mut v = real.clone();
            v["relation_kind"]["label"] = json!(null);
            cases.push((name, v, false));
            let mut v = real.clone();
            v["relation_kind"]["standard"] = json!("no");
            cases.push((name, v, false));
            // The version, and the dry-run keys.
            let mut v = real.clone();
            v["schema_version"] = json!(2);
            cases.push((name, v, false));
            let mut v = real.clone();
            v["dry_run"] = json!(true);
            cases.push((name, v, false));
            let mut v = real.clone();
            v["changes"] = json!([]);
            cases.push((name, v, false));
            let mut v = dry.clone();
            v["dry_run"] = json!(false);
            cases.push((name, v, false));
            let mut v = dry.clone();
            v.as_object_mut().unwrap().remove("changes");
            cases.push((name, v, false));
        }

        // `relation list` is read-only: the list envelope, and no dry-run keys.
        cases.push(("relation list", page.clone(), true));
        let mut v = page.clone();
        v["results"] = json!([]);
        v["total"] = json!(0);
        cases.push(("relation list", v, true));
        let mut v = page.clone();
        v["truncated"] = json!(true);
        v["next_offset"] = json!(1);
        cases.push(("relation list", v, true));
        cases.push(("relation list", real.clone(), false));
        for key in ["results", "total", "truncated", "next_offset"] {
            let mut v = page.clone();
            v.as_object_mut().unwrap().remove(key);
            cases.push(("relation list", v, false));
        }
        for key in ["id", "label", "parents", "standard"] {
            let mut v = page.clone();
            v["results"][0].as_object_mut().unwrap().remove(key);
            cases.push(("relation list", v, false));
        }
        let mut v = page.clone();
        v["results"][0]["surprise"] = json!(1);
        cases.push(("relation list", v, false));
        let mut v = page.clone();
        v["results"] = json!([{"id": "Report", "label": "Report", "parents": []}]);
        cases.push(("relation list", v, false));
        let mut v = page.clone();
        v["dry_run"] = json!(true);
        v["changes"] = json!([]);
        cases.push(("relation list", v, false));
        let mut v = page.clone();
        v["total"] = json!(-1);
        cases.push(("relation list", v, false));
        let mut v = page.clone();
        v["schema_version"] = json!(2);
        cases.push(("relation list", v, false));

        for (name, instance, valid) in cases {
            let errs = problems(&validator(name), &instance);
            assert_eq!(
                errs.is_empty(),
                valid,
                "`hali schema {name}` should {} {instance}; errors: {errs:?}",
                if valid { "accept" } else { "reject" }
            );
        }
    }

    /// #18: the `relations` of `show` are `{direction, relation, resource}` entries, strictly.
    #[test]
    fn show_relations_items_strict() {
        let entry = |direction: &str| json!({"direction": direction, "relation": "dcterms:isPartOf", "resource": record()});
        let with = |relations: Value| {
            let mut r = shown();
            r["relations"] = relations;
            json!({"schema_version": 1, "resource": r})
        };
        let mut cases: Vec<(Value, bool)> = vec![
            (with(json!([])), true),
            (with(json!([entry("out")])), true),
            (with(json!([entry("out"), entry("in")])), true),
            (with(json!([entry("sideways")])), false),
            (with(json!([entry("OUT")])), false),
            (with(json!("none")), false),
            (with(json!({})), false),
            (with(json!(["out"])), false),
            (with(json!([null])), false),
        ];
        for key in ["direction", "relation", "resource"] {
            let mut e = entry("in");
            e.as_object_mut().unwrap().remove(key);
            cases.push((with(json!([e])), false));
        }
        let mut e = entry("in");
        e["surprise"] = json!(1);
        cases.push((with(json!([e])), false));
        let mut e = entry("in");
        e["relation"] = json!(5);
        cases.push((with(json!([e])), false));
        let mut e = entry("in");
        e["direction"] = json!(null);
        cases.push((with(json!([e])), false));
        // The Resource is a full record: nothing missing, nothing extra, the right types.
        for key in [
            "id",
            "path",
            "title",
            "media_type",
            "editable",
            "missing",
            "stale",
            "types",
            "tags",
            "byte_size",
            "modified",
        ] {
            let mut e = entry("in");
            e["resource"].as_object_mut().unwrap().remove(key);
            cases.push((with(json!([e])), false));
        }
        let mut e = entry("in");
        e["resource"]["surprise"] = json!(1);
        cases.push((with(json!([e])), false));
        let mut e = entry("in");
        e["resource"]["editable"] = json!("yes");
        cases.push((with(json!([e])), false));
        let mut e = entry("in");
        e["resource"] = json!({"id": "a"});
        cases.push((with(json!([e])), false));

        let v = validator("show");
        for (instance, valid) in cases {
            let errs = problems(&v, &instance);
            assert_eq!(
                errs.is_empty(),
                valid,
                "`hali schema show` should {} {instance}; errors: {errs:?}",
                if valid { "accept" } else { "reject" }
            );
        }
    }

    /// #18: the `UNKNOWN_RELATION` branch of `error`.
    #[test]
    fn unknown_relation_branch() {
        let unknown = |details: Value| error("UNKNOWN_RELATION", details);
        let suggestion = |id: &str| json!({"id": id, "label": id});
        let v = validator("error");
        for (instance, valid) in [
            (
                unknown(json!({"relation": "annotate", "suggestions": []})),
                true,
            ),
            (
                unknown(json!({"relation": "annotate", "suggestions": [suggestion("annotates")]})),
                true,
            ),
            (
                unknown(json!({"relation": "e", "suggestions": [
                    suggestion("quotes"), suggestion("sketch"), suggestion("annotates")]})),
                true,
            ),
            (
                unknown(json!({"relation": "e", "suggestions": [
                    suggestion("A"), suggestion("B"), suggestion("C"), suggestion("D")]})),
                false,
            ),
            (unknown(json!({"suggestions": []})), false),
            (unknown(json!({"relation": "annotate"})), false),
            (unknown(json!({})), false),
            (unknown(json!({"relation": 5, "suggestions": []})), false),
            (
                unknown(json!({"relation": "annotate", "suggestions": [{"id": "annotates"}]})),
                false,
            ),
            (
                unknown(json!({"relation": "annotate", "suggestions": [
                    {"id": "annotates", "label": "annotates", "extra": 1}]})),
                false,
            ),
            // The Type and Concept keys do not satisfy the relation rule.
            (
                unknown(json!({"type": "annotate", "suggestions": []})),
                false,
            ),
            (
                unknown(json!({"concept": "annotate", "suggestions": []})),
                false,
            ),
        ] {
            let errs = problems(&v, &instance);
            assert_eq!(
                errs.is_empty(),
                valid,
                "`hali schema error` should {} {instance}; errors: {errs:?}",
                if valid { "accept" } else { "reject" }
            );
        }
    }

    /// #18: the `FORGET_BLOCKED_BY_RELATIONS` branch of `error`.
    #[test]
    fn forget_blocked_branch() {
        let blocked = |details: Value| error("FORGET_BLOCKED_BY_RELATIONS", details);
        let v = validator("error");
        for (instance, valid) in [
            (
                blocked(json!({"resource": "0192f0c4", "relations": 3, "from": 2})),
                true,
            ),
            (
                blocked(json!({"resource": "0192f0c4", "relations": 1, "from": 1})),
                true,
            ),
            (blocked(json!({"relations": 3, "from": 2})), false),
            (blocked(json!({"resource": "0192f0c4", "from": 2})), false),
            (
                blocked(json!({"resource": "0192f0c4", "relations": 3})),
                false,
            ),
            (blocked(json!({})), false),
            (
                blocked(json!({"resource": 5, "relations": 3, "from": 2})),
                false,
            ),
            (
                blocked(json!({"resource": "0192f0c4", "relations": -1, "from": 2})),
                false,
            ),
            (
                blocked(json!({"resource": "0192f0c4", "relations": 1.5, "from": 2})),
                false,
            ),
            (
                blocked(json!({"resource": "0192f0c4", "relations": "3", "from": 2})),
                false,
            ),
            (
                blocked(json!({"resource": "0192f0c4", "relations": 3, "from": null})),
                false,
            ),
        ] {
            let errs = problems(&v, &instance);
            assert_eq!(
                errs.is_empty(),
                valid,
                "`hali schema error` should {} {instance}; errors: {errs:?}",
                if valid { "accept" } else { "reject" }
            );
        }
    }

    /// #18: `IN_USE` is the Concept, the Type or the relation shape, each with every key.
    #[test]
    fn in_use_relation_branch() {
        let in_use = |details: Value| error("IN_USE", details);
        let v = validator("error");
        for (instance, valid) in [
            (
                in_use(json!({"relation": "annotates", "used": 1, "subproperties": 0})),
                true,
            ),
            (
                in_use(json!({"relation": "skos:related", "used": 0, "subproperties": 3})),
                true,
            ),
            // The Concept and Type shapes still hold.
            (
                in_use(json!({"concept": "fish", "tagged": 1, "narrower": 0})),
                true,
            ),
            (
                in_use(json!({"type": "Paper", "typed": 1, "subtypes": 0})),
                true,
            ),
            // Partial and mixed shapes.
            (in_use(json!({"used": 1, "subproperties": 0})), false),
            (
                in_use(json!({"relation": "annotates", "subproperties": 0})),
                false,
            ),
            (in_use(json!({"relation": "annotates", "used": 1})), false),
            (in_use(json!({"relation": "annotates"})), false),
            (
                in_use(json!({"relation": "annotates", "typed": 1, "subtypes": 0})),
                false,
            ),
            (
                in_use(json!({"relation": "annotates", "tagged": 1, "narrower": 0})),
                false,
            ),
            (
                in_use(json!({"concept": "fish", "used": 1, "subproperties": 0})),
                false,
            ),
            (
                in_use(json!({"type": "Paper", "used": 1, "subproperties": 0})),
                false,
            ),
            (in_use(json!({})), false),
            // Types of the values.
            (
                in_use(json!({"relation": 5, "used": 1, "subproperties": 0})),
                false,
            ),
            (
                in_use(json!({"relation": "annotates", "used": -1, "subproperties": 0})),
                false,
            ),
            (
                in_use(json!({"relation": "annotates", "used": 1.5, "subproperties": 0})),
                false,
            ),
            (
                in_use(json!({"relation": "annotates", "used": 1, "subproperties": null})),
                false,
            ),
            (
                in_use(json!({"relation": "annotates", "used": "1", "subproperties": 0})),
                false,
            ),
        ] {
            let errs = problems(&v, &instance);
            assert_eq!(
                errs.is_empty(),
                valid,
                "`hali schema error` should {} {instance}; errors: {errs:?}",
                if valid { "accept" } else { "reject" }
            );
        }
    }

    /// #18: `LABEL_TAKEN` is the Concept, the Type or the relation shape, each with every key.
    #[test]
    fn label_taken_relation_branch() {
        let taken = |details: Value| error("LABEL_TAKEN", details);
        let v = validator("error");
        for (instance, valid) in [
            (
                taken(json!({"label": "references", "language": "en",
                    "relation": "dcterms:references"})),
                true,
            ),
            // The Concept and Type shapes still hold.
            (
                taken(json!({"label": "ml", "language": "en", "concept": "ml"})),
                true,
            ),
            (
                taken(json!({"label": "paper", "language": "en", "type": "Paper"})),
                true,
            ),
            (taken(json!({"label": "x", "language": "en"})), false),
            (
                taken(json!({"language": "en", "relation": "dcterms:references"})),
                false,
            ),
            (
                taken(json!({"label": "x", "relation": "dcterms:references"})),
                false,
            ),
            (
                taken(json!({"label": "x", "language": "en", "relation": 5})),
                false,
            ),
            (
                taken(json!({"label": 1, "language": "en", "relation": "annotates"})),
                false,
            ),
            (taken(json!({})), false),
        ] {
            let errs = problems(&v, &instance);
            assert_eq!(
                errs.is_empty(),
                valid,
                "`hali schema error` should {} {instance}; errors: {errs:?}",
                if valid { "accept" } else { "reject" }
            );
        }
    }
}
