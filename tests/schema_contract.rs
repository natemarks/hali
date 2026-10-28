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

    /// Every stdout snapshot's row.
    #[allow(unused_mut)]
    fn stdout_goldens() -> Vec<(&'static str, &'static str)> {
        let mut rows = STDOUT_GOLDENS.to_vec();
        rows.extend_from_slice(GOAL_STDOUT_GOLDENS);
        rows.extend_from_slice(GOAL_STDOUT_GOLDENS_12);
        rows.extend_from_slice(GOAL_STDOUT_GOLDENS_16);
        rows.extend_from_slice(GOAL_STDOUT_GOLDENS_14);
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
}
