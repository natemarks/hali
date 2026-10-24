---
status: accepted
---

# The core ontology and SHACL shapes are built into the binary, not shipped as an editable file

The core TBox (`hali:Resource` and its standard superclasses, base declarations) and the SHACL shapes that `validate` enforces are compiled into the `hali` binary with `include_str!` and versioned with it. The user's growing vocabulary (Types, relation kinds, Concepts) lives in tool-managed `vocabulary.ttl`, the per-file facts in `resources.ttl`, and the user's own hand-written additions (alignments to NFO or schema.org, extra shapes) in an optional `extensions.ttl` that hali only reads. The core is the contract the code depends on: if users could edit it, a hand edit could silently break the queries and validation the CLI relies on, and an upgrade would have to merge their changes. The CLI also can't safely rewrite a hand-authored file, because re-serialising loses comments and formatting. Embedding the core avoids both problems while still giving the user a file to experiment with.

## Consequences

- **Core changes require a hali upgrade**, and `hali describe` reports the core ontology's version.
- **Upgrades can change the core**, which may affect existing data, so such changes must be handled when the version changes.

Decided in [Ontology domain model for v1](https://github.com/natemarks/mcp-local-notes/issues/55), refining [Standards-first RDF persistence and tooling in Rust](https://github.com/natemarks/mcp-local-notes/issues/66).
