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
- [Agent usage guide](docs/agent-usage.md) (also `hali help agents`)
- [Glossary](GLOSSARY.md)
- [Architecture decisions](docs/adr/)
- [Contributor and agent guide](AGENTS.md)

## Install

On x86_64 Linux, from a GitHub release (the first is `v0.1.0`), with the [`gh`](https://cli.github.com/) CLI (spec §13.5):

```sh
gh release download -R natemarks/hali -p 'hali-*-x86_64-linux.tar.gz' -p SHA256SUMS \
  && sha256sum -c --ignore-missing SHA256SUMS && tar xzf hali-*.tar.gz && ./hali-*/install.sh
```

`install.sh` installs `hali` into `~/.local/bin`, the man page and the bash, zsh and fish completions (spec §4), and runs `hali skill install` when `~/.claude` exists and `hali skill --help` succeeds. It needs no root, so do not run it with `sudo` or as root with your own `HOME`: it installs into the `HOME` of whoever runs it. From a checkout, `make install` builds and installs the same files.

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.

## Background

The design was worked out on a wayfinder map in [`natemarks/mcp-local-notes`](https://github.com/natemarks/mcp-local-notes/issues/48), hali's predecessor.
