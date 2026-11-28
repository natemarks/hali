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

    /// Snapshots of text that is not `--json` output, so outside the JSON contract: the
    /// `--help` examples of every command (#27).
    const TEXT_SNAPS: &[&str] = &["cli_help_examples__help_examples"];

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

    const GOAL_STDOUT_GOLDENS_19: &[(&str, &str)] = &[
        ("cli_describe__describe_brief_json", "describe"),
        ("cli_describe__describe_json", "describe"),
        ("cli_describe__describe_json_empty", "describe"),
    ];

    /// Snapshots written by the active goal (#20): hidden from a build without the goal
    /// feature (the `refresh` schema they name does not exist until the goal is met).
    const GOAL_SNAPS_20: &[&str] = &[
        "cli_dry_run__refresh_dry_run_json",
        "cli_refresh__refresh_all_json",
        "cli_refresh__refresh_json",
    ];

    const GOAL_STDOUT_GOLDENS_20: &[(&str, &str)] = &[
        ("cli_dry_run__refresh_dry_run_json", "refresh"),
        ("cli_refresh__refresh_all_json", "refresh"),
        ("cli_refresh__refresh_json", "refresh"),
    ];

    /// Snapshots written by the active goal (#22): hidden from a build without the goal
    /// feature (the `scan` schema they name does not exist until the goal is met).
    const GOAL_SNAPS_22: &[&str] = &[
        "cli_dry_run__scan_dry_run_json",
        "cli_scan__scan_json",
        "cli_scan__schema_list_json",
    ];

    const GOAL_STDOUT_GOLDENS_22: &[(&str, &str)] = &[
        ("cli_dry_run__scan_dry_run_json", "scan"),
        ("cli_scan__scan_json", "scan"),
        ("cli_scan__schema_list_json", "schema"),
    ];

    /// Snapshots written by the active goal (#21): hidden from a build without the goal
    /// feature (the `relink` schema and the `POSSIBLE_MOVE` / `HASH_MISMATCH` branches they name
    /// do not exist until the goal is met).
    const GOAL_SNAPS_21: &[&str] = &[
        "cli_dry_run__relink_dry_run_json",
        "cli_relink__hash_mismatch_json_envelope",
        "cli_relink__possible_move_json_envelope",
        "cli_relink__relink_explicit_json",
        "cli_relink__relink_json",
        "cli_relink__schema_list_json",
    ];

    const GOAL_STDOUT_GOLDENS_21: &[(&str, &str)] = &[
        ("cli_dry_run__relink_dry_run_json", "relink"),
        ("cli_relink__relink_explicit_json", "relink"),
        ("cli_relink__relink_json", "relink"),
        ("cli_relink__schema_list_json", "schema"),
    ];

    /// Snapshots written or rewritten by the active goal (#24): hidden from a build without the
    /// goal feature (the `scan` schema they name gains `relinked`, `ambiguous`, `pruned` and
    /// the `counts` keys only when the goal is met; the first two exist already and are
    /// rewritten with the new keys).
    const GOAL_SNAPS_24: &[&str] = &[
        "cli_dry_run__scan_dry_run_json",
        "cli_dry_run__scan_prune_dry_run_json",
        "cli_scan__prune_confirmation_required_json_envelope",
        "cli_scan__scan_json",
        "cli_scan__scan_moves_json",
        "cli_scan__scan_prune_json",
    ];

    const GOAL_STDOUT_GOLDENS_24: &[(&str, &str)] = &[
        ("cli_dry_run__scan_prune_dry_run_json", "scan"),
        ("cli_scan__scan_moves_json", "scan"),
        ("cli_scan__scan_prune_json", "scan"),
    ];

    /// Snapshots written by the active goal (#23): hidden from a build without the goal feature
    /// (the `scan` schema they name accepts the bare scan, `--list` and `--forget-root` outputs
    /// only when the goal is met).
    const GOAL_SNAPS_23: &[&str] = &[
        "cli_dry_run__scan_forget_root_dry_run_json",
        "cli_scan_roots__bare_prune_confirmation_required_json_envelope",
        "cli_scan_roots__bare_scan_json",
        "cli_scan_roots__forget_root_json",
        "cli_scan_roots__scan_list_json",
    ];

    const GOAL_STDOUT_GOLDENS_23: &[(&str, &str)] = &[
        ("cli_dry_run__scan_forget_root_dry_run_json", "scan"),
        ("cli_scan_roots__bare_scan_json", "scan"),
        ("cli_scan_roots__forget_root_json", "scan"),
        ("cli_scan_roots__scan_list_json", "scan"),
    ];

    /// Snapshots written by the active goal (#11): hidden from a build without the goal feature
    /// (the `editable` values they hold are only right when the goal is met).
    const GOAL_SNAPS_11: &[&str] = &[
        "cli_open__add_editable_json",
        "cli_open__find_open_config_json",
    ];

    const GOAL_STDOUT_GOLDENS_11: &[(&str, &str)] = &[
        ("cli_open__add_editable_json", "add"),
        ("cli_open__find_open_config_json", "find"),
    ];

    /// Snapshots written by the active goal (#9): hidden from a build without the goal feature
    /// (the `query` schema they name, and the `READ_ONLY_QUERY` branch of `error`, do not exist
    /// until the goal is met). Two are the W3C SPARQL Results JSON of `hali query`, which the
    /// `query` schema describes and which is not the hali envelope; the third is the error
    /// envelope of an UPDATE.
    const GOAL_SNAPS_9: &[&str] = &[
        "cli_query__ask_json",
        "cli_query__read_only_query_json_envelope",
        "cli_query__select_json",
    ];

    const GOAL_STDOUT_GOLDENS_9: &[(&str, &str)] = &[
        ("cli_query__ask_json", "query"),
        ("cli_query__select_json", "query"),
    ];

    /// Snapshots written by the active goal (#5): hidden from a build without the goal feature
    /// (the `STORE_LOCKED` envelope only validates once the `error` schema has its branch).
    /// It is a stderr snapshot, so it needs no stdout row.
    const GOAL_SNAPS_5: &[&str] = &["cli_lock__store_locked_json_envelope"];

    /// Snapshots written by the active goal (#13): hidden from a build without the goal feature.
    const GOAL_SNAPS_13: &[&str] = &[
        "cli_agent__show_agent_json",
        "cli_dry_run__add_agent_dry_run_json",
    ];

    const GOAL_STDOUT_GOLDENS_13: &[(&str, &str)] = &[
        ("cli_agent__show_agent_json", "show"),
        ("cli_dry_run__add_agent_dry_run_json", "add"),
    ];

    /// Snapshots written by the active goal (#25): hidden from a build without the goal feature
    /// (the `new` schema and the `FILES_DIR_UNUSABLE` branch they name do not exist until the
    /// goal is met).
    const GOAL_SNAPS_25: &[&str] = &[
        "cli_dry_run__new_agent_dry_run_json",
        "cli_dry_run__new_dry_run_json",
        "cli_new__files_dir_unusable_json_envelope",
        "cli_new__new_json",
    ];

    const GOAL_STDOUT_GOLDENS_25: &[(&str, &str)] = &[
        ("cli_dry_run__new_agent_dry_run_json", "new"),
        ("cli_dry_run__new_dry_run_json", "new"),
        ("cli_new__new_json", "new"),
    ];

    /// Every stdout snapshot's row.
    #[allow(unused_assignments, unused_mut)]
    fn stdout_goldens() -> Vec<(&'static str, &'static str)> {
        let mut rows = STDOUT_GOLDENS.to_vec();
        rows.extend_from_slice(GOAL_STDOUT_GOLDENS);
        rows.extend_from_slice(GOAL_STDOUT_GOLDENS_12);
        rows.extend_from_slice(GOAL_STDOUT_GOLDENS_16);
        rows.extend_from_slice(GOAL_STDOUT_GOLDENS_14);
        rows.extend_from_slice(GOAL_STDOUT_GOLDENS_17);
        rows.extend_from_slice(GOAL_STDOUT_GOLDENS_15);
        rows.extend_from_slice(GOAL_STDOUT_GOLDENS_18);
        rows.extend_from_slice(GOAL_STDOUT_GOLDENS_19);
        rows.extend_from_slice(GOAL_STDOUT_GOLDENS_20);
        rows.extend_from_slice(GOAL_STDOUT_GOLDENS_22);
        rows.extend_from_slice(GOAL_STDOUT_GOLDENS_21);
        rows.extend_from_slice(GOAL_STDOUT_GOLDENS_24);
        rows.extend_from_slice(GOAL_STDOUT_GOLDENS_23);
        rows.extend_from_slice(GOAL_STDOUT_GOLDENS_11);
        rows.extend_from_slice(GOAL_STDOUT_GOLDENS_9);
        rows.extend_from_slice(GOAL_STDOUT_GOLDENS_13);
        rows.extend_from_slice(GOAL_STDOUT_GOLDENS_25);
        // The goldens #24 rewrites are hidden without the goal feature (see `goldens`), and so
        // are their rows.
        let mut hidden_24: &[&str] = GOAL_SNAPS_24;
        {
            hidden_24 = &[];
        }
        rows.retain(|(stem, _)| !hidden_24.contains(stem));
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
        {
            pending_19 = &[];
        }
        let mut pending_20: &[&str] = GOAL_SNAPS_20;
        {
            pending_20 = &[];
        }
        let mut pending_22: &[&str] = GOAL_SNAPS_22;
        {
            pending_22 = &[];
        }
        let mut pending_21: &[&str] = GOAL_SNAPS_21;
        {
            pending_21 = &[];
        }
        let mut pending_24: &[&str] = GOAL_SNAPS_24;
        {
            pending_24 = &[];
        }
        let mut pending_23: &[&str] = GOAL_SNAPS_23;
        {
            pending_23 = &[];
        }
        let mut pending_11: &[&str] = GOAL_SNAPS_11;
        {
            pending_11 = &[];
        }
        let mut pending_9: &[&str] = GOAL_SNAPS_9;
        {
            pending_9 = &[];
        }
        let mut pending_5: &[&str] = GOAL_SNAPS_5;
        {
            pending_5 = &[];
        }
        let mut pending_13: &[&str] = GOAL_SNAPS_13;
        {
            pending_13 = &[];
        }
        let mut pending_25: &[&str] = GOAL_SNAPS_25;
        {
            pending_25 = &[];
        }
        let mut out = Vec::new();
        for entry in std::fs::read_dir(snapshots_dir()).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) != Some("snap") {
                continue;
            }
            let stem = path.file_stem().unwrap().to_str().unwrap().to_string();
            if TEXT_SNAPS.contains(&stem.as_str())
                || hidden.contains(&stem.as_str())
                || pending.contains(&stem.as_str())
                || pending_16.contains(&stem.as_str())
                || pending_14.contains(&stem.as_str())
                || pending_17.contains(&stem.as_str())
                || pending_15.contains(&stem.as_str())
                || pending_18.contains(&stem.as_str())
                || pending_19.contains(&stem.as_str())
                || pending_20.contains(&stem.as_str())
                || pending_22.contains(&stem.as_str())
                || pending_21.contains(&stem.as_str())
                || pending_24.contains(&stem.as_str())
                || pending_23.contains(&stem.as_str())
                || pending_11.contains(&stem.as_str())
                || pending_9.contains(&stem.as_str())
                || pending_5.contains(&stem.as_str())
                || pending_13.contains(&stem.as_str())
                || pending_25.contains(&stem.as_str())
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
        assert!(goldens.len() >= 73, "found only {} goldens", goldens.len());
        assert!(goldens.len() >= 76, "found only {} goldens", goldens.len());
        assert!(goldens.len() >= 79, "found only {} goldens", goldens.len());
        assert!(goldens.len() >= 85, "found only {} goldens", goldens.len());
        assert!(goldens.len() >= 89, "found only {} goldens", goldens.len());
        assert!(goldens.len() >= 94, "found only {} goldens", goldens.len());
        assert!(goldens.len() >= 95, "found only {} goldens", goldens.len());
        assert!(goldens.len() >= 96, "found only {} goldens", goldens.len());
        assert!(goldens.len() >= 97, "found only {} goldens", goldens.len());
        assert!(goldens.len() >= 100, "found only {} goldens", goldens.len());
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
        // `cli_dry_run__scan_prune_dry_run_json` is the dry run of `scan --prune` (#24): the
        // `scan` schema, not a command called `scan prune`.
        if command == "scan_prune" {
            return "scan".to_string();
        }
        // `cli_dry_run__scan_forget_root_dry_run_json` is the dry run of `scan --forget-root`
        // (#23): the `scan` schema too.
        if command == "scan_forget_root" {
            return "scan".to_string();
        }
        // `cli_dry_run__add_agent_dry_run_json` is the dry run of `add --agent` (#13): the `add`
        // schema.
        if command == "add_agent" {
            return "add".to_string();
        }
        // `cli_dry_run__new_agent_dry_run_json` is the dry run of `new --agent` (#25): the `new`
        // schema.
        if command == "new_agent" {
            return "new".to_string();
        }
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

    /// #25: `hali schema new` is served, and `new` is in the list in sorted place, between
    /// `forget` and `query`.
    #[test]
    fn new_schema_is_listed() {
        let listed = fetch_list();
        assert!(listed.len() >= 27, "{listed:?}");
        let at = listed
            .iter()
            .position(|n| n == "new")
            .expect("new is listed");
        assert_eq!(listed[at - 1], "forget");
        assert_eq!(listed[at + 1], "query");
        assert!(listed.windows(2).all(|w| w[0] < w[1]), "sorted: {listed:?}");
        assert_eq!(fetch("new")["title"], "hali new --json");
    }

    /// #25: what the `new` schema accepts and refuses: one Resource record, in a real or a
    /// dry-run payload, nothing else allowed.
    #[test]
    fn new_schema_accepts_and_rejects() {
        let changes = || json!([{"op": "add", "triple": "<a:b> <a:c> <a:d> ."}]);
        let real = || json!({"schema_version": 1, "resource": record()});
        let dry = || {
            let mut v = real();
            v["dry_run"] = json!(true);
            v["changes"] = changes();
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
        let cases: Vec<(&str, Value, bool)> = vec![
            ("real", real(), true),
            ("dry run", dry(), true),
            (
                "dry run, no changes",
                with(dry(), "changes", json!([])),
                true,
            ),
            (
                "no schema_version",
                without(real(), "schema_version"),
                false,
            ),
            ("version 2", with(real(), "schema_version", json!(2)), false),
            ("no resource", without(real(), "resource"), false),
            (
                "resource is a list",
                with(real(), "resource", json!([])),
                false,
            ),
            (
                "resource without a title",
                with(real(), "resource", {
                    let mut r = record();
                    r.as_object_mut().unwrap().remove("title");
                    r
                }),
                false,
            ),
            (
                "resource with an extra key",
                with(real(), "resource", {
                    let mut r = record();
                    r["content"] = json!("x");
                    r
                }),
                false,
            ),
            ("extra key", with(real(), "path", json!("/a/b")), false),
            (
                "the add payload",
                json!({"schema_version": 1, "added": [record()], "existing": []}),
                false,
            ),
            ("dry_run alone", with(real(), "dry_run", json!(true)), false),
            ("changes alone", with(real(), "changes", changes()), false),
            ("dry_run false", with(dry(), "dry_run", json!(false)), false),
            ("dry run without changes", without(dry(), "changes"), false),
            (
                "bad op",
                with(
                    dry(),
                    "changes",
                    json!([{"op": "replace", "triple": "<a:b> <a:c> <a:d> ."}]),
                ),
                false,
            ),
        ];
        let v = validator("new");
        for (name, instance, valid) in cases {
            let errs = problems(&v, &instance);
            assert_eq!(
                errs.is_empty(),
                valid,
                "`hali schema new` should {} `{name}`: {instance}; errors: {errs:?}",
                if valid { "accept" } else { "reject" }
            );
        }
    }

    /// #25: the `error` schema has a branch for `FILES_DIR_UNUSABLE`: the `path` (a string, or
    /// null when `HOME` is unset) and one of the five reasons.
    #[test]
    fn files_dir_unusable_error_branch() {
        let unusable = |details: Value| error("FILES_DIR_UNUSABLE", details);
        let mut cases: Vec<(Value, bool)> = Vec::new();
        for reason in [
            "unresolved",
            "relative",
            "not_a_directory",
            "own_dir",
            "cannot_create",
        ] {
            cases.push((unusable(json!({"path": "/a/b", "reason": reason})), true));
            cases.push((unusable(json!({"path": null, "reason": reason})), true));
        }
        cases.push((
            unusable(json!({"path": "/a/b", "reason": "exploded"})),
            false,
        ));
        cases.push((unusable(json!({"path": "/a/b"})), false));
        cases.push((unusable(json!({"reason": "relative"})), false));
        cases.push((unusable(json!({})), false));
        cases.push((unusable(json!({"path": 5, "reason": "relative"})), false));
        cases.push((unusable(json!({"path": "/a/b", "reason": 5})), false));
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

    /// #20: what the `refresh` schema accepts and refuses: a real branch and a dry-run branch,
    /// every key required, `unchanged` a non-negative integer, nothing else allowed.
    #[test]
    fn refresh_schema_accepts_and_rejects() {
        let record = |id: &str, missing: bool| {
            json!({"id": id, "path": "/TMP/files/a.txt", "title": "a", "media_type": "text/plain",
                "editable": true, "missing": missing, "stale": false, "types": [],
                "tags": [], "byte_size": 6, "modified": "2026-10-20T14:02:11Z"})
        };
        let real = || {
            json!({"schema_version": 1,
                "refreshed": [record("0192f0c4-0000-7000-8000-000000000902", false)],
                "missing": [record("0192f0c4-0000-7000-8000-000000000904", true)],
                "unchanged": 2})
        };
        let dry = || {
            let mut v = real();
            v["dry_run"] = json!(true);
            v["changes"] = json!([{"op": "add", "triple": "<a:b> <a:c> <a:d> ."}]);
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
        let v = validator("refresh");
        let cases: Vec<(String, Value, bool)> = vec![
            ("real".into(), real(), true),
            ("dry run".into(), dry(), true),
            (
                "empty".into(),
                json!({"schema_version": 1, "refreshed": [], "missing": [], "unchanged": 0}),
                true,
            ),
            (
                "dry run, no changes".into(),
                with(with(real(), "dry_run", json!(true)), "changes", json!([])),
                true,
            ),
            ("no unchanged".into(), without(real(), "unchanged"), false),
            (
                "unchanged -1".into(),
                with(real(), "unchanged", json!(-1)),
                false,
            ),
            (
                "unchanged 1.5".into(),
                with(real(), "unchanged", json!(1.5)),
                false,
            ),
            (
                "unchanged string".into(),
                with(real(), "unchanged", json!("2")),
                false,
            ),
            (
                "unchanged is a list".into(),
                with(real(), "unchanged", json!([])),
                false,
            ),
            ("no refreshed".into(), without(real(), "refreshed"), false),
            ("no missing".into(), without(real(), "missing"), false),
            (
                "no schema_version".into(),
                without(real(), "schema_version"),
                false,
            ),
            (
                "version 2".into(),
                with(real(), "schema_version", json!(2)),
                false,
            ),
            ("extra key".into(), with(real(), "total", json!(3)), false),
            (
                "dry_run alone".into(),
                with(real(), "dry_run", json!(true)),
                false,
            ),
            (
                "changes alone".into(),
                with(real(), "changes", json!([])),
                false,
            ),
            (
                "dry_run false".into(),
                with(dry(), "dry_run", json!(false)),
                false,
            ),
            (
                "dry run, no changes key".into(),
                without(dry(), "changes"),
                false,
            ),
            (
                "bad op".into(),
                with(
                    dry(),
                    "changes",
                    json!([{"op": "replace", "triple": "<a:b> <a:c> <a:d> ."}]),
                ),
                false,
            ),
            (
                "refreshed is not records".into(),
                with(real(), "refreshed", json!(["x"])),
                false,
            ),
            (
                "record minus stale".into(),
                with(
                    real(),
                    "refreshed",
                    json!([without(
                        record("0192f0c4-0000-7000-8000-000000000902", false),
                        "stale"
                    )]),
                ),
                false,
            ),
            (
                "record extra key".into(),
                with(
                    real(),
                    "missing",
                    json!([with(
                        record("0192f0c4-0000-7000-8000-000000000904", true),
                        "x",
                        json!(1)
                    )]),
                ),
                false,
            ),
            (
                "forget payload".into(),
                json!({"schema_version": 1,
                    "resource": record("0192f0c4-0000-7000-8000-000000000902", false)}),
                false,
            ),
        ];
        for (name, instance, valid) in cases {
            let errs = problems(&v, &instance);
            assert_eq!(
                errs.is_empty(),
                valid,
                "`hali schema refresh` should {} `{name}`: {instance}; errors: {errs:?}",
                if valid { "accept" } else { "reject" }
            );
        }
    }

    /// #22, #24: what the `scan` schema accepts and refuses: a real branch and a dry-run branch,
    /// every key required, `additionalProperties: false`, the six skip reasons, exact counts
    /// (eight keys), the `relinked`, `ambiguous` and `pruned` lists, 100 entries at most in each
    /// list and 1 to 10 candidates.
    #[test]
    fn scan_schema_accepts_and_rejects() {
        let record = |id: &str, missing: bool| {
            json!({"id": id, "path": "/TMP/root/a.txt", "title": "a", "media_type": "text/plain",
                "editable": true, "missing": missing, "stale": false, "types": [],
                "tags": [], "byte_size": 6, "modified": "2026-10-20T14:02:11Z"})
        };
        let real = || {
            json!({"schema_version": 1, "root": "/TMP/root",
                "counts": {"added": 1, "refreshed": 1, "relinked": 1, "ambiguous": 1,
                    "unchanged": 2, "missing": 1, "pruned": 1, "skipped": 1},
                "media_types": {"text/plain": 1},
                "added": [record("0192f0c4-0000-7000-8000-000000000901", false)],
                "refreshed": [record("0192f0c4-0000-7000-8000-000000000902", false)],
                "relinked": [{"resource": record("0192f0c4-0000-7000-8000-000000000905", false),
                    "from": "/TMP/root/old.txt"}],
                "ambiguous": [{"resource": record("0192f0c4-0000-7000-8000-000000000906", true),
                    "candidates": ["/TMP/root/x.txt", "/TMP/root/y.txt"]}],
                "missing": [record("0192f0c4-0000-7000-8000-000000000904", true)],
                "pruned": [record("0192f0c4-0000-7000-8000-000000000907", true)],
                "skipped": [{"path": "/TMP/root/big.bin", "reason": "too_large"}],
                "truncated": false})
        };
        let dry = || {
            let mut v = real();
            v["dry_run"] = json!(true);
            v["changes"] = json!([{"op": "add", "triple": "<a:b> <a:c> <a:d> ."}]);
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
        let in_counts = |key: &str, value: Option<Value>| {
            let mut v = real();
            match value {
                Some(value) => v["counts"][key] = value,
                None => {
                    v["counts"].as_object_mut().unwrap().remove(key);
                }
            }
            v
        };
        let skipped_with = |entry: Value| with(real(), "skipped", json!([entry]));
        let relinked_with = |entry: Value| with(real(), "relinked", json!([entry]));
        let ambiguous_with = |entry: Value| with(real(), "ambiguous", json!([entry]));
        let listed = |key: &str, n: usize| {
            let one = match key {
                "relinked" => {
                    json!({"resource": record("0192f0c4-0000-7000-8000-000000000905", false),
                    "from": "/TMP/root/old.txt"})
                }
                "ambiguous" => {
                    json!({"resource": record("0192f0c4-0000-7000-8000-000000000906", true),
                    "candidates": ["/TMP/root/x.txt"]})
                }
                "skipped" => json!({"path": "/p", "reason": "too_large"}),
                _ => record("0192f0c4-0000-7000-8000-000000000901", false),
            };
            with(real(), key, Value::Array(vec![one; n]))
        };
        let candidates = |n: usize| {
            let paths: Vec<String> = (0..n).map(|i| format!("/TMP/root/c{i:02}.txt")).collect();
            ambiguous_with(
                json!({"resource": record("0192f0c4-0000-7000-8000-000000000906", true),
                "candidates": paths}),
            )
        };
        let v = validator("scan");
        let mut cases: Vec<(String, Value, bool)> = vec![
            ("real".into(), real(), true),
            ("dry run".into(), dry(), true),
            (
                "dry run, no changes".into(),
                with(with(real(), "dry_run", json!(true)), "changes", json!([])),
                true,
            ),
            (
                "empty".into(),
                json!({"schema_version": 1, "root": "/TMP/root",
                    "counts": {"added": 0, "refreshed": 0, "relinked": 0, "ambiguous": 0,
                        "unchanged": 0, "missing": 0, "pruned": 0, "skipped": 0},
                    "media_types": {}, "added": [], "refreshed": [], "relinked": [],
                    "ambiguous": [], "missing": [], "pruned": [], "skipped": [],
                    "truncated": false}),
                true,
            ),
            ("100 relinked".into(), listed("relinked", 100), true),
            ("100 ambiguous".into(), listed("ambiguous", 100), true),
            ("100 pruned".into(), listed("pruned", 100), true),
            ("100 added".into(), listed("added", 100), true),
            ("10 candidates".into(), candidates(10), true),
            ("1 candidate".into(), candidates(1), true),
            ("101 relinked".into(), listed("relinked", 101), false),
            ("101 ambiguous".into(), listed("ambiguous", 101), false),
            ("101 pruned".into(), listed("pruned", 101), false),
            ("101 added".into(), listed("added", 101), false),
            ("101 refreshed".into(), listed("refreshed", 101), false),
            ("101 missing".into(), listed("missing", 101), false),
            ("101 skipped".into(), listed("skipped", 101), false),
            ("11 candidates".into(), candidates(11), false),
            ("no candidates".into(), candidates(0), false),
            (
                "relinked entry".into(),
                relinked_with(
                    json!({"resource": record("0192f0c4-0000-7000-8000-000000000905", false),
                    "from": "/TMP/root/old.txt"}),
                ),
                true,
            ),
            (
                "relinked without from".into(),
                relinked_with(
                    json!({"resource": record("0192f0c4-0000-7000-8000-000000000905", false)}),
                ),
                false,
            ),
            (
                "relinked without resource".into(),
                relinked_with(json!({"from": "/TMP/root/old.txt"})),
                false,
            ),
            (
                "relinked extra key".into(),
                relinked_with(
                    json!({"resource": record("0192f0c4-0000-7000-8000-000000000905", false),
                    "from": "/TMP/root/old.txt", "to": "/p"}),
                ),
                false,
            ),
            (
                "relinked from a number".into(),
                relinked_with(
                    json!({"resource": record("0192f0c4-0000-7000-8000-000000000905", false),
                    "from": 7}),
                ),
                false,
            ),
            (
                "relinked is a record".into(),
                with(
                    real(),
                    "relinked",
                    json!([record("0192f0c4-0000-7000-8000-000000000905", false)]),
                ),
                false,
            ),
            (
                "ambiguous without candidates".into(),
                ambiguous_with(
                    json!({"resource": record("0192f0c4-0000-7000-8000-000000000906", true)}),
                ),
                false,
            ),
            (
                "ambiguous without resource".into(),
                ambiguous_with(json!({"candidates": ["/p"]})),
                false,
            ),
            (
                "ambiguous extra key".into(),
                ambiguous_with(
                    json!({"resource": record("0192f0c4-0000-7000-8000-000000000906", true),
                    "candidates": ["/p"], "total": 3}),
                ),
                false,
            ),
            (
                "candidates not strings".into(),
                ambiguous_with(
                    json!({"resource": record("0192f0c4-0000-7000-8000-000000000906", true),
                    "candidates": [1]}),
                ),
                false,
            ),
            (
                "pruned is not records".into(),
                with(real(), "pruned", json!(["x"])),
                false,
            ),
            (
                "pruned record minus stale".into(),
                with(
                    real(),
                    "pruned",
                    json!([without(
                        record("0192f0c4-0000-7000-8000-000000000907", true),
                        "stale"
                    )]),
                ),
                false,
            ),
            (
                "truncated".into(),
                with(real(), "truncated", json!(true)),
                true,
            ),
            (
                "counts beyond the listed".into(),
                in_counts("added", Some(json!(5000))),
                true,
            ),
        ];
        for reason in [
            "too_large",
            "unreadable",
            "not_regular",
            "non_utf8",
            "outside_root",
            "bad_ignore_file",
        ] {
            cases.push((
                format!("reason {reason}"),
                skipped_with(json!({"path": "/p", "reason": reason})),
                true,
            ));
        }
        for key in [
            "schema_version",
            "root",
            "counts",
            "media_types",
            "added",
            "refreshed",
            "relinked",
            "ambiguous",
            "missing",
            "pruned",
            "skipped",
            "truncated",
        ] {
            cases.push((format!("no {key}"), without(real(), key), false));
        }
        for key in [
            "added",
            "refreshed",
            "relinked",
            "ambiguous",
            "unchanged",
            "missing",
            "pruned",
            "skipped",
        ] {
            cases.push((format!("no counts.{key}"), in_counts(key, None), false));
            cases.push((
                format!("counts.{key} -1"),
                in_counts(key, Some(json!(-1))),
                false,
            ));
            cases.push((
                format!("counts.{key} 1.5"),
                in_counts(key, Some(json!(1.5))),
                false,
            ));
            cases.push((
                format!("counts.{key} string"),
                in_counts(key, Some(json!("1"))),
                false,
            ));
        }
        cases.extend([
            (
                "counts extra key".to_string(),
                in_counts("total", Some(json!(0))),
                false,
            ),
            (
                "extra key".to_string(),
                with(real(), "total", json!(3)),
                false,
            ),
            (
                "version 2".to_string(),
                with(real(), "schema_version", json!(2)),
                false,
            ),
            (
                "root not a string".to_string(),
                with(real(), "root", json!(7)),
                false,
            ),
            (
                "media_types negative".to_string(),
                with(real(), "media_types", json!({"text/plain": -1})),
                false,
            ),
            (
                "media_types string".to_string(),
                with(real(), "media_types", json!({"text/plain": "1"})),
                false,
            ),
            (
                "media_types is a list".to_string(),
                with(real(), "media_types", json!([])),
                false,
            ),
            (
                "truncated string".to_string(),
                with(real(), "truncated", json!("false")),
                false,
            ),
            (
                "unknown reason".to_string(),
                skipped_with(json!({"path": "/p", "reason": "hidden"})),
                false,
            ),
            (
                "skipped no reason".to_string(),
                skipped_with(json!({"path": "/p"})),
                false,
            ),
            (
                "skipped no path".to_string(),
                skipped_with(json!({"reason": "too_large"})),
                false,
            ),
            (
                "skipped extra key".to_string(),
                skipped_with(json!({"path": "/p", "reason": "too_large", "size": 1})),
                false,
            ),
            (
                "skipped is a string".to_string(),
                with(real(), "skipped", json!(["/p"])),
                false,
            ),
            (
                "added is not records".to_string(),
                with(real(), "added", json!(["x"])),
                false,
            ),
            (
                "record minus stale".to_string(),
                with(
                    real(),
                    "refreshed",
                    json!([without(
                        record("0192f0c4-0000-7000-8000-000000000902", false),
                        "stale"
                    )]),
                ),
                false,
            ),
            (
                "dry_run alone".to_string(),
                with(real(), "dry_run", json!(true)),
                false,
            ),
            (
                "changes alone".to_string(),
                with(real(), "changes", json!([])),
                false,
            ),
            (
                "dry_run false".to_string(),
                with(dry(), "dry_run", json!(false)),
                false,
            ),
            (
                "dry run, no changes key".to_string(),
                without(dry(), "changes"),
                false,
            ),
            (
                "bad op".to_string(),
                with(
                    dry(),
                    "changes",
                    json!([{"op": "replace", "triple": "<a:b> <a:c> <a:d> ."}]),
                ),
                false,
            ),
            (
                "refresh payload".to_string(),
                json!({"schema_version": 1, "refreshed": [], "missing": [], "unchanged": 0}),
                false,
            ),
        ]);
        for (name, instance, valid) in cases {
            let errs = problems(&v, &instance);
            assert_eq!(
                errs.is_empty(),
                valid,
                "`hali schema scan` should {} `{name}`: {instance}; errors: {errs:?}",
                if valid { "accept" } else { "reject" }
            );
        }
    }

    /// #23: the `scan` schema also covers the three outputs of the remembered roots: a bare scan
    /// (`roots` and `unavailable` where `root` was), `--list` and `--forget-root` (real and
    /// dry-run). `scan <root>` keeps its shape and `schema_version` stays 1.
    #[test]
    fn scan_schema_covers_remembered_roots() {
        let entry = || {
            json!({"root": "/TMP/a", "include_hidden": false, "no_ignore": true,
                "exclude": ["*.tmp"], "max_size": null, "media_types": ["text/*"]})
        };
        let bare = || {
            json!({"schema_version": 1, "roots": ["/TMP/a", "/TMP/b"],
                "unavailable": [{"root": "/TMP/gone", "reason": "no such directory"}],
                "counts": {"added": 0, "refreshed": 0, "relinked": 0, "ambiguous": 0,
                    "unchanged": 0, "missing": 0, "pruned": 0, "skipped": 0},
                "media_types": {}, "added": [], "refreshed": [], "relinked": [],
                "ambiguous": [], "missing": [], "pruned": [], "skipped": [],
                "truncated": false})
        };
        let with = |mut v: Value, key: &str, value: Value| {
            v[key] = value;
            v
        };
        let without = |mut v: Value, key: &str| {
            v.as_object_mut().unwrap().remove(key);
            v
        };
        let list = || json!({"schema_version": 1, "roots": [entry()]});
        let forget = || json!({"schema_version": 1, "root": entry()});
        let entry_with = |key: &str, value: Value| with(entry(), key, value);
        let v = validator("scan");
        let cases: Vec<(String, Value, bool)> = vec![
            ("bare".into(), bare(), true),
            (
                "bare, nothing remembered".into(),
                with(with(bare(), "roots", json!([])), "unavailable", json!([])),
                true,
            ),
            (
                "bare dry run".into(),
                with(with(bare(), "dry_run", json!(true)), "changes", json!([])),
                true,
            ),
            (
                "bare with root".into(),
                with(bare(), "root", json!("/TMP/a")),
                false,
            ),
            ("bare without roots".into(), without(bare(), "roots"), false),
            (
                "bare without unavailable".into(),
                without(bare(), "unavailable"),
                false,
            ),
            (
                "bare without counts".into(),
                without(bare(), "counts"),
                false,
            ),
            (
                "roots not strings".into(),
                with(bare(), "roots", json!([1])),
                false,
            ),
            (
                "roots a string".into(),
                with(bare(), "roots", json!("/TMP/a")),
                false,
            ),
            (
                "unavailable a string".into(),
                with(bare(), "unavailable", json!("x")),
                false,
            ),
            (
                "unavailable without reason".into(),
                with(bare(), "unavailable", json!([{"root": "/TMP/gone"}])),
                false,
            ),
            (
                "unavailable without root".into(),
                with(bare(), "unavailable", json!([{"reason": "gone"}])),
                false,
            ),
            (
                "unavailable extra key".into(),
                with(
                    bare(),
                    "unavailable",
                    json!([{"root": "/TMP/gone", "reason": "gone", "path": "/p"}]),
                ),
                false,
            ),
            (
                "unavailable reason a number".into(),
                with(
                    bare(),
                    "unavailable",
                    json!([{"root": "/TMP/gone", "reason": 1}]),
                ),
                false,
            ),
            (
                "bare extra key".into(),
                with(bare(), "extra", json!(1)),
                false,
            ),
            (
                "bare version 2".into(),
                with(bare(), "schema_version", json!(2)),
                false,
            ),
            ("list".into(), list(), true),
            (
                "list, empty".into(),
                json!({"schema_version": 1, "roots": []}),
                true,
            ),
            (
                "list, two entries".into(),
                json!({"schema_version": 1, "roots": [entry(), entry_with("max_size", json!(1024))]}),
                true,
            ),
            (
                "list extra key".into(),
                with(list(), "extra", json!(1)),
                false,
            ),
            (
                "list version 2".into(),
                with(list(), "schema_version", json!(2)),
                false,
            ),
            (
                "list without roots".into(),
                json!({"schema_version": 1}),
                false,
            ),
            (
                "list entry as string".into(),
                json!({"schema_version": 1, "roots": ["/TMP/a"]}),
                false,
            ),
            ("forget".into(), forget(), true),
            (
                "forget dry run".into(),
                with(with(forget(), "dry_run", json!(true)), "changes", json!([])),
                true,
            ),
            (
                "forget, max_size a number".into(),
                with(forget(), "root", entry_with("max_size", json!(5))),
                true,
            ),
            (
                "forget without root".into(),
                json!({"schema_version": 1}),
                false,
            ),
            (
                "forget root a string".into(),
                with(forget(), "root", json!("/TMP/a")),
                false,
            ),
            (
                "forget extra key".into(),
                with(forget(), "extra", json!(1)),
                false,
            ),
            (
                "forget dry run without changes".into(),
                with(forget(), "dry_run", json!(true)),
                false,
            ),
            (
                "forget changes not empty".into(),
                with(
                    with(forget(), "dry_run", json!(true)),
                    "changes",
                    json!([{"op": "add", "triple": "<a:b> <a:c> <a:d> ."}]),
                ),
                false,
            ),
            (
                "entry without root".into(),
                with(forget(), "root", without(entry(), "root")),
                false,
            ),
            (
                "entry without include_hidden".into(),
                with(forget(), "root", without(entry(), "include_hidden")),
                false,
            ),
            (
                "entry without no_ignore".into(),
                with(forget(), "root", without(entry(), "no_ignore")),
                false,
            ),
            (
                "entry without exclude".into(),
                with(forget(), "root", without(entry(), "exclude")),
                false,
            ),
            (
                "entry without max_size".into(),
                with(forget(), "root", without(entry(), "max_size")),
                false,
            ),
            (
                "entry without media_types".into(),
                with(forget(), "root", without(entry(), "media_types")),
                false,
            ),
            (
                "entry extra key".into(),
                with(forget(), "root", entry_with("extra", json!(1))),
                false,
            ),
            (
                "max_size a string".into(),
                with(forget(), "root", entry_with("max_size", json!("1KiB"))),
                false,
            ),
            (
                "max_size negative".into(),
                with(forget(), "root", entry_with("max_size", json!(-1))),
                false,
            ),
            (
                "max_size fractional".into(),
                with(forget(), "root", entry_with("max_size", json!(1.5))),
                false,
            ),
            (
                "include_hidden a string".into(),
                with(forget(), "root", entry_with("include_hidden", json!("no"))),
                false,
            ),
            (
                "exclude a string".into(),
                with(forget(), "root", entry_with("exclude", json!("*.tmp"))),
                false,
            ),
            (
                "exclude holds a number".into(),
                with(forget(), "root", entry_with("exclude", json!([1]))),
                false,
            ),
            (
                "media_types a string".into(),
                with(forget(), "root", entry_with("media_types", json!("text/*"))),
                false,
            ),
            (
                "root a number".into(),
                with(forget(), "root", entry_with("root", json!(1))),
                false,
            ),
        ];
        for (name, instance, valid) in cases {
            let errs = problems(&v, &instance);
            assert_eq!(
                errs.is_empty(),
                valid,
                "`hali schema scan` should {} `{name}`: {instance}; errors: {errs:?}",
                if valid { "accept" } else { "reject" }
            );
        }
    }

    /// #22: `hali schema scan` is served, and `scan` is in the list in sorted place.
    #[test]
    fn scan_schema_is_listed() {
        let listed = fetch_list();
        assert!(listed.len() >= 25, "{listed:?}");
        let at = listed
            .iter()
            .position(|n| n == "scan")
            .expect("scan is listed");
        assert!(
            ["relation remove", "relink"].contains(&listed[at - 1].as_str()),
            "{listed:?}"
        );
        assert_eq!(listed[at + 1], "schema");
        assert!(listed.windows(2).all(|w| w[0] < w[1]), "sorted: {listed:?}");
        assert_eq!(fetch("scan")["title"], "hali scan --json");
    }

    /// #21: what the `relink` schema accepts and refuses: four branches (a search or an explicit
    /// relink, each real or dry-run), every key required, nothing else allowed, the caps (100
    /// listed, 10 candidates) as limits.
    #[test]
    fn relink_schema_accepts_and_rejects() {
        let record = |id: &str, missing: bool| {
            json!({"id": id, "path": "/TMP/root/a.txt", "title": "a", "media_type": "text/plain",
                "editable": true, "missing": missing, "stale": false, "types": [],
                "tags": [], "byte_size": 6, "modified": "2026-10-20T14:02:11Z"})
        };
        let changes = || json!([{"op": "add", "triple": "<a:b> <a:c> <a:d> ."}]);
        let search = || {
            json!({"schema_version": 1, "roots": ["/TMP/root"],
                "counts": {"relinked": 1, "ambiguous": 1, "unmatched": 1},
                "relinked": [{"resource": record("0192f0c4-0000-7000-8000-000000000901", false),
                    "from": "/TMP/root/a.txt"}],
                "ambiguous": [{"resource": record("0192f0c4-0000-7000-8000-000000000902", true),
                    "candidates": ["/TMP/root/x.txt", "/TMP/root/y.txt"]}],
                "unmatched": [record("0192f0c4-0000-7000-8000-000000000904", true)],
                "truncated": false})
        };
        let explicit = || {
            json!({"schema_version": 1,
                "resource": record("0192f0c4-0000-7000-8000-000000000901", false),
                "from": "/TMP/root/a.txt", "refreshed": false})
        };
        let dry = |base: Value| {
            let mut v = base;
            v["dry_run"] = json!(true);
            v["changes"] = changes();
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
        let counts = |key: &str, value: Option<Value>| {
            let mut v = search();
            match value {
                Some(value) => v["counts"][key] = value,
                None => {
                    v["counts"].as_object_mut().unwrap().remove(key);
                }
            }
            v
        };
        let many_relinked = |n: usize| {
            let entry = json!({"resource": record("0192f0c4-0000-7000-8000-000000000901", false),
                "from": "/TMP/root/a.txt"});
            with(search(), "relinked", Value::Array(vec![entry; n]))
        };
        let candidates = |n: usize| {
            let paths: Vec<Value> = (0..n)
                .map(|i| json!(format!("/TMP/root/f{i}.txt")))
                .collect();
            with(
                search(),
                "ambiguous",
                json!([{"resource": record("0192f0c4-0000-7000-8000-000000000902", true),
                    "candidates": paths}]),
            )
        };
        let v = validator("relink");
        let cases: Vec<(String, Value, bool)> = vec![
            ("search".into(), search(), true),
            ("search, dry run".into(), dry(search()), true),
            (
                "search, dry run, no changes".into(),
                with(dry(search()), "changes", json!([])),
                true,
            ),
            (
                "search, nothing missing".into(),
                json!({"schema_version": 1, "roots": [],
                    "counts": {"relinked": 0, "ambiguous": 0, "unmatched": 0},
                    "relinked": [], "ambiguous": [], "unmatched": [], "truncated": false}),
                true,
            ),
            (
                "search, truncated".into(),
                with(search(), "truncated", json!(true)),
                true,
            ),
            ("search, 100 relinked".into(), many_relinked(100), true),
            ("search, 101 relinked".into(), many_relinked(101), false),
            ("search, 10 candidates".into(), candidates(10), true),
            ("search, 11 candidates".into(), candidates(11), false),
            ("search, no candidates".into(), candidates(0), false),
            ("explicit".into(), explicit(), true),
            ("explicit, dry run".into(), dry(explicit()), true),
            (
                "explicit, refreshed".into(),
                with(explicit(), "refreshed", json!(true)),
                true,
            ),
            ("search, no roots".into(), without(search(), "roots"), false),
            (
                "search, roots not strings".into(),
                with(search(), "roots", json!([1])),
                false,
            ),
            (
                "search, no counts".into(),
                without(search(), "counts"),
                false,
            ),
            (
                "search, no unmatched count".into(),
                counts("unmatched", None),
                false,
            ),
            (
                "search, negative count".into(),
                counts("relinked", Some(json!(-1))),
                false,
            ),
            (
                "search, fractional count".into(),
                counts("ambiguous", Some(json!(1.5))),
                false,
            ),
            (
                "search, extra count".into(),
                counts("skipped", Some(json!(0))),
                false,
            ),
            (
                "search, no relinked".into(),
                without(search(), "relinked"),
                false,
            ),
            (
                "search, no ambiguous".into(),
                without(search(), "ambiguous"),
                false,
            ),
            (
                "search, no unmatched".into(),
                without(search(), "unmatched"),
                false,
            ),
            (
                "search, no truncated".into(),
                without(search(), "truncated"),
                false,
            ),
            (
                "search, relinked without from".into(),
                with(
                    search(),
                    "relinked",
                    json!([{"resource": record("0192f0c4-0000-7000-8000-000000000901", false)}]),
                ),
                false,
            ),
            (
                "search, relinked entry is a bare record".into(),
                with(
                    search(),
                    "relinked",
                    json!([record("0192f0c4-0000-7000-8000-000000000901", false)]),
                ),
                false,
            ),
            (
                "search, ambiguous without candidates".into(),
                with(
                    search(),
                    "ambiguous",
                    json!([{"resource": record("0192f0c4-0000-7000-8000-000000000902", true)}]),
                ),
                false,
            ),
            (
                "search, unmatched is not records".into(),
                with(search(), "unmatched", json!(["x"])),
                false,
            ),
            (
                "search, extra key".into(),
                with(search(), "skipped", json!([])),
                false,
            ),
            (
                "search, dry_run alone".into(),
                with(search(), "dry_run", json!(true)),
                false,
            ),
            (
                "search, changes alone".into(),
                with(search(), "changes", json!([])),
                false,
            ),
            (
                "search, dry_run false".into(),
                with(dry(search()), "dry_run", json!(false)),
                false,
            ),
            (
                "search, dry run without changes".into(),
                without(dry(search()), "changes"),
                false,
            ),
            (
                "search, bad op".into(),
                with(
                    dry(search()),
                    "changes",
                    json!([{"op": "replace", "triple": "<a:b> <a:c> <a:d> ."}]),
                ),
                false,
            ),
            (
                "search, version 2".into(),
                with(search(), "schema_version", json!(2)),
                false,
            ),
            (
                "search, no schema_version".into(),
                without(search(), "schema_version"),
                false,
            ),
            (
                "explicit, no from".into(),
                without(explicit(), "from"),
                false,
            ),
            (
                "explicit, no refreshed".into(),
                without(explicit(), "refreshed"),
                false,
            ),
            (
                "explicit, refreshed not a bool".into(),
                with(explicit(), "refreshed", json!("no")),
                false,
            ),
            (
                "explicit, no resource".into(),
                without(explicit(), "resource"),
                false,
            ),
            (
                "explicit, resource is a list".into(),
                with(explicit(), "resource", json!([])),
                false,
            ),
            (
                "explicit, extra key".into(),
                with(explicit(), "forced", json!(true)),
                false,
            ),
            (
                "explicit with roots".into(),
                with(explicit(), "roots", json!(["/TMP/root"])),
                false,
            ),
            (
                "explicit, dry_run alone".into(),
                with(explicit(), "dry_run", json!(true)),
                false,
            ),
            (
                "forget payload".into(),
                json!({"schema_version": 1,
                    "resource": record("0192f0c4-0000-7000-8000-000000000901", false)}),
                false,
            ),
        ];
        for (name, instance, valid) in cases {
            let errs = problems(&v, &instance);
            assert_eq!(
                errs.is_empty(),
                valid,
                "`hali schema relink` should {} `{name}`: {instance}; errors: {errs:?}",
                if valid { "accept" } else { "reject" }
            );
        }
    }

    /// #21: `hali schema relink` is served, and `relink` is in the list in sorted place: 26
    /// names, between `relation remove` and `scan`.
    #[test]
    fn relink_schema_is_listed() {
        let listed = fetch_list();
        assert!(listed.len() >= 26, "{listed:?}");
        let at = listed
            .iter()
            .position(|n| n == "relink")
            .expect("relink is listed");
        assert_eq!(listed[at - 1], "relation remove");
        assert_eq!(listed[at + 1], "scan");
        assert!(listed.windows(2).all(|w| w[0] < w[1]), "sorted: {listed:?}");
        assert_eq!(fetch("relink")["title"], "hali relink --json");
    }

    /// #21: the `error` schema has a branch for each new rule: `POSSIBLE_MOVE` (the path being
    /// added and the ids of the Missing Resources with its content) and `HASH_MISMATCH` (the
    /// Resource, the path, the recorded hash and the hash of the file).
    #[test]
    fn relink_error_branches() {
        let sha_a = "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03";
        let sha_b = "a948904f2f0f479b8f8197694b30184b0d2ed1c1cd2a1ec0fb85d299a192a447";
        let id = "0192f0c4-0000-7000-8000-000000000904";
        let id2 = "0192f0c4-0000-7000-8000-000000000905";
        let mismatch = |details: Value| error("HASH_MISMATCH", details);
        let cases: Vec<(Value, bool)> = vec![
            (
                error("POSSIBLE_MOVE", json!({"path": "/a/b", "resources": [id]})),
                true,
            ),
            (
                error(
                    "POSSIBLE_MOVE",
                    json!({"path": "/a/b", "resources": [id, id2]}),
                ),
                true,
            ),
            (error("POSSIBLE_MOVE", json!({})), false),
            (error("POSSIBLE_MOVE", json!({"path": "/a/b"})), false),
            (error("POSSIBLE_MOVE", json!({"resources": [id]})), false),
            (
                error("POSSIBLE_MOVE", json!({"path": "/a/b", "resources": []})),
                false,
            ),
            (
                error("POSSIBLE_MOVE", json!({"path": "/a/b", "resources": [5]})),
                false,
            ),
            (
                error("POSSIBLE_MOVE", json!({"path": "/a/b", "resources": id})),
                false,
            ),
            (
                error("POSSIBLE_MOVE", json!({"path": 5, "resources": [id]})),
                false,
            ),
            (
                mismatch(
                    json!({"resource": id, "path": "/a/b", "expected": sha_a, "actual": sha_b}),
                ),
                true,
            ),
            (mismatch(json!({})), false),
            (
                mismatch(json!({"path": "/a/b", "expected": sha_a, "actual": sha_b})),
                false,
            ),
            (
                mismatch(json!({"resource": id, "expected": sha_a, "actual": sha_b})),
                false,
            ),
            (
                mismatch(json!({"resource": id, "path": "/a/b", "actual": sha_b})),
                false,
            ),
            (
                mismatch(json!({"resource": id, "path": "/a/b", "expected": sha_a})),
                false,
            ),
            (
                mismatch(json!({"resource": id, "path": "/a/b", "expected": 1, "actual": sha_b})),
                false,
            ),
        ];
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

    /// #9: `hali schema query` is the W3C SPARQL 1.1 Query Results JSON of a SELECT or ASK. It is
    /// not the hali envelope (no `schema_version`), a SELECT has `head.vars` and
    /// `results.bindings`, an ASK has an empty `head` and a `boolean`, never both; a term is a
    /// `uri`, `literal`, `bnode` or `triple`.
    #[test]
    fn query_schema_accepts_and_rejects() {
        let uri = |v: &str| json!({"type": "uri", "value": v});
        let select = || {
            json!({"head": {"vars": ["a", "b"]}, "results": {"bindings": [
                {"a": uri("urn:x:a"),
                 "b": {"type": "literal", "value": "v", "xml:lang": "en"}},
                {"a": uri("urn:x:c")}]}})
        };
        let ask = |b: bool| json!({"head": {}, "boolean": b});
        let with = |mut v: Value, path: &[&str], value: Value| {
            let mut at = &mut v;
            for key in &path[..path.len() - 1] {
                at = &mut at[*key];
            }
            at[path[path.len() - 1]] = value;
            v
        };
        let without = |mut v: Value, path: &[&str]| {
            let mut at = &mut v;
            for key in &path[..path.len() - 1] {
                at = &mut at[*key];
            }
            at.as_object_mut().unwrap().remove(path[path.len() - 1]);
            v
        };
        let binding =
            |term: Value| json!({"head": {"vars": ["a"]}, "results": {"bindings": [{"a": term}]}});
        let cases: Vec<(&str, Value, bool)> = vec![
            ("select", select(), true),
            ("ask true", ask(true), true),
            ("ask false", ask(false), true),
            (
                "select, no rows",
                json!({"head": {"vars": ["a"]}, "results": {"bindings": []}}),
                true,
            ),
            (
                "select, no variables",
                json!({"head": {"vars": []}, "results": {"bindings": [{}]}}),
                true,
            ),
            (
                "typed literal",
                binding(json!({"type": "literal", "value": "5",
                    "datatype": "http://www.w3.org/2001/XMLSchema#integer"})),
                true,
            ),
            (
                "plain literal",
                binding(json!({"type": "literal", "value": "t"})),
                true,
            ),
            (
                "blank node",
                binding(json!({"type": "bnode", "value": "b0"})),
                true,
            ),
            (
                "triple term",
                binding(json!({"type": "triple", "value": {
                    "subject": uri("urn:x:s"), "predicate": uri("urn:x:p"),
                    "object": {"type": "literal", "value": "o"}}})),
                true,
            ),
            // The hali envelope is not this format.
            (
                "select with schema_version",
                with(select(), &["schema_version"], json!(1)),
                false,
            ),
            (
                "ask with schema_version",
                with(ask(true), &["schema_version"], json!(1)),
                false,
            ),
            (
                "enveloped select",
                json!({"schema_version": 1, "results": select()}),
                false,
            ),
            ("select without head", without(select(), &["head"]), false),
            ("ask without head", json!({"boolean": true}), false),
            (
                "select without vars",
                without(select(), &["head", "vars"]),
                false,
            ),
            (
                "select without results",
                without(select(), &["results"]),
                false,
            ),
            (
                "select without bindings",
                without(select(), &["results", "bindings"]),
                false,
            ),
            ("empty object", json!({}), false),
            ("head only", json!({"head": {}}), false),
            (
                "ask boolean a string",
                with(ask(true), &["boolean"], json!("true")),
                false,
            ),
            (
                "ask boolean null",
                with(ask(true), &["boolean"], Value::Null),
                false,
            ),
            (
                "select and ask in one",
                with(select(), &["boolean"], json!(true)),
                false,
            ),
            (
                "ask with results",
                with(ask(true), &["results"], json!({"bindings": []})),
                false,
            ),
            (
                "bindings not an array",
                with(select(), &["results", "bindings"], json!({})),
                false,
            ),
            (
                "vars not strings",
                with(select(), &["head", "vars"], json!([1])),
                false,
            ),
            (
                "vars a string",
                with(select(), &["head", "vars"], json!("a")),
                false,
            ),
            (
                "unknown term type",
                binding(json!({"type": "iri", "value": "urn:x:a"})),
                false,
            ),
            (
                "term without type",
                binding(json!({"value": "urn:x:a"})),
                false,
            ),
            ("term without value", binding(json!({"type": "uri"})), false),
            (
                "term value a number",
                binding(json!({"type": "uri", "value": 1})),
                false,
            ),
            ("term a string", binding(json!("urn:x:a")), false),
            (
                "literal datatype a number",
                binding(json!({"type": "literal", "value": "5", "datatype": 5})),
                false,
            ),
            (
                "literal xml:lang a number",
                binding(json!({"type": "literal", "value": "x", "xml:lang": 5})),
                false,
            ),
            (
                "triple without object",
                binding(json!({"type": "triple", "value": {
                    "subject": uri("urn:x:s"), "predicate": uri("urn:x:p")}})),
                false,
            ),
            (
                "extra top-level key",
                with(select(), &["extra"], json!(1)),
                false,
            ),
            ("a list", json!([select()]), false),
        ];
        let v = validator("query");
        for (name, instance, valid) in cases {
            let errs = problems(&v, &instance);
            assert_eq!(
                errs.is_empty(),
                valid,
                "`hali schema query` should {} {name}: {instance}; errors: {errs:?}",
                if valid { "accept" } else { "reject" }
            );
        }
    }

    /// #9: the schema says what the command prints, in its title and by what it leaves out.
    #[test]
    fn query_schema_is_listed() {
        let listed = fetch_list();
        assert!(listed.len() >= 27, "{listed:?}");
        let at = listed
            .iter()
            .position(|n| n == "query")
            .expect("query is listed");
        assert!(
            ["forget", "find", "new"].contains(&listed[at - 1].as_str()),
            "{listed:?}"
        );
        assert_eq!(listed[at + 1], "refresh");
        assert!(listed.windows(2).all(|w| w[0] < w[1]), "sorted: {listed:?}");
        let schema = fetch("query");
        let title = schema["title"].as_str().expect("a title");
        for word in ["CONSTRUCT", "DESCRIBE", "Turtle"] {
            assert!(title.contains(word), "title must mention {word}: {title}");
        }
        let text = schema.to_string();
        assert!(
            !text.contains("schema_version"),
            "the W3C format has no schema_version: {text}"
        );
        assert!(!text.contains("\"$id\""), "{text}");
    }

    /// #9: the `error` schema has branches for the two rules `hali query` adds: `READ_ONLY_QUERY`
    /// carries no details; `QUERY_FAILED` carries a reason, and the service for the `service`
    /// reason.
    #[test]
    fn query_error_branches() {
        let v = validator("error");
        let read_only = |details: Value| error("READ_ONLY_QUERY", details);
        let failed = |details: Value| error("QUERY_FAILED", details);
        let cases: Vec<(Value, bool)> = vec![
            (read_only(json!({})), true),
            (read_only(json!({"x": 1})), false),
            (read_only(json!([])), false),
            (read_only(json!("x")), false),
            (read_only(Value::Null), false),
            (
                failed(json!({"reason": "service", "service": "http://127.0.0.1:9/"})),
                true,
            ),
            (failed(json!({"reason": "timeout"})), true),
            (failed(json!({"reason": "too_large"})), true),
            (failed(json!({})), false),
            (failed(json!({"reason": "service"})), false),
            (failed(json!({"reason": "service", "service": 5})), false),
            (failed(json!({"service": "http://127.0.0.1:9/"})), false),
            (failed(json!({"reason": "nope"})), false),
            (failed(json!({"reason": 1})), false),
            (failed(json!({"reason": null})), false),
            (failed(json!("timeout")), false),
            (failed(Value::Null), false),
        ];
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
}
