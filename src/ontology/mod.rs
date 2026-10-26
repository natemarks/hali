//! The embedded core ontology and the vocabulary terms the code relies on.

pub mod shacl;

/// The core ontology, built into the binary (spec §3.1, ADR-0003).
pub const CORE_TTL: &str = include_str!("core.ttl");

/// The core SHACL shapes, built into the binary and loaded into the core graph (spec §2.7).
pub const SHAPES_TTL: &str = include_str!("shapes.ttl");

/// The `hali:` namespace.
pub const HALI: &str = "https://w3id.org/hali#";

/// The fixed, ordered prefix table of tool-managed Turtle (spec §3.4).
pub const PREFIXES: [(&str, &str); 12] = [
    ("rdf", "http://www.w3.org/1999/02/22-rdf-syntax-ns#"),
    ("rdfs", "http://www.w3.org/2000/01/rdf-schema#"),
    ("owl", "http://www.w3.org/2002/07/owl#"),
    ("xsd", "http://www.w3.org/2001/XMLSchema#"),
    ("skos", "http://www.w3.org/2004/02/skos/core#"),
    ("dcterms", "http://purl.org/dc/terms/"),
    ("dcat", "http://www.w3.org/ns/dcat#"),
    ("prov", "http://www.w3.org/ns/prov#"),
    ("foaf", "http://xmlns.com/foaf/0.1/"),
    ("spdx", "http://spdx.org/rdf/terms#"),
    ("sh", "http://www.w3.org/ns/shacl#"),
    ("hali", HALI),
];

/// The one Concept scheme, `hali:topics`.
pub const TOPICS: &str = "https://w3id.org/hali#topics";

/// SKOS terms.
pub mod skos {
    pub const CONCEPT: &str = "http://www.w3.org/2004/02/skos/core#Concept";
    pub const CONCEPT_SCHEME: &str = "http://www.w3.org/2004/02/skos/core#ConceptScheme";
    pub const IN_SCHEME: &str = "http://www.w3.org/2004/02/skos/core#inScheme";
    pub const PREF_LABEL: &str = "http://www.w3.org/2004/02/skos/core#prefLabel";
    pub const ALT_LABEL: &str = "http://www.w3.org/2004/02/skos/core#altLabel";
    pub const BROADER: &str = "http://www.w3.org/2004/02/skos/core#broader";
}
