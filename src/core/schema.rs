//! The JSON Schemas of every command's `--json` output and of the error envelope (spec §9.1).
//! The documents live in `src/core/schemas/` and are embedded; the shared `$defs` in
//! `defs.json` are spliced into each document that references them, so each is self-contained.

use serde_json::{Map, Value};

/// (name, document), sorted by name.
const SCHEMAS: [(&str, &str); 5] = [
    ("concept add", include_str!("schemas/concept_add.json")),
    ("concept list", include_str!("schemas/concept_list.json")),
    ("error", include_str!("schemas/error.json")),
    ("schema", include_str!("schemas/schema.json")),
    ("validate", include_str!("schemas/validate.json")),
];

const DEFS: &str = include_str!("schemas/defs.json");

/// The schema names, sorted.
pub fn names() -> Vec<&'static str> {
    SCHEMAS.iter().map(|(name, _)| *name).collect()
}

/// The bare schema document for `name`, with the `$defs` it references spliced in.
pub fn get(name: &str) -> Option<Value> {
    let (_, text) = SCHEMAS.iter().find(|(n, _)| *n == name)?;
    let mut doc: Value = serde_json::from_str(text).expect("embedded schema is JSON");
    let shared: Map<String, Value> = serde_json::from_str(DEFS).expect("embedded defs are JSON");
    let used: Map<String, Value> = shared
        .into_iter()
        .filter(|(def, _)| text.contains(&format!("\"#/$defs/{def}\"")))
        .collect();
    if !used.is_empty() {
        doc.as_object_mut()
            .expect("embedded schema is an object")
            .insert("$defs".to_string(), Value::Object(used));
    }
    Some(doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_sorted_and_all_resolve() {
        let names = names();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted);
        for name in names {
            assert!(get(name).is_some(), "{name}");
        }
        assert!(get("nope").is_none());
    }

    #[test]
    fn defs_are_spliced_only_where_referenced() {
        let add = get("concept add").unwrap();
        assert!(add["$defs"]["change"].is_object());
        assert!(add["$defs"].get("violation").is_none());
        assert!(get("schema").unwrap().get("$defs").is_none());
    }
}
