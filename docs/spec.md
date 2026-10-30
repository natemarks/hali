# hali v1 specification

**Status:** accepted for implementation, 2026-10-24.

**Audience:** humans and AI agents implementing hali. Each section is normative unless it says otherwise.

**Provenance:** every decision here was made on the wayfinder map [Local file ontology CLI + Neovim finder: v1 spec](https://github.com/natemarks/mcp-local-notes/issues/48). Each section links its deciding ticket, whose resolution comment holds the full rationale. The research behind it lives on the `research/*` branches of `natemarks/mcp-local-notes`. Domain terms are defined in [`GLOSSARY.md`](../GLOSSARY.md) and long-lived decisions in [`docs/adr/`](adr/).

---

## 1. What hali is

hali is a **user-wide Linux CLI** (`hali`) that keeps a local ontology describing files **in place**, plus a **read-only Neovim plugin** that finds Resources and opens them: text files in a buffer, everything else with the OS opener.

**Goals**
- Find files by meaning: Type, Concept (with broader/narrower), relations, title.
- Build on W3C standards (RDF, RDFS, OWL 2, SKOS, SPARQL 1.1, SHACL), reusing existing vocabularies. **Learning ontologies is an explicit goal and outranks performance.**
- Serve three kinds of client through one CLI: the user at a terminal, Neovim, and local AI agents.

**Standing constraints**
- **Platform:** Ubuntu 26.04 only. OS-specific code stays behind seams so macOS remains possible.
- **Machines:** a single machine.
- **Scale:** low thousands of Resources (designed for fewer than 10k).
- **Files:** hali **never moves, renames, rewrites or deletes cataloged files**, and never writes metadata into them (no frontmatter, no xattrs). Reading frontmatter as a hint is allowed.
- **Neovim:** read-only. Metadata changes happen only through the CLI.
- **Agents:** they use the same CLI. There is no MCP server.
- **Data:** no migration from `mcp-local-notes`. v1 starts with an empty ontology.

Sources:
- [Name the new project](https://github.com/natemarks/mcp-local-notes/issues/54)
- the map's Notes and Out of scope sections

---

## 2. Domain model

Source: [Ontology domain model for v1](https://github.com/natemarks/mcp-local-notes/issues/55), building on [Standard vocabularies for describing local files](https://github.com/natemarks/mcp-local-notes/issues/65).

**Namespace:** `hali:` = `https://w3id.org/hali#`. Registering the w3id.org redirect is a release-time task; the IRIs are valid before it exists.

**Vocabularies reused:** DCAT 3, Dublin Core Terms, SKOS, PROV-O, SPDX 2.3 (checksum terms only), FOAF (`primaryTopic`, `name`), SHACL. NIE/NFO and schema.org appear only in optional user alignments.

### 2.1 Resource
- **One file per Resource.** There is no work/copies layer: a PDF and an EPUB of the same paper are two Resources linked by a relation.
- **Class:** `hali:Resource rdfs:subClassOf dcat:Distribution, prov:Entity`.
- **IRI:** a `urn:uuid:` **UUIDv7**, minted once at `add`/`scan`/`new`, never derived from path, inode or hash ([ADR-0002](adr/0002-opaque-uuid-resource-iris.md)).

| Property | Cardinality | Value |
|---|---|---|
| `dcterms:title` | exactly 1 | Plain string, **not unique**. Defaults to the file name without extension; frontmatter `title` is a hint. |
| `dcat:downloadURL` | exactly 1 | `<file:///…>` per RFC 8089 (the canonicalised real path, UTF-8 percent-encoded). |
| `dcat:mediaType` | exactly 1 of the two | IANA types: `<http://www.iana.org/assignments/media-types/{type}/{subtype}>`. |
| `dcterms:format` | (the other) | Non-IANA (freedesktop `x-`) types: `<https://w3id.org/hali/media-types/{type}/{subtype}>`, declared `a dcterms:MediaType`. |
| `dcat:byteSize` | exactly 1 | `xsd:integer`, at least 0 (the store normalises `xsd:nonNegativeInteger` to `xsd:integer`, so the shape enforces the lower bound with `sh:minInclusive 0`). |
| `dcterms:modified` | exactly 1 | The file's mtime as of the last add/refresh/scan. |
| `spdx:checksum` | exactly 1 | `<urn:uuid:…#sha256>`, a node with an IRI derived from the Resource IRI (not a blank node), with `spdx:algorithm spdx:checksumAlgorithm_sha256` and `spdx:checksumValue "…"^^xsd:hexBinary`. |
| `rdf:type` | 1 or more | `hali:Resource` and/or Types. |
| `dcterms:subject` | 0 or more | Concepts. |
| `dcterms:created`, `prov:wasAttributedTo` | 0 or 1 each | **Only on files hali created** (§8): the creation time and the requesting Agent. |

### 2.2 Types
- `owl:Class`es under `hali:Resource`, forming a hierarchy (e.g. `hali:Paper ⊑ hali:Document ⊑ hali:Resource`).
- A Resource may have **several** asserted Types; an untyped Resource is just a `hali:Resource`.
- The set ships **empty** and grows only by explicit command.
- Labels are `rdfs:label "…"@en`.
- Media type is never a Type.

### 2.3 Concepts
- **Scheme:** one `skos:ConceptScheme`, `hali:topics`. The model and CLI are shaped so that further schemes can be added later.
- **Labels:** exactly one `skos:prefLabel` per language, plus optional `skos:altLabel`s. The language tag defaults to `@en` (configurable). **All labels are unique across the scheme, per language.**
- **Hierarchy:** several `skos:broader` parents are allowed. Only `broader` is stored; narrower is derived by query.
- **IRIs:** slugs minted once from the first prefLabel (`hali:machine-learning`). A rename changes the prefLabel and keeps the old one as an altLabel; the IRI never changes.
- **Growth:** only by explicit command.

### 2.4 Relations
- Standard properties are used directly: `dcterms:relation`, `dcterms:isPartOf`, `dcterms:references`, `prov:wasDerivedFrom`.
- Custom relation kinds are `owl:ObjectProperty`s that **must** declare a standard parent via `rdfs:subPropertyOf`. They are added only by explicit command.
- Only one direction is stored; inverses (`hasPart` etc.) are derived by query.

### 2.5 Provenance
- **Catalog record:** every Resource has a `dcat:CatalogRecord` (IRI `urn:uuid:`) with `foaf:primaryTopic` (the Resource), `dcterms:issued` (when cataloged), `dcterms:modified` and `prov:wasAttributedTo` (the Agent).
- **The user:** a `prov:Person`, minted once at first run (`urn:uuid:`), with `foaf:name` from config or `$USER`.
- **AI agents:** each is a `prov:SoftwareAgent`, minted the first time it identifies itself (§9.6), with `prov:actedOnBehalfOf` the user.
- **hali itself** is not recorded as an Agent.

### 2.6 Removal
There is no archive and no delete. **Forget** removes a Resource and its catalog record from the ontology and never touches the file. It is refused while other Resources have relations pointing to it, unless `--force`. Concepts, Types and relation kinds that are in use cannot be removed.

### 2.7 Constraints (SHACL, enforced on every mutation and by `validate`)
- Exactly one `downloadURL` (an IRI matching `^file:///`), one media type, one checksum and one title per Resource.
- Every `dcterms:subject` value is a Concept in `hali:topics`.
- Every relation target is a `hali:Resource` (`hali:RelationTargetShape`, `sh:targetClass hali:Resource`): the object of any triple whose predicate reaches a standard relation through `rdfs:subPropertyOf*` is a `hali:Resource`. A relation to a node that is no cataloged Resource therefore fails SHACL, and so does every later mutation until it is removed.
- Every custom relation property has a standard parent (`hali:RelationParentShape`, `sh:targetClass owl:ObjectProperty`): a property in the `hali:` namespace reaches a standard relation through `rdfs:subPropertyOf+`. Alignment properties outside `hali:` are not constrained.
- Labels are unique.

File existence and hash drift are not graph facts. They are checked by code (§6).

### 2.8 Example (abridged)
```turtle
@prefix hali:    <https://w3id.org/hali#> .
@prefix dcat:    <http://www.w3.org/ns/dcat#> .
@prefix dcterms: <http://purl.org/dc/terms/> .
@prefix foaf:    <http://xmlns.com/foaf/0.1/> .
@prefix owl:     <http://www.w3.org/2002/07/owl#> .
@prefix prov:    <http://www.w3.org/ns/prov#> .
@prefix rdfs:    <http://www.w3.org/2000/01/rdf-schema#> .
@prefix skos:    <http://www.w3.org/2004/02/skos/core#> .
@prefix spdx:    <http://spdx.org/rdf/terms#> .
@prefix xsd:     <http://www.w3.org/2001/XMLSchema#> .

# vocabulary.ttl
hali:Paper a owl:Class ; rdfs:subClassOf hali:Resource ; rdfs:label "Paper"@en .
hali:linked-data a skos:Concept ; skos:inScheme hali:topics ;
    skos:prefLabel "linked data"@en ; skos:broader hali:semantic-web .

# resources.ttl
<urn:uuid:0192f0c4-7b1e-7cc3-9a43-6c1d2e8f4b10> a hali:Resource , hali:Paper ;
    dcterms:title "Linked Data Patterns" ;
    dcat:downloadURL <file:///home/nmarks/papers/linked-data-patterns.pdf> ;
    dcat:mediaType <http://www.iana.org/assignments/media-types/application/pdf> ;
    dcat:byteSize 482113 ;
    dcterms:modified "2026-10-20T14:02:11Z"^^xsd:dateTime ;
    spdx:checksum <urn:uuid:0192f0c4-7b1e-7cc3-9a43-6c1d2e8f4b10#sha256> ;
    dcterms:subject hali:linked-data .
<urn:uuid:0192f0c4-7b1e-7cc3-9a43-6c1d2e8f4b10#sha256>
    spdx:algorithm spdx:checksumAlgorithm_sha256 ;
    spdx:checksumValue "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"^^xsd:hexBinary .
<urn:uuid:0192f0c4-7b20-7a01-8d2e-1f0b9c3a5e77> a dcat:CatalogRecord ;
    foaf:primaryTopic <urn:uuid:0192f0c4-7b1e-7cc3-9a43-6c1d2e8f4b10> ;
    dcterms:issued "2026-10-24T09:30:00Z"^^xsd:dateTime ;
    prov:wasAttributedTo <urn:uuid:0192f0c4-0000-7000-8000-000000000001> .
```

---

## 3. Storage

Sources:
- [Choose language and storage stack](https://github.com/natemarks/mcp-local-notes/issues/56) ([ADR-0001](adr/0001-rust-turtle-canonical-oxigraph.md), [ADR-0003](adr/0003-core-ontology-embedded-in-binary.md))
- [Standards-first RDF persistence and tooling in Rust](https://github.com/natemarks/mcp-local-notes/issues/66)

### 3.1 Files (in `$XDG_DATA_HOME/hali/`, default `~/.local/share/hali/`)
| File | Contents | Written by |
|---|---|---|
| core ontology + SHACL shapes | `hali:Resource`, the four standard relations (`dcterms:relation`, `dcterms:isPartOf`, `dcterms:references` and `prov:wasDerivedFrom`, each an `owl:ObjectProperty` with an `@en` `rdfs:label`, and `dcterms:isPartOf` and `dcterms:references` under `dcterms:relation`, as DCMI states), other base declarations, shapes | **built into the binary** (`include_str!`); `describe` reports its version |
| `vocabulary.ttl` | Types, custom relation kinds, the `hali:topics` scheme and its Concepts | the CLI |
| `resources.ttl` | Resources, checksum nodes, catalog records, Agents | the CLI |
| `extensions.ttl` (optional) | the user's own alignments and extra shapes | the user, by hand; **read-only to hali** |

**The Turtle files are canonical, and they are the export.** There is no separate export/import path; the data directory can be put under git as-is.

### 3.2 Loading
- Each invocation loads all four sources into an **in-memory** `oxigraph::Store`, one **named graph per file**.
- `oxigraph` is built with `default-features = false`: pure Rust, no RocksDB.
- Everything sits behind one **`storage` seam**. If `make bench` (§14.5) shows p95 over 200 ms at 10k Resources, an on-disk Oxigraph store replaces the in-memory one behind that seam.

### 3.3 Concurrency
- **Writers** take an exclusive `flock` (`std::fs::File::lock`, Rust ≥ 1.89) on a lock file in the data dir and publish each changed file by write-temp-then-`rename`.
- **Readers** take a shared lock.
- **A process that cannot get the lock waits up to 5 s**, then exits 3 with `STORE_LOCKED`.

### 3.4 Serialization of tool-managed files
- **No blank nodes.** RDFC-1.0 (`oxrdf`, feature `rdfc-10`) is the fallback if one ever appears.
- Triples are sorted by subject, then predicate, then object (comparing N-Triples forms), with `rdf:type` first per subject, and written with `oxttl::TurtleSerializer`.
- A **fixed, ordered prefix table**: `rdf`, `rdfs`, `owl`, `xsd`, `skos`, `dcterms`, `dcat`, `prov`, `foaf`, `spdx`, `sh`, `hali`.
- A first line `# generated by hali; edit with the CLI`, LF line endings and a trailing newline.
- Loading and re-serializing a file is **byte-identical** (a golden test enforces this).

### 3.5 Inference
**None in v1.** Subclass, broader-Concept and sub-property matching use SPARQL 1.1 property paths (`rdf:type/rdfs:subClassOf*`, `dcterms:subject/skos:broader*`, `rdfs:subPropertyOf*`).

---

## 4. Locations (XDG)

Sources:
- [XDG base-dir layout and user-wide install conventions](https://github.com/natemarks/mcp-local-notes/issues/52)
- [Where tool-created files live](https://github.com/natemarks/mcp-local-notes/issues/57)

| What | Where |
|---|---|
| Canonical store (§3.1) | `$XDG_DATA_HOME/hali/` (`~/.local/share/hali/`); overridable by `HALI_DATA_DIR` |
| Rebuildable caches (none required in v1) | `$XDG_CACHE_HOME/hali/` |
| Logs, remembered scan roots and their settings | `$XDG_STATE_HOME/hali/` (`~/.local/state/hali/`) |
| Config (optional) | `$XDG_CONFIG_HOME/hali/config.toml` |
| Files created by `hali new` | `$XDG_DOCUMENTS_DIR/hali/` (`~/Documents/hali/`), via XDG user-dirs; overridable by `files.dir` |
| Binary | `~/.local/bin/hali` |
| Man page / completions | `~/.local/share/man/man1/`, `~/.local/share/bash-completion/completions/` (and zsh/fish equivalents) |

Nothing is ever written under scan roots or next to cataloged files. All path resolution goes through the **`paths` seam**.

### 4.1 Config file (all keys optional)
```toml
[user]
name = "Nate Marks"            # foaf:name of the prov:Person; default $USER

[vocabulary]
language = "en"                # language tag for Concept and Type labels

[files]
dir = "~/Documents/hali"       # where `hali new` creates files

[open]
editor = ["text/html"]         # MIME patterns forced into Neovim
system = ["text/csv"]          # MIME patterns forced to the OS opener

[scan]
exclude = ["*.tmp", "Downloads/**"]
include_hidden = false
max_size = "1GiB"
```
The `[user]` and `[vocabulary]` key names were not fixed on the map and are proposed here. The others come from their deciding tickets.

---

## 5. Media types and opening

Sources:
- [Idiomatic file-type detection and opening on Ubuntu 26.04](https://github.com/natemarks/mcp-local-notes/issues/51)
- [Open rule: nvim buffer vs OS opener](https://github.com/natemarks/mcp-local-notes/issues/58)

### 5.1 Detection
- Done **in-process** with the freedesktop shared-mime-info database, **not** `file`/libmagic, which reports `.md` as `text/plain`. It sits behind the **`media_type` seam** and uses the `xdg-mime` crate. If that crate proves broken, the fallback is reading `/usr/share/mime/{globs2,magic,subclasses}` directly.
- **Order:** filename glob first, then content magic.
- **Empty files:** use the glob result, not `application/x-zerosize`.
- **No glob match:** text-looking content becomes `text/plain`; binary content becomes `application/octet-stream`.

### 5.2 Editable rule (computed at output time, never stored)
- `editable = is_a(media_type, text/plain)` over the shared-mime-info subclass tree, with the implicit rule that every `text/*` type is a subtype of `text/plain`.
- **Built-in exceptions** (never editable): `text/html`, `image/svg+xml`, `application/postscript`, `application/rtf`, `application/x-desktop`.
- **Precedence:** `open.system` > `open.editor` > built-in exceptions > the `is_a` rule. Config entries are MIME types, and `type/*` wildcards are allowed.
- txt and md are the tested and documented targets; JSON, TOML, shell scripts, source code and so on also open in Neovim.

### 5.3 Who opens
The **Neovim plugin** opens files: `:edit` when `editable`, `vim.ui.open` (a detached `xdg-open`) otherwise. **The CLI never launches editors or GUI apps** (there is no `hali open` in v1).

---

## 6. Identity, change and relink

Source: [Resource identity, missing files and relink](https://github.com/natemarks/mcp-local-notes/issues/59).

- **Reads never hash.** `find`, `show` and the picker only `stat`.
- **`missing`** and **`stale`** are computed at output time (`stat`, compared with the stored `dcat:byteSize`/`dcterms:modified`) and never stored. `validate` reports stale and duplicate-content Resources as information, not errors.
- **`hali refresh [<ref>…|--all]`** re-hashes and updates `spdx:checksum`, `dcat:byteSize` and `dcterms:modified`. `scan` refreshes everything under its root.
- **`hali relink [--root DIR]…`**: for each Missing Resource, walk the search roots, compare sizes first, and hash only the files whose size matches.
  - Exactly one match: update `dcat:downloadURL`.
  - Several matches: report them and change nothing.
  - Default roots: every remembered scan root plus `files.dir`.
- **`hali relink <ref> <path>`**: an explicit move. If the content hash differs (the file was moved and edited), it is refused with `HASH_MISMATCH` unless `--force`, which also refreshes the hash.
- **Duplicates:**
  - Same content at different paths is allowed.
  - Re-`add` of a cataloged path is **idempotent** and returns the existing Resource.
  - `add` of a new path whose hash matches a *missing* Resource is refused with `POSSIBLE_MOVE`, suggesting `relink`; `--new` overrides.
- **Paths:** `add` stores the **real path** (symlinks resolved).
- **What can be a Resource:** **regular files only** (otherwise `NOT_A_REGULAR_FILE`), at any absolute local path, including outside `$HOME`. **Non-UTF-8 paths are rejected** (`NON_UTF8_PATH`).
- **Hashing:** SHA-256 only, recorded via `spdx:algorithm`. Files are hashed in full at `add`/`refresh`/`scan`.

---

## 7. Scan

Sources:
- [scan ignore rules and pruning](https://github.com/natemarks/mcp-local-notes/issues/67)
- §6

- **Walker:** the `ignore` crate (ripgrep's).
  - Skips hidden files and directories by default.
  - Honours `.gitignore` inside git repos, plus `.ignore`, `.git/info/exclude`, the global gitignore and **`.haliignore`** (gitignore syntax) in any directory.
  - Does not follow symlinked directories, and resolves symlinked files to their real paths.
- **Built-in excludes:** `node_modules`, `target`, `__pycache__`, `venv`, plus `[scan].exclude`.
- **Size limit:** files over `max_size` (default 1 GiB) are skipped and reported. An explicit `add` has no limit and bypasses all ignore rules.
- **What gets registered:** every regular file that passes the filters, of any media type, **untagged and untyped**, with its detected media type. `--media-type` (repeatable, wildcards allowed) narrows a scan.
- **`--dry-run`** prints a summary first: counts by media type, and new / refreshed / relinked / missing / skipped.
- **Remembered settings:** `--include-hidden`, `--no-ignore`, `--exclude GLOB`, `--max-size` and `--media-type` are stored with the root in `$XDG_STATE_HOME/hali/`.
  - A bare `hali scan` re-scans every remembered root with its own settings.
  - `hali scan --list` shows the roots.
  - `hali scan --forget-root <root>` stops remembering a root without touching its Resources.
- **Moves within a root:** a new file whose hash matches exactly one Missing Resource is **relinked automatically** and listed in the summary. With several candidates, they are reported and nothing changes.
- **Vanished files** are only reported by default. `--prune` forgets them, with `forget`'s rules: `--yes` is needed without a TTY, and Resources with inbound relations are skipped unless `--force`. Files are never deleted.
- **Ignore rules gate only new registrations.** Already-cataloged files are never un-cataloged or flagged by them.
- **Progress** goes to stderr when it is a TTY.

---

## 8. Creating files (`hali new`)

Source: [Where tool-created files live](https://github.com/natemarks/mcp-local-notes/issues/57).

- **`hali new "<title>" [--type T]… [--tag C]…`** creates a **markdown** file and catalogs it in one step. It is CLI-only.
- **Location:** `files.dir` (default `~/Documents/hali/`), a flat directory created on first use.
- **Name:** `<slug-of-title>.md`, with `-2`, `-3`, … appended on collision. The name is fixed at creation; retitling changes only `dcterms:title`.
- **Contents:** exactly `# <title>\n`. **No frontmatter.**
- **Afterwards** it is an ordinary Resource (never rewritten or renamed; `forget` leaves the file on disk). It also carries `dcterms:created` and `prov:wasAttributedTo` the requesting Agent.
- **Output:** the command prints the Resource and exits. It never launches an editor; use `--path-only` (e.g. `nvim "$(hali new 'X' --path-only)"`).

---

## 9. CLI

Sources:
- [CLI command surface and JSON contract](https://github.com/natemarks/mcp-local-notes/issues/60)
- [Conventions for agent-friendly CLIs and agent docs](https://github.com/natemarks/mcp-local-notes/issues/63)

One `hali` binary (clap 4, derive).

### 9.1 Commands
```
# orientation
hali describe [--brief]                  # vocabulary, types, relation kinds, counts, core ontology version,
                                         # store path, skill install state
hali schema [<command>]                  # JSON Schema of each command's --json output and of the error envelope
hali help agents [--snippet]             # embedded agent usage guide; --snippet prints an AGENTS.md block
hali skill install [--agents] [--print]

# Resources
hali add <path>… [--title T] [--type T]… [--tag C]… [--new]
hali new "<title>" [--type T]… [--tag C]…
hali scan [<root>] [--include-hidden] [--no-ignore] [--exclude GLOB]… [--max-size SIZE]
          [--media-type M]… [--prune] [--list] [--forget-root <root>]
hali find [--tag C]… [--exact] [--type T]… [--title TEXT] [--media-type M]
          [--related-to REF [--via REL]] [--missing] [--stale]
          [--sort modified|added|title] [--limit N] [--offset N] [--fields F,…] [--path-only]
hali show <ref> [--path-only]            # one Resource incl. relations and catalog record
hali edit <ref> [--title T] [--add-type T]… [--remove-type T]… [--add-tag C]… [--remove-tag C]…
hali relate <ref> <relation> <ref>
hali unrelate <ref> <relation> <ref>
hali forget <ref> [--force]
hali refresh [<ref>…|--all]
hali relink [--root DIR]…
hali relink <ref> <path> [--force]
hali validate
hali query '<sparql>'

# vocabulary (noun–verb)
hali concept add <label> [--broader C]… [--alt L]…
hali concept list | show <C> | rename <C> <new label> | remove <C>
hali concept edit <C> [--add-alt L]… [--remove-alt L]… [--add-broader C]… [--remove-broader C]…
hali type add <Name> [--parent T] | list | remove <T>
hali relation add <name> --parent <standard property> | list | remove <R>

# hidden
hali generate man
hali generate completions <bash|zsh|fish>

# global flags
--json   --dry-run (every mutating command)   --agent NAME   --yes
```
**Not in v1:** `export`/`import` (the files are the export), `infer`, `open`.

**Tags:** `add --tag C` tags the Resources it newly catalogs; for an already-cataloged path it is ignored, like `--title`. `edit` needs at least one change flag (else a usage error); naming one Concept in both `--add-tag` and `--remove-tag` is a usage error. Adding a present tag or removing an absent one is a silent no-op. An edit is one mutation with one SHACL check, and bumps the catalog record's `dcterms:modified` only when something changed. Its text summary is `edited|would edit|unchanged resource <id> "<title>"`. `concept add --broader C` writes a `skos:broader` link to each resolved Concept; the output's `broader` lists their ids, sorted. Every Concept reference is resolved before anything changes. `edit` resolves `--add-tag` references first, then `--remove-tag`, each list in argument order, and reports the first unknown one in that order. The CLI rejects an `edit` with no change flag before reading the store (exit 2); core keeps the same check.

**Forget:** `forget <ref>` removes the Resource's triples from `resources.ttl`, its own checksum node (`<id>#sha256`; a `spdx:checksum` link to any other node leaves that node alone), every catalog record whose `foaf:primaryTopic` it is, and every triple that points at it, and the format node of a non-IANA media type when no other Resource uses it; it never touches the file, the user Person, `vocabulary.ttl` or `extensions.ttl`. A Resource whose file is gone can still be forgotten, by path or UUID. An id that is no longer cataloged is `UNKNOWN_RESOURCE`. It prints `{"schema_version":1,"resource":<the record as it was>}` (text: `forgot|would forget resource <id> "<title>" <path>`). It is destructive and confirms as §9.8 says. It is refused with `FORGET_BLOCKED_BY_RELATIONS` (§2.6) while other Resources have relations pointing to the Resource, unless `--force` is given, which removes those relations too (see **Relations**).

**Concepts:** `concept show`, `rename` and `edit` read and write labels in the configured language only; labels in other languages are never changed, except that `remove` deletes all of them. Labels stay unique per language across the scheme, prefLabels and altLabels together, folded as in §9.2 (`LABEL_TAKEN`, `details {"label","language","concept"}`, the Concept that holds it).
- **`concept show <C>`** is read-only. It prints `{"schema_version":1,"concept":{"id","label","alt_labels","broader","narrower","tagged"}}`. `narrower` is the sorted ids of the Concepts with a direct `skos:broader <C>` in any graph (`extensions.ttl` included), derived by query and never stored. `tagged` is the number of distinct subjects with `dcterms:subject <C>` in any graph (direct tags only). Text: one `name<TAB>value` line each for `id`, `label`, `alt_labels`, `broader`, `narrower` and `tagged`, lists joined with `, ` (empty: nothing after the tab).
- **`concept rename <C> <new label>`** normalises the label as `concept add` does, replaces the configured-language prefLabel in `vocabulary.ttl` and adds the old prefLabel as an altLabel; the IRI never changes. A new label that folds equal to another Concept's label is `LABEL_TAKEN`. One equal to an altLabel of the Concept itself swaps them (that altLabel is removed). The exact current prefLabel is a no-op (`changes []`); a change of case or spacing only replaces the prefLabel and adds no altLabel. It prints `{"schema_version":1,"concept":<the Concept after>}`; text `renamed|would rename|unchanged concept <id> "<label>"`. It never asks for confirmation.
- **`concept edit <C> [--add-alt L]… [--remove-alt L]… [--add-broader C]… [--remove-broader C]…`:** all four flags repeat and at least one is required (a usage error before the store is read; core keeps the check). Checks run in this order: (1) the same folded label in `--add-alt` and `--remove-alt` is a usage error, before the store is read; (2) `<C>` is resolved; (3) `--add-broader`, then `--remove-broader`, each in argument order, the first unknown one being `UNKNOWN_CONCEPT`; (4) `<C>` in `--add-broader` ("a Concept cannot be broader than itself"), or one Concept in both broader lists ("a Concept cannot be both added and removed"), is a usage error; (5) `--add-alt` values in order: `LABEL_TAKEN` if another Concept holds the label; (6) SHACL. These are silent no-ops: adding an alt that folds equal to one of the Concept's own labels; removing an alt the Concept does not have (matched folded; naming the prefLabel removes nothing, because only `rename` changes it); adding a broader link that is already there; removing one that is absent or held only in `extensions.ttl`. Broader cycles are allowed (`find` handles them, §9.7). `skos:narrower` is never written. Output and text as for `rename`, with `edited|would edit|unchanged`. It never asks.
- **`concept remove <C>`** is refused with `IN_USE` (exit 1) while anything has `dcterms:subject <C>` in any graph, or any other Concept has `skos:broader <C>` in any graph. Otherwise it removes, from `vocabulary.ttl` only, every triple whose subject is `<C>` (all languages, its own broader links) and every triple whose object is `<C>`. It never removes the `hali:topics` scheme node, even for the last Concept, and refuses a hand-edited vocabulary's `hali:topics` that is also a Concept with a usage error (exit 2, nothing written) before `IN_USE`; a blank node with `skos:broader <C>` counts as a narrower Concept for `IN_USE`, and Concepts are compared as nodes, not by id; and never touches `resources.ttl` or `extensions.ttl`; a Concept that exists only in `extensions.ttl` gives `changes []`, no prompt, exit 0 and the text `unchanged concept …`. It prints `{"schema_version":1,"concept":<the Concept as it was>}`; text `removed|would remove concept <id> "<label>"`. It is destructive and confirms as §9.8 says. Order: usage, store, `UNKNOWN_CONCEPT`, `IN_USE`, SHACL, then the confirmation.
- **`IN_USE`:** message `Concept <id> is in use: <n> tagged, broader of <m> Concepts; nothing was written`; `details {"concept":"<id>","tagged":<n>,"narrower":<m>}`; `suggested_fix` `Untag the Resources (hali edit <ref> --remove-tag <id>) and unlink the narrower Concepts (hali concept edit <C> --remove-broader <id>), then retry.`; `suggested_command` `hali find --exact --tag <id>` when `<n>` is above 0, else `hali concept show <id>`. An `<id>` in these texts is POSIX single-quoted unless it is made of `[A-Za-z0-9._-]` only, and in `--tag=<id>` form when it starts with `-`. It is checked before SHACL. Type `remove` and relation `remove` add their own cases (see **Types** and **Relations**).

**Types:** a Type is a named node `T`, not `hali:Resource`, with `T a owl:Class` and `T rdfs:subClassOf+ hali:Resource`, both read from any graph (`extensions.ttl` included). Its id is its `hali:` local name, otherwise its whole IRI; its `label` is its `rdfs:label` in the configured language (the smallest if several), falling back to the id; its `parents` are the sorted ids of its direct `rdfs:subClassOf` objects that are Types (`hali:Resource` and alignment classes are never listed; a top-level Type has `[]`). A Type record is `{"id","label","parents"}`. hali always asserts `a hali:Resource` on a Resource; a Type adds a separate `a <T>`, `--remove-type` never removes `hali:Resource`, an untyped Resource has `types: []`, and `types` lists asserted Types only, never inherited supertypes. `type` commands never write `extensions.ttl` or `resources.ttl`.
- **`type add <Name> [--parent T]`:** `<Name>` matches `^[A-Za-z][A-Za-z0-9_-]*$` and is at most 64 characters (a usage error, exit 2, in the CLI and in core); it is the IRI local name (`hali:<Name>`) and the label `rdfs:label "<Name>"@<lang>`; nothing is normalised. A name that folds (ASCII, case-insensitive) to `resource` is a usage error ("Resource is the root class hali:Resource, not a Type"). `--parent` is not repeatable. It writes to `vocabulary.ttl`: `T a owl:Class`, `T rdfs:label "<Name>"@<lang>` and `T rdfs:subClassOf <parent or hali:Resource>` (no direct link to `hali:Resource` when a parent is given). Order of checks: usage; store; `--parent` resolved (`UNKNOWN_TYPE`); `LABEL_TAKEN` if any Type's id or configured-language label folds equal to the name (`details {"label","language","type"}`, message `label "<Name>"@<lang> is already used by Type <id>`, `suggested_fix` "Use the existing Type, or choose a different name.", `suggested_command` `hali type list`); a usage error if `hali:<Name>` already appears in any position of any graph ("hali:<Name> already names something in the store; choose another name"); SHACL. It prints `{"schema_version":1,"type":{"id","label","parents"}}`; text `added|would add type <id> "<label>"`. It never asks for confirmation.
- **`type list [--limit N] [--offset N]`** is read-only. It lists every Type sorted by label, then id, as `{"schema_version":1,"results":[<type>…],"total","truncated","next_offset"}`. Text: `<id><TAB><label><TAB><parents joined with ", ">` per line, each value escaped for the terminal.
- **`type remove <T>`:** order of checks: usage; store; `UNKNOWN_TYPE`; a usage error if `<T>` is `hali:Resource`; `IN_USE`; SHACL; then the confirmation. It is refused with `IN_USE` (exit 1) while any subject has `rdf:type <T>`, or any subject other than `<T>` has `rdfs:subClassOf <T>`, in any graph; a blank node counts, and Types are compared as nodes, not by id. Otherwise it removes, from `vocabulary.ttl` only, every triple whose subject is `<T>` (all languages) and every triple whose object is `<T>`. A Type that exists only in `extensions.ttl` gives `changes []`, no prompt, exit 0 and the text `unchanged type …`. It prints `{"schema_version":1,"type":<the Type as it was>}`; text `removed|would remove|unchanged type <id> "<label>"`. It is destructive and confirms as §9.8 says.
- **`IN_USE` for a Type:** message `Type <id> is in use: <n> typed, parent of <m> Types; nothing was written`; `details {"type":"<id>","typed":<n>,"subtypes":<m>}`; `suggested_fix` `Remove the Type from the Resources (hali edit <ref> --remove-type <id>) and remove its subtypes first (hali type remove <S>), then retry.`; `suggested_command` `hali find --type <id>` when `<n>` is above 0, else `hali type list`. An `<id>` in these texts is POSIX single-quoted unless it is made of `[A-Za-z0-9._-]` only, and in `--remove-type=<id>` / `--type=<id>` form when it starts with `-`. It is checked before SHACL.
- **`add --type T…`** types only the Resources it newly catalogs; for an already-cataloged path it is ignored, like `--tag`. Every Type reference is resolved before anything changes: the `--type`s in argument order, then the `--tag`s. Each Type gives `R rdf:type T` in `resources.ttl`.
- **`edit --add-type T… --remove-type T…`** join `--title`, `--add-tag` and `--remove-tag` as the change flags, at least one of which is required (the core usage message is `edit needs --title, --add-type, --remove-type, --add-tag or --remove-tag`). References are resolved in the order `--add-type`, `--remove-type`, `--add-tag`, `--remove-tag`, each in argument order, and the first unknown one is reported. One Type in both type lists is a usage error ("a Type cannot be both added and removed"), checked before the same check for tags. These are silent no-ops: adding an asserted Type; removing an unasserted one; removing a Type held only through a subclass; removing one asserted only in `extensions.ttl`. Output and text are as for tags.
- **`find --type T…`** is described in §9.7.

**Relations:** a relation kind is one of the four standard relations (`dcterms:relation`, `dcterms:isPartOf`, `dcterms:references`, `prov:wasDerivedFrom`, a fixed list in code that the core ontology declares, §3.1) or a named `P a owl:ObjectProperty` with `P rdfs:subPropertyOf+` a standard one, both read from any graph (`extensions.ttl` included). Its id is its `hali:` local name, otherwise `prefix:local` from the fixed prefix table (`dcterms:isPartOf`), otherwise its whole IRI; its `label` is its `rdfs:label` in the configured language (the smallest if several), falling back to the id; its `parents` are the sorted ids of its direct `rdfs:subPropertyOf` objects that are kinds. A relation kind record is `{"id","label","parents","standard"}`, `standard` being true for the four. Only the stored direction exists, `A R B` in `resources.ttl`: the inverse is derived by query and never written, and no inverse name (`hasPart`) appears in any output. `relation` commands write only `vocabulary.ttl`; `relate`, `unrelate` and `forget` write only `resources.ttl`; nothing writes `extensions.ttl`.
- **`relation add <name> --parent <P>`:** `--parent` is required and not repeatable; `<P>` is a relation reference (§9.2) that must name a standard relation. `<name>` matches `^[a-z][A-Za-z0-9_-]*$` and is at most 64 characters (a usage error, exit 2, in the CLI and in core); it is the IRI local name (`hali:<name>`) and the label `rdfs:label "<name>"@<lang>`; nothing is normalised. It writes to `vocabulary.ttl`: `hali:<name> a owl:ObjectProperty`, the label, and `rdfs:subPropertyOf <P>`. Order of checks: usage; store; `--parent` resolved (`UNKNOWN_RELATION`); a usage error if `<P>` is not a standard relation ("--parent must be a standard relation: dcterms:relation, dcterms:isPartOf, dcterms:references or prov:wasDerivedFrom"); `LABEL_TAKEN` if any kind's local id part or configured-language label folds equal to the name (`details {"label","language","relation"}`, message `label "<name>"@<lang> is already used by relation kind <id>`, `suggested_fix` "Use the existing relation kind, or choose a different name.", `suggested_command` `hali relation list`); a usage error if `hali:<name>` already appears in any position of any graph ("hali:<name> already names something in the store; choose another name"); SHACL. It prints `{"schema_version":1,"relation_kind":{"id","label","parents","standard"}}`; text `added|would add relation <id> "<label>"`. It never asks for confirmation.
- **`relation list [--limit N] [--offset N]`** is read-only. It lists every relation kind, the four standard ones included, sorted by label, then id, as `{"schema_version":1,"results":[<relation kind>…],"total","truncated","next_offset"}`. Text: `<id><TAB><label><TAB><parents joined with ", ">` per line, each value escaped for the terminal.
- **`relation remove <R>`:** order of checks: usage; store; `UNKNOWN_RELATION`; a usage error if `<R>` is a standard relation ("<id> is a standard relation and cannot be removed"); `IN_USE`; SHACL; then the confirmation. It is refused with `IN_USE` (exit 1) while any triple in any graph has `<R>` as its predicate, or any subject other than `<R>` has `rdfs:subPropertyOf <R>`; a blank node counts, and kinds are compared as nodes, not by id. Otherwise it removes, from `vocabulary.ttl` only, every triple whose subject is `<R>` and every triple whose object is `<R>`. A kind that exists only in `extensions.ttl` gives `changes []`, no prompt, exit 0 and the text `unchanged relation …`. It prints `{"schema_version":1,"relation_kind":<the kind as it was>}`; text `removed|would remove|unchanged relation <id> "<label>"`. It is destructive and confirms as §9.8 says.
- **`IN_USE` for a relation kind:** message `relation kind <id> is in use: <n> relations, parent of <m> relation kinds; nothing was written`; `details {"relation":"<id>","used":<n>,"subproperties":<m>}`, `<n>` being the number of triples that use the kind as predicate and `<m>` the number of subjects that are `rdfs:subPropertyOf` it; `suggested_fix` `Remove the relations (hali unrelate <ref> <id> <ref>) and remove its sub-relations first (hali relation remove <S>), then retry.`; `suggested_command` `hali relation list`. An `<id>` in these texts is POSIX single-quoted unless it is made of `[A-Za-z0-9._-]` only. It is checked before SHACL.
- **`relate <A> <R> <B>`:** `<A>` and `<B>` are Resource references (§9.2), `<R>` a relation reference. Order of checks: usage; store; `<A>` (`UNKNOWN_RESOURCE`); `<R>` (`UNKNOWN_RELATION`); `<B>` (`UNKNOWN_RESOURCE`); a usage error if `<A>` and `<B>` are the same Resource ("a Resource cannot be related to itself"); SHACL. It writes the single triple `<A> <R> <B>` to `resources.ttl`, never the inverse, and, when that changes anything, sets `<A>`'s catalog record's `dcterms:modified` to now, as `edit` does. A triple `resources.ttl` already holds is a no-op (`changes []`); one only `extensions.ttl` holds is still written. It prints `{"schema_version":1,"relation":{"from","relation","to"}}`, the ids of the two Resources and the kind; text `related|would relate|unchanged <A> <relation id> <B>` with the ids in full. It never asks for confirmation.
- **`unrelate <A> <R> <B>`:** the same checks, except that `<B>` may be the full UUID of a Resource that is no longer cataloged, so that a relation left dangling by a hand edit can be removed. It removes exactly the triple `<A> <R> <B>` from `resources.ttl`: not a sub-property's triple and not the reverse direction. An absent triple, or one only `extensions.ttl` holds, is a no-op. It bumps `<A>`'s catalog record's `dcterms:modified` when it changes anything. Output as for `relate`; text `unrelated|would unrelate|unchanged <A> <relation id> <B>`. It never asks.
- **`forget <ref> [--force]`:** order of checks: usage; store; `UNKNOWN_RESOURCE`; `FORGET_BLOCKED_BY_RELATIONS`, unless `--force`; SHACL; then the confirmation. It is blocked while any Resource other than this one has a triple with a relation kind as predicate pointing at it, in any graph. Message `Resource <id> has <n> inbound relations from <m> other Resources; nothing was written` (<n> counts the distinct triples, <m> the distinct subjects); `details {"resource","relations","from"}` (the id and the two counts); `suggested_fix` `Remove them (hali unrelate <ref> <relation> <id>), or forget with --force to remove them too, then retry.`; `suggested_command` `hali show <id>`. With `--force` the removals are as without it, the inbound relations of `resources.ttl` going with the Resource; an inbound relation that only `extensions.ttl` holds cannot be removed, so SHACL refuses the forget (`SHACL_VIOLATION`, nothing written). Relations the Resource itself holds go with it, and a relation of the Resource to itself does not block. The confirmation retry is `hali forget <id> --force --yes` when `--force` was given, else `hali forget <id> --yes`.
- **`find --related-to REF [--via REL]`** is described in §9.7, and the relations of `show` in §9.3.

**`hali schema`:** a name is the subcommand path as separate words (`hali schema concept add`), or `error` for the error envelope. With no name it lists the names, sorted: one per line in text mode, `{"schema_version":1,"schemas":[…]}` with `--json`. With a name it prints the bare JSON Schema document (draft 2020-12, no `$id`, no external `$ref`) on one line, in text and `--json` mode alike, not wrapped in the envelope (as with `hali query`, §9.9). An unknown name, or extra words, is a usage error (exit 2) that lists the valid names. It reads no config or store and writes nothing. Every command with a `--json` output has a schema, and the test suite checks every golden JSON snapshot against its schema (§14.6).

### 9.2 References
- **Resource `<ref>`:** a path (absolute, or relative to the current directory, resolved to the real path) or the **full** UUID. Prefixes are not accepted.
- **Concept:** its slug or any of its labels. The exact slug (the `hali:` local name, as printed in `tags[].id`) wins; then any `prefLabel` or `altLabel` in the configured language, compared whitespace-normalised and case-insensitive (as label uniqueness is). Anything else is `UNKNOWN_CONCEPT` (exit 1), with `details {"concept": <ref>, "suggestions": [{"id","label"}…]}`: at most 3 Concepts whose folded slug or any label contains the folded reference, or is within edit distance max(2, len/3) of it, ranked by distance, then label, then id. Its `suggested_command` is `hali --dry-run concept add <ref>`, the reference POSIX single-quoted unless it is made of `[A-Za-z0-9._-]` only. With several references, the first unknown one in argument order is reported. A reference starting with `-` gets a `--` before it (`hali --dry-run concept add -- -x`, or `-- '-1 rated'` when it needs quoting), so it is not read as a flag; on the command line it is given as `--tag=-x`. An empty or whitespace-only reference is a usage error (exit 2), not `UNKNOWN_CONCEPT`.
- **Type:** its id (the `hali:` local name, as printed in `types[]`): the exact id wins; then an ASCII case-insensitive id match; then a configured-language `rdfs:label` compared folded as for Concepts; among several folded matches the first in IRI order wins. An empty or whitespace-only reference, or one that resolves to `hali:Resource`, is a usage error (exit 2). Anything else is `UNKNOWN_TYPE` (exit 1), message `no Type matches <ref>`, with `details {"type": <ref>, "suggestions": [{"id","label"}…]}` (at most 3, ranked as for Concepts), `suggested_fix` "Use the name of an existing Type, or add the Type first." and `suggested_command` `hali --dry-run type add <ref>` when `<ref>` meets the Type name rules, else `hali type list`. With several references, the first unknown one in argument order is reported.
- **Relation:** its id (the `hali:` local name, `prefix:local` for a term of the fixed prefix table such as `dcterms:isPartOf`, otherwise the whole IRI; as printed by `relation list`): the exact id wins; then the reference expanded through the prefix table (`hali:annotates`) equal to a kind's IRI; then an ASCII case-insensitive id match, the first in IRI order winning. A bare name never resolves to a standard relation (`isPartOf` is not `dcterms:isPartOf`). An empty or whitespace-only reference is a usage error (exit 2). Anything else is `UNKNOWN_RELATION` (exit 1), message `no relation kind matches <ref>`, with `details {"relation": <ref>, "suggestions": [{"id","label"}…]}` (at most 3, ranked as for Concepts, a kind's names being its id, its local part and its configured-language labels, so `isPartOf` suggests `dcterms:isPartOf`), `suggested_fix` "Use an existing relation kind (hali relation list), or add one first." and `suggested_command` `hali --dry-run relation add <ref> --parent dcterms:relation` when `<ref>` meets the relation name rules, else `hali relation list`. With several references, the first unknown one in argument order is reported.

### 9.3 Output
- **Text** is the human default. `--json` gives a stable envelope, `{"schema_version": 1, …}`. `schema_version` is an integer, bumped only when the contract breaks, and is independent of the crate version.
- **Lists** add `results`, `total`, `truncated` and `next_offset`, with a default `--limit 50` and `--offset`. `--fields` narrows records.
- **Resource record:**
  ```json
  {"id":"0192f0c4-7b1e-7cc3-9a43-6c1d2e8f4b10","path":"/home/nmarks/papers/linked-data-patterns.pdf",
   "title":"Linked Data Patterns","media_type":"application/pdf","editable":false,"missing":false,
   "stale":false,"types":["Paper"],"tags":[{"id":"linked-data","label":"linked data"}],
   "byte_size":482113,"modified":"2026-10-20T14:02:11Z"}
  ```
  `show` adds `relations` and the catalog record. `relations` holds `{"direction":"out"|"in","relation":"<relation id>","resource":<Resource record>}` for every distinct triple, in any graph, whose predicate is a relation kind and whose other end is a cataloged Resource: `out` when the shown Resource is the subject, `in` when it is the object. A relation of a Resource to itself is listed once, as `out`; a dangling end is skipped (`validate` reports it). They are sorted `out` before `in`, then by relation id, then by the other Resource's id, and the nested records carry `missing` and `stale` like the shown one. Text: after `modified`, one line per relation, `relation<TAB>-> <relation id> <id> "<title>"` for `out` and `relation<TAB><- <relation id> <id> "<title>"` for `in`.
- **`--path-only`** prints one absolute path per line (`find`, `show`, `new`).
- **No pager.** No color when stdout isn't a TTY or `NO_COLOR` is set.
- **`find` never returns file content.**

### 9.4 Exit codes and errors
| Code | Meaning |
|---|---|
| 0 | success, **including empty results** (never 1 for "no matches") |
| 1 | rule/domain error (a structured error was emitted) |
| 2 | usage error (bad flags or arguments) |
| 3 | store or environment problem (no store, `STORE_LOCKED`, unparseable file) |

- **Under `--json`**, the error goes to stderr:
  ```json
  {"schema_version":1,"error":{"rule":"UNKNOWN_CONCEPT","message":"…","details":{"concept":"ml","suggestions":[]},
   "suggested_fix":"…","suggested_command":"hali --dry-run concept add ml"}}
  ```
- **In text mode**, the output is one `error:` line plus a `hint:` line.
- **Size:** errors stay well under 10k characters.
- **Rules** (SCREAMING_SNAKE, minimum set): `UNKNOWN_RESOURCE`, `UNKNOWN_CONCEPT`, `UNKNOWN_TYPE`, `UNKNOWN_RELATION`, `LABEL_TAKEN`, `POSSIBLE_MOVE`, `FORGET_BLOCKED_BY_RELATIONS`, `IN_USE`, `HASH_MISMATCH`, `NOT_A_REGULAR_FILE`, `NON_UTF8_PATH`, `FILE_UNREADABLE`, `SHACL_VIOLATION`, `CONFIRMATION_REQUIRED`, `READ_ONLY_QUERY`, `STORE_LOCKED`.

### 9.5 Dry run
`--dry-run` is a global flag, like `--json`, accepted before or after the subcommand. A mutating command run with it returns the same payload as a real run, with the same keys in the same order, plus `"dry_run": true` and a `changes` list of the triples it would add to or remove from a tool-managed file (`vocabulary.ttl`, `resources.ttl`), in N-Triples form. A real run never prints `dry_run` or `changes`.
```json
{"op":"add","triple":"<urn:uuid:…> <http://purl.org/dc/terms/subject> <https://w3id.org/hali#ml> ."}
```
- `changes` is always present, and `[]` when nothing would change. A triple the file (graph) it goes to already holds is not listed; a scheme triple that only `extensions.ttl` holds is listed, because the real run writes it into `vocabulary.ttl`.
- **Order:** every `add` before every `remove`, then the whole triple string in plain byte order. No triple is listed twice, and `rdf:type` is not forced first.
- **Triple form:** canonical N-Triples as the RDF library prints it, plus ` .`: full IRIs in `<>`, no prefixes; simple literals have no datatype; other datatypes are `^^<iri>`; language tags are lowercase.
- SHACL runs against the would-be graph, so violations are reported.
- A dry run writes nothing: it creates no data dir, file, temp file or lock file, and changes no file. It does not check that the data dir is writable, so it can succeed where the real run would fail with `STORE_IO`.
- A real run rewrites the whole tool-managed file in canonical form (§3.4), while `changes` lists only the triples that differ, so comments and formatting in a hand-edited file are lost on a real run.
- **Errors:** a dry run reports the same errors as a real run with `--yes`, with identical stderr and exit code, and no `dry_run` or `changes` in the envelope; it never fails with `CONFIRMATION_REQUIRED` (§9.8). A successful dry run exits 0.
- **Read-only commands** (`concept list`, `concept show`, `type list`, `relation list`, `validate`, `schema`, `find`, `show`, `query`) accept `--dry-run` and ignore it: their output is identical to a run without it.
- **Text mode:** `concept add` prints `would add concept <id> "<label>"`. Each mutating command prints its own conditional summary line, followed by the shared `+ <triple>` or `- <triple>` line per change, and `note: dry run; nothing was written` on stderr.

### 9.6 Agent identity
- `--agent NAME` overrides the `HALI_AGENT` environment variable.
- With neither set, changes are attributed to the user.
- An unknown name mints a new `prov:SoftwareAgent` that `prov:actedOnBehalfOf` the user.

### 9.7 `find` semantics
- `--tag` matches the Concept **and every narrower Concept** (`dcterms:subject/skos:broader*`, with the `skos:broader` triples of every file, `extensions.ttl` included; cycles terminate); `--exact` turns that off for every `--tag`, and without `--tag` it does nothing. An unknown Concept is `UNKNOWN_CONCEPT`, not an empty result.
- `--type` matches the Type **and every subclass** (`rdf:type/rdfs:subClassOf*`, with the `rdfs:subClassOf` triples of every file, `extensions.ttl` included; cycles terminate). Repeated `--type`s are ANDed with each other and with every other filter; `--exact` applies to `--tag` only. An unknown Type is `UNKNOWN_TYPE`, not an empty result; tags are resolved before Types.
- `--related-to REF` matches a Resource X when `X p REF` or `REF p X` holds in the union of the graphs (`extensions.ttl` included) with `p rdfs:subPropertyOf* V`, where V is the `--via` relation or, without `--via`, any of the four standard relations. `--via` matches the relation **and every sub-property** (`dcterms:relation` includes `dcterms:isPartOf`, `dcterms:references` and the custom kinds under them; `prov:wasDerivedFrom` stands alone), and requires `--related-to` (a usage error, exit 2, otherwise). REF itself is never a match, cycles of sub-properties terminate, and only one hop is followed. `--related-to` is ANDed with every other filter and is not repeatable. Resolution order: `--related-to`, the `--tag`s, the `--type`s, then `--via`; an unknown Resource is `UNKNOWN_RESOURCE` and an unknown relation `UNKNOWN_RELATION`, not an empty result.
- Repeated filters are **AND**.
- `--title` is a case-insensitive substring match.
- **Default sort:** `modified` descending.

### 9.8 Destructive commands
- `forget`, `scan --prune`, `concept|type|relation remove` and `relink --force` ask for confirmation **only on a TTY**: when stdin **and** stderr are both terminals. The prompt goes to stderr, `forget resource <id> "<title>" <path>? [y/N] ` for forget, `remove concept <id> "<label>"? [y/N] ` for `concept remove` and `remove type <id> "<label>"? [y/N] ` for `type remove` and `remove relation <id> "<label>"? [y/N] ` for `relation remove` (no trailing newline; the title or label is escaped for the terminal), and the answer is one line from stdin; stdout stays clean and `--json` changes none of this. `y` or `yes` (trimmed, any case) confirms; anything else, end of input or a read error declines, with the same rule as below and `"declined": true`, and nothing is written.
- **Without both terminals** they fail with `CONFIRMATION_REQUIRED` (exit 1, `details {"command": …, "declined": false}`), suggesting `--yes`: its `suggested_command` is the command again with full references and `--yes` (`hali concept remove <id> --yes`; `hali --yes concept remove -- <id>` when the id starts with `-`; likewise `hali type remove <id> --yes` and `hali relation remove <id> --yes`, a relation id with a `:` being single-quoted; `hali forget <id> --yes`, or `hali forget <id> --force --yes` when `--force` was given), and `details.command` names the command (`"concept remove"`, `"type remove"`, `"relation remove"`, `"forget"`).
- `--yes` is a global flag (§9.1), accepted before or after the subcommand. It confirms; commands that are not destructive ignore it.
- **Order:** usage, store, domain and SHACL errors come before the confirmation. `--dry-run` (§9.5) never asks and never fails with `CONFIRMATION_REQUIRED`. A command that would change nothing never asks.
- Nothing ever waits on stdin: without both terminals it is never read, and `--yes` and `--dry-run` never read it.

### 9.9 `hali query`
- **Read-only:** SPARQL 1.1 over all named graphs. UPDATE is rejected with `READ_ONLY_QUERY`.
- **Output with `--json`:** SELECT/ASK results use the **W3C SPARQL 1.1 Query Results JSON** format, not the hali envelope; CONSTRUCT/DESCRIBE results are Turtle.
- **Text mode** shows a table.
- **Paging:** there is no `--limit`; write `LIMIT` in the query.

---

## 10. Neovim plugin

Sources:
- [Telescope picker over an external CLI in Neovim 0.12](https://github.com/natemarks/mcp-local-notes/issues/53)
- [Neovim plugin UX for v1](https://github.com/natemarks/mcp-local-notes/issues/61)
- [Open rule: nvim buffer vs OS opener](https://github.com/natemarks/mcp-local-notes/issues/58)

**Read-only.** It never changes metadata, and it never runs a mutating `hali` command.

### 10.1 Pickers
| Picker | Shows |
|---|---|
| `find` | All Resources, fuzzy-matched on title, tags and path. |
| `concepts` | A Concept tree (indented under broader Concepts) → that Concept's Resources, including narrower Concepts. |
| `types` | A Type → its Resources, including subtypes. |
| `related` | Resources related to the **current buffer's** Resource, in both directions, with relation names. |

### 10.2 Entries
```
 Attention Is All You Need   Paper  #ml #transformers   ~/papers/attention.pdf  ↗
 SPARQL property paths       Note   #sparql             ~/Documents/hali/sparql-property-paths.md
 Old budget                  —      #finance            ~/Documents/budget.ods  [missing]
```
- A media-type icon (nvim-web-devicons, when present).
- `↗` marks Resources that open externally.
- Missing Resources are dimmed and marked `[missing]`.
- Paths are shown home-relative.

### 10.3 Previews and actions
- **Preview:** file contents for editable Resources (Telescope's file previewer). Everything else, and missing files, gets a **metadata card**: title, types, tags, media type, size, modified, path, relations, catalog record.
- **Actions:**

  | Key | Action |
  |---|---|
  | `<CR>` | open |
  | `<C-v>` / `<C-x>` / `<C-t>` | vertical split / split / tab (editable Resources) |
  | `<C-o>` | force the OS opener |
  | `<C-y>` | yank the absolute path |
- **Missing Resources** never open a buffer at their path, since `:w` would recreate the file. Selecting one shows a notification suggesting `hali relink`.
- **`:Hali info`** shows the current buffer's Resource as a read-only floating metadata card. If the file isn't cataloged, it shows a notification with the exact `hali add <path>` command; the plugin never runs it.

### 10.4 Integration
- **Data:**
  - **Calls:** `vim.system` runs `hali find --json --limit <max_results>` asynchronously, and the picker opens on completion. The plugin uses `finders.new_table`, not job finders, which drop stderr and the exit code.
  - **Truncation:** if `truncated` is true, the plugin warns.
  - **Errors:** CLI errors (stderr JSON) become notifications that include `suggested_command`.
  - **Vocabulary:** `hali describe --json` supplies the Concept and Type lists, and `hali show <ref> --json` supplies relations.
- **Commands:** `:Hali find|concepts|types|related|info` and `:Telescope hali find|concepts|types|related`.
- **Keymaps:** **none by default.** The README suggests:

  | Key | which-key desc |
  |---|---|
  | `<leader>o` | group `'[O]ntology'` |
  | `<leader>of` | `'[O]ntology [F]ind'` |
  | `<leader>oc` | `'[O]ntology [C]oncepts'` |
  | `<leader>ot` | `'[O]ntology [T]ypes'` |
  | `<leader>or` | `'[O]ntology [R]elated'` |
  | `<leader>oi` | `'[O]ntology [I]nfo'` |

  These were checked against the author's config (`natemarks/kickstart.nvim`), where `<leader>o` is free.
- **Config:** `require("hali").setup({ cmd = "hali", max_results = 5000 })`. That is the whole config.
- **Health:** `:checkhealth hali` checks the binary is on `$PATH`, that `schema_version` is supported, and that `hali describe` succeeds. A `schema_version` mismatch at runtime says which side to upgrade.
- **Without Telescope:** the same pickers run through `vim.ui.select` (no preview), and `:Hali info` still works.
- **Installation:** supports both **lazy.nvim** (`{ "natemarks/hali", version = "*" }`, with a root `lazy.lua` packspec declaring the optional Telescope dependency and `opts = {}`) and Neovim 0.12's **`vim.pack`** (`vim.pack.add({ { src = "https://github.com/natemarks/hali", version = vim.version.range("0.x") } })`).

---

## 11. Agent integration

Sources:
- [Agent usage: docs, skill and discovery](https://github.com/natemarks/mcp-local-notes/issues/64)
- [Conventions for agent-friendly CLIs and agent docs](https://github.com/natemarks/mcp-local-notes/issues/63)

### 11.1 The CLI surface
- **Orientation:** `describe`, `schema`, `help agents`.
- **Output:** bounded, paged JSON; exit 0 for empty results; structured errors with `suggested_command`; `--dry-run` everywhere; never interactive without a TTY.
- **`--help`** starts with 2-4 real examples (including one with `--json` and one with `--dry-run`). The man page is generated from the same clap definitions.

### 11.2 The skill (`skill/SKILL.md`, embedded in the binary)
- **Frontmatter:** portable, with `name: hali`, a short third-person description that leads with trigger words ("find, open, tag or catalog local files and documents; ontology, concepts, types, related files; hali"), and `metadata: {hali-version: X.Y.Z}`.
- **`allowed-tools`** pre-approves **read-only commands only**: `Bash(hali describe *)`, `find`, `show`, `query`, `schema`, `validate`, `help`, and dry runs through the prefix rule `Bash(hali --dry-run *)`. Match `--dry-run` only as a prefix: the flag then sits before the subcommand and any `--`, so it is always parsed as the flag (`--dry-run=false` and `--no-dry-run` are usage errors, so nothing can unset it). A rule matching it at the end or anywhere in the command would pre-approve real writes, because `hali concept add -- --dry-run` adds a Concept labelled `--dry-run` and a quoted label such as `"x --dry-run"` can slip past a matcher that strips quotes. Mutations still go through Claude Code's permission prompt.
- **Body** (under 200 lines):
  1. Set `HALI_AGENT=<agent name>`.
  2. Run `hali describe --json` first.
  3. Read-only flow: `find` → `show` → read the file at `path` directly.
  4. For mutations, run with `--dry-run` first, show the user the triple changes and ask before running for real.
  5. Never grow the vocabulary without asking. On `UNKNOWN_CONCEPT`, offer the closest existing Concepts or a `concept add`.
  6. Turn structured errors into questions using `suggested_command`.
  7. Always report absolute paths.
  8. Use `hali query` for anything `find` can't express.
- **`references/usage.md`** holds the usage guide (§11.3).

### 11.3 Install and keeping in sync
- **`hali skill install`** writes the embedded skill to `~/.claude/skills/hali/`. `--agents` also writes `~/.agents/skills/hali/` (Codex; opencode reads both), `--print` writes it to stdout, and `--dry-run` is supported.
- **`hali describe`** reports `skill: {installed, binary, stale}`.
- **The release `install.sh`** runs `hali skill install` only if `~/.claude` exists.
- **Usage guide:** `docs/agent-usage.md` is the single source. It is embedded for `hali help agents`, copied as the skill's `references/usage.md`, and linked from the README. A unit test asserts that every `hali …` command it mentions exists in the clap tree.
- **Other projects:** `hali help agents --snippet` prints a block for other repos' `AGENTS.md`/`CLAUDE.md`. hali never edits other repos.

---

## 12. Repository layout

Source: [Repo layout for the CLI and nvim plugin](https://github.com/natemarks/mcp-local-notes/issues/62).

```
hali/
├── Cargo.toml  Cargo.lock  rust-toolchain.toml
├── src/
│   ├── main.rs              # thin: parse args → call lib
│   ├── lib.rs
│   ├── cli/                 # clap defs, text/JSON rendering, exit codes, hidden `generate`
│   ├── core/                # one module per operation; no clap, no stdout; takes Clock + IdGen
│   ├── ontology/            # core.ttl + shapes.ttl (include_str!), SPARQL queries, SHACL
│   ├── storage/             # seam: load files → Store, flock, sorted serialization
│   ├── media_type/          # seam: shared-mime-info
│   └── paths/               # seam: XDG base dirs + user-dirs
├── tests/                   # cargo integration tests: real binary on temp stores
│   └── snapshots/           # insta golden files
├── plugin/hali.lua
├── lua/hali/                # init, cli, pickers, card, health
├── lua/telescope/_extensions/hali.lua
├── lazy.lua
├── doc/hali.txt
├── spec/                    # plenary-busted tests + fake `hali` script fixture
│   └── install/             # lazy_init.lua, pack_init.lua (per-manager install smoke tests)
├── skill/SKILL.md
├── scripts/install.sh       # shipped in release tarballs
├── docs/{spec.md, agent-usage.md, adr/}
├── AGENTS.md  CLAUDE.md  GLOSSARY.md  README.md  LICENSE
├── Makefile  .pre-commit-config.yaml
└── .github/{workflows/static-check.yml, dependabot.yml}
```
- **Single crate** (lib + bin).
- **`core` is transport-agnostic, and `cli` is a thin adapter.** No business logic lives in `cli`.
- **The plugin lives at the repo root**, because neither lazy.nvim nor `vim.pack` can load a plugin from a subdirectory.

---

## 13. Build, install and release

Source: [Repo layout for the CLI and nvim plugin](https://github.com/natemarks/mcp-local-notes/issues/62).

### 13.1 Toolchain
- Rust via **rustup**, pinned in `rust-toolchain.toml` (a specific stable version ≥ 1.89).
- All dependencies are **pinned exactly** (`=`) in `Cargo.toml`.
- **Dependabot** checks `cargo` and `github-actions` weekly.
- **Pre-commit** runs gitleaks and `make static`.

### 13.2 Make targets
| Target | Does |
|---|---|
| `static` | `fmt-check clippy dead-code unit audit shellcheck stylua-check lua-test` |
| `static-check` | CI variant (check-only), run exactly once per PR and per push to `main` |
| `unit` | `cargo test --features test-hooks`: every test that needs no outside software |
| `unit-update-golden` | `INSTA_UPDATE=always cargo test --features test-hooks` |
| `test-external` | `cargo test --features test-hooks -- --ignored`: the `#[ignore]`d tests that need outside software (the MIME database); not in `static` |
| `lua-test` | headless Neovim + plenary-busted against the fake CLI |
| `lua-install-test` | per-manager install smoke tests (`spec/install/`) |
| `bench` | latency benchmark (§14.5); not in `static` |
| `coverage` | `cargo llvm-cov` report; not in `static` |
| `install` | developer-local install: release build → `~/.local/bin`, plus man page and completions (via `hali generate`); warns about re-login if `~/.local/bin` was just created |
| `dist` | release artifacts (§13.4) |
| `release-prepare BUMP=patch\|minor\|major` | release step 1 |
| `release-publish` | release step 2 |

Every release target accepts `DRY_RUN=1`.

### 13.3 Versioning and release flow (local, automated with `gh`)
- **Version:** one version for CLI and plugin, from `Cargo.toml`. Tags are `vX.Y.Z` (required by `vim.pack`). Releases start at **`v0.1.0`** and stay on 0.x until this spec is fully built.
- **`make release-prepare BUMP=…`:**
  1. Preflight: on `main`, clean, up to date with `origin`, `gh auth status` OK, and `make static-check` passes.
  2. Create branch `release/vX.Y.Z`.
  3. Run `cargo set-version --bump <BUMP>` (pinned `cargo-edit`).
  4. Commit, push, and run `gh pr create`.
- **`make release-publish`** (after the PR is merged):
  1. Preflight: `main` contains the bump, the tag doesn't exist yet, and the tree is clean.
  2. Create the annotated tag and run `make dist`.
  3. Push the tag and run `gh release create vX.Y.Z --verify-tag --generate-notes dist/*`.
- **Release notes** come from `--generate-notes`. There is no `CHANGELOG.md`.
- **Checklist for `v0.1.0`:** run `make bench` natively and record the results in ADR-0001.

### 13.4 Artifacts (`make dist`, built locally for `x86_64-unknown-linux-gnu`)
- **`hali-vX.Y.Z-x86_64-linux.tar.gz`:** binary, man page, bash/zsh/fish completions, README, LICENSE, and `install.sh` (shellcheck-clean). `install.sh` installs into `~/.local/bin`, `~/.local/share/man/man1` and the completion dirs, warns about re-login, and runs `hali skill install` if `~/.claude` exists.
- **`SHA256SUMS`**

### 13.5 Fresh-machine install
```sh
gh release download -R natemarks/hali -p 'hali-*-x86_64-linux.tar.gz' -p SHA256SUMS \
  && sha256sum -c --ignore-missing SHA256SUMS && tar xzf hali-*.tar.gz && ./hali-*/install.sh
```
Then add the plugin with lazy.nvim or `vim.pack`, pinned to the same tag.

---

## 14. Testing

Source: [Testing strategy for hali v1](https://github.com/natemarks/mcp-local-notes/issues/68).

### 14.1 Discipline
- **TDD at the `core` seam:** write the failing test first, make it pass, then wire the operation into `cli`.
- **Every command** gets:
  - a `core` unit test;
  - a CLI integration test (`assert_cmd` on the real binary, `tempfile` store);
  - an `insta` golden JSON snapshot.
- **Every error rule** gets a test that triggers it.
- **There is no coverage percentage gate.**

### 14.2 Determinism
- `core` takes a `Clock` and an `IdGen`.
- Binary-level tests set `HALI_TEST_NOW` and `HALI_TEST_ID_SEED`, which are **compiled in only under the `test-hooks` cargo feature**, so release binaries cannot be steered.
- `HALI_TEST_TTY=1|0` makes the CLI treat stdin and stderr as terminals (`1`) or not (`0`) when it decides whether to ask for confirmation (§9.8). Like the two above, it is compiled in only under `test-hooks`; without it, or unset, the real terminal check applies.
- `insta` redactions cover any remaining UUIDs and timestamps.

### 14.3 RDF tests
- **Round-trip:** loading and re-serializing each tool-managed file is byte-identical.
- **SHACL:** every shape has a conforming fixture and at least one violating fixture.
- **SPARQL:** each property-path query has a test over a small fixture graph.
- **Dry run:** `--dry-run`'s `changes` equal exactly what a real run writes.

### 14.4 Concurrency
About 8 overlapping reader and writer processes run on one store, each writer adding a unique Concept. The test asserts that:
- no reader ever fails to parse;
- no write is lost;
- a held lock leads to exit 3 after 5 s.

### 14.5 Benchmark (`make bench`)
- An `#[ignore]`d test: release binary, synthetic stores of 1k and 10k Resources, 30 fresh `hali find --json` processes per size, reporting p50 and p95.
- **It must run natively before `v0.1.0`.**
- p95 over 200 ms at 10k makes the on-disk storage fallback a v0.1 task.

### 14.6 Contracts
- **Plugin fixtures:** the plugin's fake `hali` returns the **CLI's golden JSON files**, so CLI output changes automatically flow into the plugin tests.
- **Schema:** every golden JSON file is validated against `hali schema <command>` (`jsonschema`).
- **Usage guide:** the drift test of §11.3.

### 14.7 Environment-dependent tests and CI
- **Outside software:** a test that needs software outside the repository, such as the MIME database (`/usr/share/mime`) for detection tests, is `#[ignore = "<reason>"]`, so `make unit` runs without it. `make test-external` runs those tests.
- **CI** (`ubuntu-latest`): installs `shared-mime-info` and runs `make test-external` in its own job, apart from `static-check`; it also installs Neovim 0.12 (via a setup action pinned to a commit), stylua and the toolchain from `rust-toolchain.toml`. `lua-install-test` clones lazy.nvim at a pinned commit. `bench` and `coverage` never run in CI.

---

## 15. Dependencies

Every dependency is pinned **exactly**. Versions are those current at spec time (2026-10-24); confirm the latest compatible releases when creating `Cargo.toml`, then pin them.

| Purpose | Crate | Version at spec time |
|---|---|---|
| RDF store + SPARQL | `oxigraph` (`default-features = false`) | 0.5.13 |
| Turtle I/O | `oxttl` | 0.2.5 |
| RDF model, RDFC-1.0 | `oxrdf` (feature `rdfc-10`) | 0.3.5 |
| SHACL | `shacl` (rudof), Native mode | 0.3.35 |
| CLI | `clap` 4 (derive), `clap_mangen`, `clap_complete` | 4.6.7 / pin at build / pin at build |
| JSON | `serde`, `serde_json` | pin at build / 1.0.154 |
| Media types | `xdg-mime` | 0.4.0 |
| XDG base dirs | `etcetera` | 0.11.0 |
| Directory walk | `ignore` | 0.4.33 |
| Identity | `uuid` (v7), `sha2` | 1.29.0 / 0.11.1 |
| Config | a TOML parser (e.g. `toml`) | pin at build |
| **dev:** CLI tests | `assert_cmd`, `tempfile` | 2.2.2 / 3.27.0 |
| **dev:** snapshots | `insta` | 1.49.0 |
| **dev:** schema checks | `jsonschema` | 0.60.1 |
| **tool:** version bump | `cargo-edit` (`cargo set-version`) | pin at build |

---

## 16. Implementation notes to confirm while building

These are known uncertainties, not open decisions.
- **User-dirs:** `etcetera` resolves the XDG *base* dirs. Resolving `XDG_DOCUMENTS_DIR` needs user-dirs support (e.g. `directories::UserDirs::document_dir`, or a small `user-dirs.dirs` parser) behind the `paths` seam.
- **`xdg-mime`** was last released in 2024-04. If it misbehaves, use the direct-database fallback in §5.1.
- **rudof `shacl` features (settled in #7):** `shacl` and `rudof_rdf` `=0.3.35` with their default `sparql` feature; without it, `sh:sparql` constraints (the label-uniqueness shape) are silently ignored. `rudof_rdf` forces oxigraph's `http-client` (`http-client-rustls-native`) and `rdf-12` features on, which `default-features = false` cannot avoid: Cargo.lock grows from 132 to 340 packages (tokio, reqwest, hyper, rustls), hand-edited files may hold RDF 1.2 triple terms, and `hali query` must use `SparqlEvaluator::without_default_http_service_handler()` so SERVICE cannot reach the network. They share the one oxigraph/oxrdf hali already pins. hali copies triples from the store into rudof and never hands it Turtle text. rudof's SPARQL evaluator can reach the network if a query uses `SERVICE`, and hali cannot disable that, so no user text may reach it: only the embedded shapes' query runs. `extensions.ttl` is `SHAPES_INVALID` if it has a triple with `sh:select`, `sh:ask`, `sh:sparql`, `sh:prefixes`, `sh:declare`, `sh:prefix` or `sh:namespace`, any triple about a named subject of the embedded shapes (extensions cannot modify core shapes), any triple whose object is a named subject of the embedded shapes (extensions cannot reuse core shapes on their own focus nodes, e.g. through `sh:node`, `sh:not`, `sh:property`, `sh:qualifiedValueShape` or an `sh:and` / `sh:or` / `sh:xone` list), or any triple whose object is an RDF 1.2 triple term (rudof prints such a term on stdout before panicking). These checks run before any rudof call. Objects naming terms defined only in `core.ttl` or the vocabulary, such as `sh:targetClass hali:Resource`, stay allowed. Besides rudof_rdf, quick-xml (SPARQL XML results) is reached through `sparql_service`, which shacl's `sparql` feature pulls in and which also depends on oxrdfio and sparesults. A panic inside rudof is caught and reported as `SHAPES_INVALID`; a Rust stack overflow inside rudof (very deep or cyclic shapes) is not a panic and still aborts the process.
- **Accepted `cargo audit` warnings (settled in #7):** `paste` 1.0.15 (RUSTSEC-2024-0436) and `proc-macro-error2` 2.0.1 (RUSTSEC-2026-0173) are unmaintained build-time proc-macros pulled in through `rudof_rdf`. They are not vulnerabilities, so `cargo audit` exits 0; they are deliberately left visible as warnings rather than ignored. The `extensions.ttl` refusal above is a denylist of what rudof 0.3.35 parses, so every bump of `shacl`, `rudof_rdf` or oxigraph needs a security re-check: SHACL-AF features such as `sh:SPARQLTarget` or `sh:validator` would not be on the list.
- **Latency figures** in ADR-0001 are proxies until `make bench` runs natively.
- **License:** not chosen yet; a `LICENSE` file must exist before the first release (the tarball includes it).

---

## 17. Future work (out of scope for v1)

These were ruled out of scope on the map. None changes a v1 decision, and each fits the design as an addition.
- **Release:** a CI release workflow on tag push (provenance), a static musl build, a `.deb`, and publishing `hali` to crates.io (a real `0.1.0`, never a placeholder).
- **Query:** full-text or semantic search, returning excerpts to agents, a live-query picker mode.
- **Model:**
  - a work layer above Resources (`dcat:Dataset` over Distributions);
  - additional Concept schemes (projects, people);
  - a per-command `prov:Activity` log;
  - an `infer` command (OWL 2 RL via `reasonable`, filling a separate graph that is never saved).
- **CLI:** `hali open`, and templates for `hali new`.
- **Plugin:** creating Resources or editing metadata from Neovim.
- **Integrations:** a Claude plugin/marketplace for the skill; `llms.txt`.
- **Sync:** across machines (opaque IRIs keep it possible).

Never in scope:
- an MCP server
- macOS or other distros (until the platform scope changes)
- a filesystem watcher/daemon
- writing metadata into cataloged files
- non-standard stores (Neo4j, a bespoke SQLite schema)
- migrating `mcp-local-notes` data
