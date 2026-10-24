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
| `dcat:byteSize` | exactly 1 | `xsd:nonNegativeInteger`. |
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
- Every relation target is a `hali:Resource`.
- Every custom relation property has a standard parent.
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
<urn:uuid:0192f0c4-7b1e-7cc3-9a43-6c1d2e8f4b10> a hali:Paper ;
    dcterms:title "Linked Data Patterns" ;
    dcat:downloadURL <file:///home/nmarks/papers/linked-data-patterns.pdf> ;
    dcat:mediaType <http://www.iana.org/assignments/media-types/application/pdf> ;
    dcat:byteSize "482113"^^xsd:nonNegativeInteger ;
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
| core ontology + SHACL shapes | `hali:Resource`, base declarations, shapes | **built into the binary** (`include_str!`); `describe` reports its version |
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
hali edit <ref> [--title T] [--add-type T] [--remove-type T] [--add-tag C] [--remove-tag C]
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
hali concept edit <C> [--add-alt L] [--remove-alt L] [--add-broader C] [--remove-broader C]
hali type add <Name> [--parent T] | list | remove <T>
hali relation add <name> --parent <standard property> | list | remove <R>

# hidden
hali generate man
hali generate completions <bash|zsh|fish>

# global flags
--json   --dry-run (every mutating command)   --agent NAME   --yes
```
**Not in v1:** `export`/`import` (the files are the export), `infer`, `open`.

### 9.2 References
- **Resource `<ref>`:** a path (absolute, or relative to the current directory, resolved to the real path) or the **full** UUID. Prefixes are not accepted.
- **Concept:** its slug or any of its labels.
- **Type:** its local name (`Paper`).
- **Relation:** a local name (`annotates`) or a prefixed standard term (`dcterms:isPartOf`).

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
  `show` adds relations (both directions, with relation names) and the catalog record.
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
  {"schema_version":1,"error":{"rule":"UNKNOWN_CONCEPT","message":"…","details":{},
   "suggested_fix":"…","suggested_command":"hali concept add ml --dry-run"}}
  ```
- **In text mode**, the output is one `error:` line plus a `hint:` line.
- **Size:** errors stay well under 10k characters.
- **Rules** (SCREAMING_SNAKE, minimum set): `UNKNOWN_CONCEPT`, `UNKNOWN_TYPE`, `UNKNOWN_RELATION`, `LABEL_TAKEN`, `POSSIBLE_MOVE`, `FORGET_BLOCKED_BY_RELATIONS`, `IN_USE`, `HASH_MISMATCH`, `NOT_A_REGULAR_FILE`, `NON_UTF8_PATH`, `SHACL_VIOLATION`, `CONFIRMATION_REQUIRED`, `READ_ONLY_QUERY`, `STORE_LOCKED`.

### 9.5 Dry run
`--dry-run` returns the same payload as a real run, plus `"dry_run": true` and a `changes` list of the triples that would be added or removed, in N-Triples form:
```json
{"op":"add","triple":"<urn:uuid:…> <http://purl.org/dc/terms/subject> <https://w3id.org/hali#ml> ."}
```
SHACL runs against the would-be graph, so violations are reported. Nothing is written.

### 9.6 Agent identity
- `--agent NAME` overrides the `HALI_AGENT` environment variable.
- With neither set, changes are attributed to the user.
- An unknown name mints a new `prov:SoftwareAgent` that `prov:actedOnBehalfOf` the user.

### 9.7 `find` semantics
- `--tag` matches the Concept **and every narrower Concept** (`skos:broader*`); `--exact` turns that off.
- `--type` matches subclasses.
- `--related-to` matches either direction, and `--via` restricts it to a relation and its sub-properties.
- Repeated filters are **AND**.
- `--title` is a case-insensitive substring match.
- **Default sort:** `modified` descending.

### 9.8 Destructive commands
- `forget`, `scan --prune`, `concept|type|relation remove` and `relink --force` ask for confirmation **only on a TTY**.
- **Without a TTY** they fail with `CONFIRMATION_REQUIRED`, suggesting `--yes`.
- Nothing ever waits on stdin.

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
- **`allowed-tools`** pre-approves **read-only commands only**: `Bash(hali describe *)`, `find`, `show`, `query`, `schema`, `validate`, `help`, and `--dry-run` invocations. Mutations still go through Claude Code's permission prompt.
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
| `unit` | `cargo test` |
| `unit-update-golden` | `INSTA_UPDATE=always cargo test` |
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
- **MIME database:** detection tests that need `/usr/share/mime` are marked so they can be skipped on machines without it.
- **CI** (`ubuntu-latest`): installs `shared-mime-info`, Neovim 0.12 (via a setup action pinned to a commit), stylua and the toolchain from `rust-toolchain.toml`. `lua-install-test` clones lazy.nvim at a pinned commit. `bench` and `coverage` never run in CI.

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
- **rudof `shacl` features:** choose them with `cargo tree`, since feature unification with `oxigraph` matters.
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
