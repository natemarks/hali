//! The JSON Schemas of every command's `--json` output and of the error envelope (spec §9.1).
//! The documents live in `src/core/schemas/` and are embedded; the shared `$defs` in
//! `defs.json` are spliced into each document that references them, so each is self-contained.

use serde_json::{Map, Value};

/// (name, document), sorted by name.
const SCHEMAS: [(&str, &str); 17] = [
    ("add", include_str!("schemas/add.json")),
    ("concept add", include_str!("schemas/concept_add.json")),
    ("concept edit", include_str!("schemas/concept_edit.json")),
    ("concept list", include_str!("schemas/concept_list.json")),
    (
        "concept remove",
        include_str!("schemas/concept_remove.json"),
    ),
    (
        "concept rename",
        include_str!("schemas/concept_rename.json"),
    ),
    ("concept show", include_str!("schemas/concept_show.json")),
    ("edit", include_str!("schemas/edit.json")),
    ("error", include_str!("schemas/error.json")),
    ("find", include_str!("schemas/find.json")),
    ("forget", include_str!("schemas/forget.json")),
    ("schema", include_str!("schemas/schema.json")),
    ("show", include_str!("schemas/show.json")),
    ("type add", include_str!("schemas/type_add.json")),
    ("type list", include_str!("schemas/type_list.json")),
    ("type remove", include_str!("schemas/type_remove.json")),
    ("validate", include_str!("schemas/validate.json")),
];

const DEFS: &str = include_str!("schemas/defs.json");

/// The schema names, sorted.
pub fn names() -> Vec<&'static str> {
    SCHEMAS.iter().map(|(name, _)| *name).collect()
}

/// The bare schema document for `name`, with the `$defs` it references, directly or through
/// another def, spliced in.
pub fn get(name: &str) -> Option<Value> {
    let (_, text) = SCHEMAS.iter().find(|(n, _)| *n == name)?;
    let mut doc: Value = serde_json::from_str(text).expect("embedded schema is JSON");
    let shared: Map<String, Value> = serde_json::from_str(DEFS).expect("embedded defs are JSON");
    let refers = |text: &str, def: &str| text.contains(&format!("\"#/$defs/{def}\""));
    let mut used: Map<String, Value> = Map::new();
    let mut texts = vec![text.to_string()];
    while let Some(text) = texts.pop() {
        for (def, body) in &shared {
            if !used.contains_key(def) && refers(&text, def) {
                texts.push(body.to_string());
                used.insert(def.clone(), body.clone());
            }
        }
    }
    used.sort_keys();
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

    #[test]
    fn defs_referenced_by_defs_are_spliced_too() {
        let add = get("add").unwrap();
        let defs: Vec<&String> = add["$defs"].as_object().unwrap().keys().collect();
        assert_eq!(defs, ["change", "resource", "tag"]);
    }
}
