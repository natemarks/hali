//! `describe`: the one-call orientation (spec §9.1 **Describe**): the core ontology version,
//! the store path, the counts and, unless brief, every Concept, Type and relation kind.

use super::concept::{self, Concept};
use super::relation::{self, RelationKind};
use super::types::{self, Type};
use super::{Context, resource};
use crate::ontology::{ONTOLOGY, owl};
use crate::storage::{CORE_GRAPH, Store};
use oxigraph::model::{NamedNodeRef, Term};
use serde::Serialize;

/// The counts of a store.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Counts {
    pub resources: usize,
    pub concepts: usize,
    pub types: usize,
    pub relation_kinds: usize,
    pub relations: usize,
}

/// What `describe` reports. The lists are `None` (and omitted) when brief.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Description {
    pub core_version: String,
    pub store: String,
    pub counts: Counts,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concepts: Option<Vec<Concept>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub types: Option<Vec<Type>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relation_kinds: Option<Vec<RelationKind>>,
}

/// Describes the store. `store_path` is shown as given; reads the store only.
pub fn describe(ctx: &Context, store: &Store, store_path: &str, brief: bool) -> Description {
    let concepts = concept::list(ctx, store, 0, usize::MAX);
    let types = types::list(ctx, store, 0, usize::MAX);
    let kinds = relation::list(ctx, store, 0, usize::MAX);
    Description {
        core_version: core_version(store),
        store: store_path.to_string(),
        counts: Counts {
            resources: resource::cataloged_count(store),
            concepts: concepts.total,
            types: types.total,
            relation_kinds: kinds.total,
            relations: relation::relation_count(store),
        },
        concepts: (!brief).then_some(concepts.results),
        types: (!brief).then_some(types.results),
        relation_kinds: (!brief).then_some(kinds.results),
    }
}

/// The `owl:versionInfo` of the core ontology, from the core graph only.
fn core_version(store: &Store) -> String {
    let ontology = NamedNodeRef::new_unchecked(ONTOLOGY);
    let predicate = NamedNodeRef::new_unchecked(owl::VERSION_INFO);
    let graph = NamedNodeRef::new_unchecked(CORE_GRAPH);
    store
        .quads_for_pattern(
            Some(ontology.into()),
            Some(predicate),
            None,
            Some(graph.into()),
        )
        .filter_map(|q| match q.expect("reading the in-memory store").object {
            Term::Literal(l) => Some(l.value().to_string()),
            _ => None,
        })
        .min()
        .unwrap_or_default()
}
