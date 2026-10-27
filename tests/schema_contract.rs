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

    struct Golden {
        stem: String,
        stream: String,
        body: String,
    }

    fn snapshots_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots")
    }

    fn goldens() -> Vec<Golden> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(snapshots_dir()).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) != Some("snap") {
                continue;
            }
            let stem = path.file_stem().unwrap().to_str().unwrap().to_string();
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
        STDOUT_GOLDENS
            .iter()
            .find(|(stem, _)| *stem == g.stem)
            .map(|(_, name)| *name)
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
        for (stem, name) in STDOUT_GOLDENS {
            assert!(
                stems.iter().any(|s| s == stem),
                "STDOUT_GOLDENS row `{stem}` -> `{name}` has no snapshot file"
            );
            assert!(
                goldens()
                    .iter()
                    .any(|g| g.stem == *stem && g.stream == "stdout"),
                "STDOUT_GOLDENS row `{stem}` is not a stdout snapshot"
            );
            fetch(name);
        }
        // The list a row's name comes from is itself live output.
        let listed = fetch_list();
        for (stem, name) in STDOUT_GOLDENS {
            assert!(
                listed.iter().any(|n| n == name),
                "row `{stem}` names `{name}`, which `hali schema` does not list: {listed:?}"
            );
        }
    }

    #[test]
    fn dry_run_goldens_named() {
        let dry: Vec<Golden> = goldens()
            .into_iter()
            .filter(|g| g.stem.contains("dry_run"))
            .collect();
        assert!(dry.len() >= 3, "expected the three #8 dry-run goldens");
        let concept_add = validator("concept add");
        for g in dry {
            assert_eq!(schema_name(&g), Some("concept add"), "{}", g.stem);
            assert!(g.body.contains("\"dry_run\":true"), "{}", g.stem);
            let instance: Value = serde_json::from_str(&g.body).unwrap();
            let errs = problems(&concept_add, &instance);
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
        case("error", error("UNKNOWN_CONCEPT", json!({})), true);
        case("error", error("unknown_concept", json!({})), false);
        case("error", error("", json!({})), false);
        let mut v = error("UNKNOWN_CONCEPT", json!({}));
        v.as_object_mut().unwrap().remove("schema_version");
        case("error", v, false);
        let mut v = error("UNKNOWN_CONCEPT", json!({}));
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
}
