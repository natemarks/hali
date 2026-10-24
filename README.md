# hali

A user-wide Linux CLI that keeps a local, standards-based ontology (RDF, SKOS, OWL 2, SHACL) describing your files **in place**, plus a read-only Neovim/Telescope finder that opens text files in a buffer and everything else with the OS opener. Local AI agents use the same CLI.

**Status:** pre-implementation. The v1 spec is in [`docs/spec.md`](docs/spec.md).

## What it will do

```sh
hali scan ~/Documents --dry-run                 # see what would be cataloged
hali concept add "linked data" --broader "semantic web"
hali add ~/papers/ldp.pdf --type Paper --tag "linked data"
hali find --tag "semantic web" --json           # matches narrower concepts too
hali query 'SELECT ?t WHERE { ?r <http://purl.org/dc/terms/title> ?t }'
```

In Neovim: `:Hali find`, `:Hali concepts`, `:Hali types`, `:Hali related`, `:Hali info`.

## Docs

- [Specification](docs/spec.md)
- [Glossary](GLOSSARY.md)
- [Architecture decisions](docs/adr/)
- [Contributor and agent guide](AGENTS.md)

Install instructions arrive with the first release (`v0.1.0`).

## Background

The design was worked out on a wayfinder map in [`natemarks/mcp-local-notes`](https://github.com/natemarks/mcp-local-notes/issues/48), hali's predecessor.
