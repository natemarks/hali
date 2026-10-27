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

/// `hali:Resource`.
pub const RESOURCE: &str = "https://w3id.org/hali#Resource";

/// Where IANA media types live (`dcat:mediaType`).
pub const IANA_MEDIA_TYPES: &str = "http://www.iana.org/assignments/media-types/";

/// Where hali names non-IANA (`x-`) media types (`dcterms:format`).
pub const HALI_MEDIA_TYPES: &str = "https://w3id.org/hali/media-types/";

/// Dublin Core terms.
pub mod dcterms {
    pub const TITLE: &str = "http://purl.org/dc/terms/title";
    pub const MODIFIED: &str = "http://purl.org/dc/terms/modified";
    pub const ISSUED: &str = "http://purl.org/dc/terms/issued";
    pub const FORMAT: &str = "http://purl.org/dc/terms/format";
    pub const MEDIA_TYPE: &str = "http://purl.org/dc/terms/MediaType";
    pub const SUBJECT: &str = "http://purl.org/dc/terms/subject";
}

/// DCAT terms.
pub mod dcat {
    pub const DOWNLOAD_URL: &str = "http://www.w3.org/ns/dcat#downloadURL";
    pub const MEDIA_TYPE: &str = "http://www.w3.org/ns/dcat#mediaType";
    pub const BYTE_SIZE: &str = "http://www.w3.org/ns/dcat#byteSize";
    pub const CATALOG_RECORD: &str = "http://www.w3.org/ns/dcat#CatalogRecord";
}

/// SPDX checksum terms.
pub mod spdx {
    pub const CHECKSUM: &str = "http://spdx.org/rdf/terms#checksum";
    pub const ALGORITHM: &str = "http://spdx.org/rdf/terms#algorithm";
    pub const SHA256: &str = "http://spdx.org/rdf/terms#checksumAlgorithm_sha256";
    pub const CHECKSUM_VALUE: &str = "http://spdx.org/rdf/terms#checksumValue";
}

/// PROV-O terms.
pub mod prov {
    pub const PERSON: &str = "http://www.w3.org/ns/prov#Person";
    pub const WAS_ATTRIBUTED_TO: &str = "http://www.w3.org/ns/prov#wasAttributedTo";
}

/// FOAF terms.
pub mod foaf {
    pub const NAME: &str = "http://xmlns.com/foaf/0.1/name";
    pub const PRIMARY_TOPIC: &str = "http://xmlns.com/foaf/0.1/primaryTopic";
}

/// SKOS terms.
pub mod skos {
    pub const CONCEPT: &str = "http://www.w3.org/2004/02/skos/core#Concept";
    pub const CONCEPT_SCHEME: &str = "http://www.w3.org/2004/02/skos/core#ConceptScheme";
    pub const IN_SCHEME: &str = "http://www.w3.org/2004/02/skos/core#inScheme";
    pub const PREF_LABEL: &str = "http://www.w3.org/2004/02/skos/core#prefLabel";
    pub const ALT_LABEL: &str = "http://www.w3.org/2004/02/skos/core#altLabel";
    pub const BROADER: &str = "http://www.w3.org/2004/02/skos/core#broader";
}
