---
status: accepted
---

# Rust, with Turtle files as the canonical store, loaded into an in-memory Oxigraph store

hali is a user-wide CLI that Neovim and AI agents run as short-lived processes on every query, over fewer than 10k files. The project is explicitly standards-first (RDF, RDFS, OWL 2, SKOS, SPARQL 1.1, SHACL), with learning ontologies valued above performance, and Python was ruled out for packaging reasons. So hali is written in Rust; Turtle files are the canonical store; each invocation loads them into an in-memory Oxigraph store (`default-features = false`, no RocksDB); inference is done with SPARQL property paths rather than a reasoner; and SHACL validation uses rudof's `shacl` crate.

## Considered Options

- **Go + SQLite.** The best fit if standards were optional: one static binary, under 1 ms startup, about 13 ms for 10k rows. Rejected because Go has no maintained embedded RDF store or SPARQL engine, so the ontology would become a bespoke relational schema with Turtle only as an export, which defeats the learning goal.
- **Rust + an on-disk Oxigraph (RocksDB) store as canonical.** Faster opens (about 45 ms). Rejected for v1:
  - it adds a C++ build;
  - only one read-write opener is allowed, and a second fails at once;
  - read-only access during writes is undefined behaviour;
  - the on-disk format needs migrations;
  - it needs a separate export for git.

  It remains the fallback behind the `storage` seam.
- **TBox in files, ABox in RocksDB.** Pays every RocksDB cost for the bulk of the data and adds a second persistence mechanism.
- **Java/Jena.** The best ontology tooling, but JVM startup (0.5 s or more) is too slow for a picker that calls the CLI on every open.
- **Neo4j or other property-graph stores.** Not RDF and not a standard.

## Consequences

- **Load time:** about 150 ms per invocation at 100k triples, a proxy estimate measured with pyoxigraph. `make bench` must measure it natively before `v0.1.0`; if p95 exceeds 200 ms at 10k Resources, the on-disk store replaces the in-memory one behind the `storage` seam.
- **Canonical data:** human-readable, diffable Turtle that opens in Protégé and other RDF tools. The stored files are the git export, so there is no separate export path.
- **Dependencies:** they are 0.x crates with a fast release cadence (oxigraph, rudof), so every dependency is pinned exactly and upgraded deliberately.
- **Queries:** with no inference, queries must spell out transitive paths (`skos:broader*`, `rdfs:subClassOf*`).

Decided in [Choose language and storage stack](https://github.com/natemarks/mcp-local-notes/issues/56).
