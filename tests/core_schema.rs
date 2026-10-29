//! Goal tests for the `core` schema registry (#6, spec §9.1): `hali::core::schema`.
//!
//! Assumed API: `names() -> Vec<&'static str>` (sorted) and
//! `get(&str) -> Option<serde_json::Value>` (the bare schema document).

mod goal {
    use hali::core::schema;
    use serde_json::Value;
    use std::collections::BTreeMap;

    fn names() -> Vec<String> {
        schema::names().iter().map(|n| n.to_string()).collect()
    }

    fn leaf_paths(cmd: &clap::Command, prefix: &str, out: &mut Vec<String>) {
        let subs: Vec<&clap::Command> = cmd
            .get_subcommands()
            .filter(|c| c.get_name() != "help")
            .collect();
        if subs.is_empty() {
            out.push(prefix.trim().to_string());
        }
        for sub in subs {
            leaf_paths(sub, &format!("{prefix} {}", sub.get_name()), out);
        }
    }

    #[test]
    fn names_and_get() {
        assert_eq!(
            names(),
            [
                "add",
                "concept add",
                "concept edit",
                "concept list",
                "concept remove",
                "concept rename",
                "concept show",
                "edit",
                "error",
                "find",
                "forget",
                "schema",
                "show",
                "type add",
                "type list",
                "type remove",
                "validate"
            ]
        );
        for name in names() {
            let doc = schema::get(&name).unwrap_or_else(|| panic!("no schema for {name}"));
            assert!(doc.is_object(), "{name}");
            assert_eq!(
                doc["$schema"], "https://json-schema.org/draft/2020-12/schema",
                "{name}"
            );
            assert!(doc.get("$id").is_none(), "{name}");
        }
        for unknown in ["", "concept", "nope", "Error", "concept  add"] {
            assert!(schema::get(unknown).is_none(), "{unknown:?}");
        }
    }

    #[test]
    fn every_command_has_a_schema() {
        let mut leaves = Vec::new();
        leaf_paths(&hali::cli::command(), "", &mut leaves);
        assert!(!leaves.is_empty());
        let names = names();
        for leaf in &leaves {
            assert!(
                names.contains(leaf),
                "subcommand `{leaf}` has no schema: add one under src/core/schemas/ and register it"
            );
        }
        for name in &names {
            assert!(
                name == "error" || leaves.contains(name),
                "schema `{name}` is not a subcommand (only `error` is not)"
            );
        }
    }

    #[test]
    fn shared_defs_identical() {
        let mut defs: BTreeMap<String, Vec<(String, Value)>> = BTreeMap::new();
        for name in names() {
            let doc = schema::get(&name).unwrap();
            if let Some(map) = doc.get("$defs").and_then(|d| d.as_object()) {
                for (def, body) in map {
                    defs.entry(def.clone())
                        .or_default()
                        .push((name.clone(), body.clone()));
                }
            }
        }
        for (def, users) in &defs {
            for (name, body) in &users[1..] {
                assert_eq!(
                    body, &users[0].1,
                    "$defs/{def} differs between `{}` and `{name}`",
                    users[0].0
                );
            }
        }
        let users = |def: &str| -> Vec<&str> {
            defs.get(def)
                .map(|u| u.iter().map(|(n, _)| n.as_str()).collect())
                .unwrap_or_default()
        };
        assert!(
            users("change").contains(&"concept add"),
            "{:?}",
            users("change")
        );
        let violation = users("violation");
        assert!(violation.contains(&"validate"), "{violation:?}");
        assert!(violation.contains(&"error"), "{violation:?}");
    }

    /// #16: the `error` schema has an `UNKNOWN_CONCEPT` branch: `details` carries the reference
    /// and at most three `{id, label}` suggestions.
    #[test]
    fn unknown_concept_details_required() {
        use serde_json::json;
        let doc = schema::get("error").unwrap();
        let validator = jsonschema::draft202012::new(&doc).unwrap();
        let envelope = |details: Value| {
            json!({"schema_version": 1, "error": {
                "rule": "UNKNOWN_CONCEPT", "message": "m", "details": details,
                "suggested_fix": "f", "suggested_command": "hali --dry-run concept add x"}})
        };
        let one = json!({"id": "ml", "label": "machine learning"});
        let cases: Vec<(Value, bool)> = vec![
            (
                envelope(json!({"concept": "x", "suggestions": [one.clone()]})),
                true,
            ),
            (envelope(json!({"concept": "x", "suggestions": []})), true),
            (
                envelope(
                    json!({"concept": "x", "suggestions": [one.clone(), one.clone(), one.clone()]}),
                ),
                true,
            ),
            (
                envelope(
                    json!({"concept": "x", "suggestions": [one.clone(), one.clone(), one.clone(), one.clone()]}),
                ),
                false,
            ),
            (envelope(json!({})), false),
            (envelope(json!({"concept": "x"})), false),
            (envelope(json!({"suggestions": []})), false),
            (envelope(json!({"concept": 1, "suggestions": []})), false),
            (
                envelope(json!({"concept": "x", "suggestions": "ml"})),
                false,
            ),
            (
                envelope(json!({"concept": "x", "suggestions": [{"id": "ml"}]})),
                false,
            ),
            (
                envelope(json!({"concept": "x", "suggestions": [{"label": "ml"}]})),
                false,
            ),
            (
                envelope(json!({"concept": "x", "suggestions": [{"id": 1, "label": "ml"}]})),
                false,
            ),
        ];
        for (instance, valid) in cases {
            let errs: Vec<String> = validator
                .iter_errors(&instance)
                .map(|e| e.to_string())
                .collect();
            assert_eq!(
                errs.is_empty(),
                valid,
                "`hali schema error` should {} {instance}; errors: {errs:?}",
                if valid { "accept" } else { "reject" }
            );
        }
    }
}
