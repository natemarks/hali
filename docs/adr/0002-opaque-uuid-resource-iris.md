---
status: accepted
---

# Resources are identified by opaque `urn:uuid:` IRIs, not by path or content hash

A Resource's IRI is a UUIDv7 `urn:uuid:` minted once when it is cataloged, and is never derived from its path, inode or content hash. Path and hash are both things that change in normal use (files move; text Resources are edited in Neovim), so making either the identity would break every tag, relation and catalog record each time a file moved or was edited. Storing them as ordinary mutable properties (`dcat:downloadURL`, `spdx:checksum`) means that relinking a moved file rewrites one triple and leaves everything else intact.

## Considered Options

- **The `file://` URL as the IRI** (GNOME Tracker's approach). Changes on every move or rename.
- **`urn:fileid:<fs-uuid>:<inode>`.** Survives renames on one filesystem only, and editors that save atomically change the inode.
- **A content-addressed `ni:///sha-256;…` IRI.** Survives moves but changes on every edit.
- **An `https://` slash IRI.** The Linked Data purist option, but nothing on a single machine would ever serve it.

## Consequences

- **The CLI only takes full UUIDs** (or paths) as Resource references. UUIDv7 starts with a timestamp, so short prefixes collide for files added close together.
- **The design stays open to cross-machine sync**, because identity doesn't depend on local paths.

Decided in [Ontology domain model for v1](https://github.com/natemarks/mcp-local-notes/issues/55), building on [Standard vocabularies for describing local files](https://github.com/natemarks/mcp-local-notes/issues/65).
