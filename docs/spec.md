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
| `dcat:downloadURL` | exactly 1 | `<file:///…>` per RFC 8089 (the canonicalised real path, UTF-8 percent-encoded). Only `relink` changes it (§6). |
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
- A Resource is an IRI, never a blank node (`sh:nodeKind sh:IRI` on `hali:ResourceShape`). The SPARQL constraints below do not look at blank nodes, so this one reports them.
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
- **The lock file** is `<data dir>/.lock`: empty, a regular file of mode 0600, never deleted. A real run of a mutating command creates it (`O_CREAT|O_NOFOLLOW|O_NONBLOCK|O_NOCTTY|O_CLOEXEC`, then checked with `fstat`); readers and dry runs never create it, and when it is absent they read without a lock (each file is published atomically, so they still see whole files). A `.lock` that is not a regular file (a symlink, a FIFO, a directory) is `STORE_IO`.
- **Writers:** a real run of `add`, `edit`, `relate`, `unrelate`, `forget`, `refresh`, `relink` (both forms), `scan` (one root or bare) and `concept|type|relation add|rename|edit|remove` takes an **exclusive** lock before it loads the store and holds it through the compute, hashing, any confirmation prompt, the publish and (for scan) the `roots.json` write, until the process ends. Long scans, relink searches and `refresh --all` therefore block readers while they run.
- **Readers:** `describe`, `find`, `show`, `validate`, `query`, the list and show commands, and every `--dry-run`, take a **shared** lock for the duration of the load only. `schema`, `help`, `generate`, `scan --list` and `scan --forget-root` take no lock.
- **Order:** each lock is taken after the usage, config and roots checks, so §9.1's order of checks is kept.
- **Waiting:** a process tries the lock (`std::fs::File::try_lock` / `try_lock_shared`, Rust ≥ 1.89) every 10 ms to 100 ms for up to **5 s**, measured with a monotonic clock, then exits 3 with `STORE_LOCKED`: message `another hali process holds the store lock <path>; gave up after 5 s`, `details {"path":<lock file>,"reason":"timeout"}`, the fix "Wait for the other hali process to finish (it may be waiting at a confirmation prompt), then retry." and no suggested command. `reason` is `timeout` or `changed` (below). Locks are not inherited across exec (`CLOEXEC`) and the kernel frees them when their holder dies. A data dir on a network filesystem (NFS and the like) is unsupported: it is not detected, and its locks are not relied on.
- **Prompts** (§9.8) are answered with the exclusive lock held, so what the human confirms is exactly what is written and no update is lost; meanwhile other processes get `STORE_LOCKED` after 5 s. Only an interactive terminal ever waits at a prompt.
- **An absent data dir:** a writer that finds no data dir locks nothing, so a run that writes nothing still creates nothing. At its first publish it creates the dir (mode 0700) and `.lock`, takes the exclusive lock and checks that the store files are still absent; if one now exists (another process wrote first) it fails with `STORE_LOCKED`: message `another hali process wrote the store first (lock <path>); nothing was written`, `details {"path":<lock file>,"reason":"changed"}`, the same fix and no suggested command; it writes nothing. A store file that cannot be checked is `STORE_IO`.
- **Atomic publish** of each changed file, under the exclusive lock: a target that exists and is not a regular file is `STORE_IO`; whatever sits at the fixed temp path `.<name>.tmp` in the data dir is removed (a planted symlink or file itself, never its target; a directory there is `STORE_IO`); the temp is created with `O_CREAT|O_EXCL|O_NOFOLLOW|O_NOCTTY` at mode 0600 (set again with `fchmod`, whatever the umask), written and `fsync`ed, then `rename`d over the target, and the dir (opened `O_DIRECTORY|O_NONBLOCK`) is `fsync`ed. On any error before the rename the temp is removed, the old bytes stay and the run is `STORE_IO`; once the rename is done the bytes are published, so a failed `fsync` of the dir does not fail the run. A temp left by a crash is never loaded. Every mutation writes one tool-managed file, so each mutation is atomic.
- **Reads** of `vocabulary.ttl`, `resources.ttl` and `extensions.ttl`: anything there but a regular file (by `lstat`; a symlink included, `extensions.ttl` too) is `STORE_IO`; the file is opened `O_NONBLOCK|O_NOFOLLOW|O_NOCTTY` and checked again with `fstat`, so a FIFO is `STORE_IO` and never a hang; one over **256 MiB** is `STORE_IO` before it is read, and at most 256 MiB + 1 bytes are ever read.
- **The data dir** (checked by every command that loads the store, before it locks): one not owned by the effective user, or group- or world-writable (`mode & 0o022`), is `STORE_IO` naming the reason, with the fix `chmod go-w <dir>`; read and search bits for others are fine (a clone makes 0755 dirs), and a symlinked data dir is followed. hali creates dirs with mode 0700 and store files and `.lock` with mode 0600, whatever the umask.

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
| Logs, remembered scan roots and their settings (`roots.json`, §7) | `$XDG_STATE_HOME/hali/` (`~/.local/state/hali/`; an empty or relative `XDG_STATE_HOME` is ignored) |
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
dir = "~/Documents/hali"       # where `hali new` creates files; the root `hali relink` searches by default

[open]
editor = ["text/html"]         # MIME patterns forced into Neovim
system = ["text/csv"]          # MIME patterns forced to the OS opener

[scan]
exclude = ["*.tmp", "Downloads/**"]
include_hidden = false
max_size = "1GiB"
```
The `[user]` and `[vocabulary]` key names were not fixed on the map and are proposed here. `[files] dir` may start with `~`, which is `$HOME`; without it, `files.dir` is `$XDG_DOCUMENTS_DIR/hali` from user-dirs (`$XDG_CONFIG_HOME/user-dirs.dirs`), else `~/Documents/hali`. The others come from their deciding tickets.

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
- **`hali refresh [<ref>…|--all]`** re-hashes and updates `spdx:checksum`, `dcat:byteSize` and `dcterms:modified`: named Resources are always re-hashed; `--all` hashes only Stale ones; Missing ones are reported. `scan` refreshes everything under its root.
- **`hali relink [--root DIR]…`**: for each Missing Resource, walk the search roots, compare sizes first, and hash only the files whose size matches.
  - Exactly one match: update `dcat:downloadURL`.
  - Several matches: report them and change nothing.
  - Default roots: `files.dir` (`[files] dir`, else the user-dirs documents dir plus `hali`, else `~/Documents/hali`; the first one defined is the only one, and it is skipped silently when it is not a usable directory), then the remembered scan roots (§7) in byte order, without repeats; one that is gone, not a directory, unreadable or hali's data dir is skipped silently. Relink walks every root with its own settings, never with the ones a root was remembered with. `--root` replaces every default, the remembered roots included.
- **`hali relink <ref> <path>`**: an explicit move. If the content hash differs (the file was moved and edited), it is refused with `HASH_MISMATCH` unless `--force`, which also refreshes the hash.
- **Duplicates:**
  - Same content at different paths is allowed.
  - Re-`add` of a cataloged path is **idempotent** and returns the existing Resource.
  - `add` of a new path whose hash matches a *missing* Resource is refused with `POSSIBLE_MOVE`, suggesting `relink`; `--new` overrides.
- **Paths:** `add` stores the **real path** (symlinks resolved).
- **What can be a Resource:** **regular files only** (otherwise `NOT_A_REGULAR_FILE`), at any absolute local path, including outside `$HOME`. **Non-UTF-8 paths are rejected** (`NON_UTF8_PATH`).
- **Hashing:** SHA-256 only, recorded via `spdx:algorithm`. Files are hashed in full at `add`/`refresh`/`scan` and by an explicit `relink`; a `relink` search reads a candidate only up to the recorded size + 1 bytes.

---

## 7. Scan

Sources:
- [scan ignore rules and pruning](https://github.com/natemarks/mcp-local-notes/issues/67)
- §6

- **Walker:** the `ignore` crate (ripgrep's), sequential, and it only reads.
  - Skips hidden files and directories by default (the root itself is always walked).
  - Ignore files: `.gitignore`, `.git/info/exclude` and the global gitignore (git's `core.excludesFile`, else `$XDG_CONFIG_HOME/git/ignore`) apply only inside a git repository (a directory holding `.git` or `.jj` counts as one); `.ignore` and **`.haliignore`** (gitignore syntax) apply in any directory, including the root's parents. `--no-ignore` turns off exactly these five, and nothing else. An invalid glob line in one of the per-directory sources (`.gitignore`, `.ignore`, `.haliignore`, `.git/info/exclude`) still lets the file's valid lines apply, and the file is reported as skipped (`bad_ignore_file`); the global gitignore is not covered (see "What fail-closed does not cover").
  - **Ignore files fail closed:** before the walk enters a directory it checks each `.gitignore`, `.ignore` and `.haliignore` in it, `.git/info/exclude` when `.git` is a directory (or a symlink to one) and, for a `.git` file (which names the repository's `info/exclude`), the file itself, the `commondir` it leads to and `<common>/info/exclude`, with `lstat` and a bounded, non-blocking read. One that is not a regular file, is over 256 KiB (262,144 bytes), has `{` nesting deeper than 100 levels (comment lines are not checked, and every leading byte-order mark on the first line is stripped first, as the crate does), is not valid UTF-8 or cannot be read is reported as skipped (`bad_ignore_file`, at that file) and its directory is not walked; for the root or a parent of the root, nothing is walked. A `.git` that is a FIFO, a symlink to a file, or dangling is rejected the same way. A matcher the `ignore` crate fails to build for a file that passed those checks (a regex size or nesting limit; the crate would drop every rule of that file) also fails closed, reported at the candidate file. A FIFO or a device named as an ignore file is therefore never opened.
  - **Live ignore-rule budget:** the `ignore` crate keeps the compiled matcher of every ancestor directory alive while the walk is under it, so memory grows with depth (about 170 MB for one ignore file near the 256 KiB cap, gigabytes for a few nested directories). The walk therefore keeps a running total of the bytes of ignore files accepted along the current path (the root's parents included; a file is counted once for each directory that reads it, so an `info/exclude` shared by several worktrees is counted once per directory, which matches the crate; the files of one directory add up against the budget) and reports a candidate that would take it over 512 KiB (524,288 bytes) as skipped (`bad_ignore_file`, at that file); its directory is not walked, and for the root or a parent of the root, nothing is walked. A file counts only while the walk is under its directory, so siblings are not charged for each other's files.
  - **CPU in deep trees:** the `ignore` crate matches every entry against the matchers of every ancestor directory, so time grows with depth × files per level × ignore files (1,900 nested directories with three 9-byte ignore files each took 63 s against 1.2 s; memory stays small). There is no depth cap. This is accepted: `hali scan` (#22) must include a time bound in its tests, and the follow-up that reads ignore files with hali's own matcher, with a depth or live-matcher cap, is the fix.
  - **Caller notes:** `walk()` checks the ignore files of the root and its parents when it is called, but the `ignore` crate reads them at the first `next()`, so callers must iterate at once. `WalkOptions::new` and `walk()` must not be called on a thread with a tiny custom stack (a debug build aborts on a 100-level pattern on a 64 KiB stack; 256 KiB is fine). The budget assumes the sequential depth-first walk and would silently break under a parallel walk.
  - **The remaining race:** between this check and the crate's own read of the file, a race remains, and it is not narrow: whoever controls the tree controls how wide it is. An attacker who wins it can swap a checked regular file for a FIFO (the walk hangs) or for a symlink to `/dev/zero` (the process aborts); a symlink to a file outside the root is opened, but its contents are used only as patterns and never output. Those are denials of service. If a file changes after the check, the crate also silently throws away I/O errors from ignore files, so a directory can be walked with none, or only the first lines, of its rules, and nothing is reported: rules can be lost silently. This is accepted. The follow-up, reading and applying ignore files with hali's own bounded matcher, closes it and is the first task after `hali scan` (#22); `hali scan` leaves the store unchanged if the process dies mid-scan.
  - **Parents and `!` lines:** ignore files in the root's parents are honoured (git semantics), including world-writable ones such as `/tmp`; a world-writable parent such as `/tmp` that holds large ignore files can use the whole budget for every root below it. A `!` line in an ignore file (in the root, a subdirectory or a parent) can override the hidden rule; user excludes, the built-in excludes and the data dir cannot be overridden. This is accepted; a follow-up may honour only parent files owned by the user.
  - **What fail-closed does not cover:** the scanned tree and the root's parents, not git's own configuration (`~/.gitconfig`, `$XDG_CONFIG_HOME/git/config`, `GIT_CONFIG_GLOBAL`, `GIT_CONFIG_SYSTEM`, `/etc/gitconfig`) or the file `core.excludesFile` names. Those are user-controlled. A FIFO at one of the git config files blocks the walk (`walk()` does not return); the file `core.excludesFile` names is checked with `is_file()` first, so a FIFO there is never opened. The `ignore` crate drops errors in the global gitignore silently: an invalid line, or a file it cannot build, is never reported, and its rules vanish.
  - **The root and the data dir:** the `ignore` crate never applies the built-in and data-dir filter to the root itself, so a root that is the data dir, or inside it, is walked; `hali scan` refuses such a root.
  - Does not follow symlinked directories (nothing under one is scanned, and it is not reported). A symlinked file is resolved to its real path: inside the root it is silent, because the real file is walked directly and is subject to every filter (a link never bypasses an exclude, an ignore file or the hidden rule); outside the root it is skipped (`outside_root`); a broken or looping one, or one that leads to something that is not a regular file, is skipped (`not_regular`). A skipped symlink is reported at its own path.
  - Skip reasons: `too_large` (over `max_size`), `unreadable` (a directory the walk cannot read, a symlink whose target cannot be reached for lack of permission, or an entry whose `stat` fails), `not_regular` (FIFOs, sockets, devices and broken symlinks, which are never opened), `non_utf8` (a directory with such a name is reported once and not entered), `outside_root` and `bad_ignore_file`. A skipped entry never stops the walk.
  - The walker does not open regular files other than ignore-file candidates (opened by the check above and by the `ignore` crate): whether one can be read is left to `inspect`, so an unreadable regular file is still listed.
  - Hidden, ignored, built-in and pruned directories are listed one level by the `ignore` crate before being skipped, but nothing under them is yielded or entered. The walk is built from the canonical root.
- **Built-in excludes:** directories named `.git`, `node_modules`, `target`, `__pycache__` and `venv`, at any depth, and hali's data dir and state dir (§4) are never entered, even with `--include-hidden` or `--no-ignore`. User excludes (`[scan].exclude`, `--exclude GLOB`) use gitignore syntax relative to the root: a pattern with no `/` matches a name at any depth, one with a `/` is anchored at the root; an invalid glob, an empty or whitespace-only one, one starting with `!`, or one with `{` nested deeper than 100 levels (the same bound as for ignore files), is rejected.
- **Size limit:** files over `max_size` (default 1 GiB) are skipped and reported. An explicit `add` has no limit and bypasses all ignore rules.
- **What gets registered:** every regular file that passes the filters, of any media type, **untagged and untyped**, with its detected media type. `--media-type` (repeatable, wildcards allowed) narrows a scan.
- **`--dry-run`** prints a summary first: counts by media type, and new / refreshed / relinked / missing / skipped.
- **Remembered settings:** every successful real `scan <root>` remembers its root (the real path) with the `--include-hidden`, `--no-ignore`, `--exclude GLOB`, `--max-size` and `--media-type` given on the command line (not values merged with the config), in `roots.json` in the state dir (§4). A scan that changes nothing remembers too; a `--dry-run`, a scan that fails and a declined prune do not. Rescanning a root replaces its settings.
  - **The file:** `{"version":1,"roots":[{"root","include_hidden","no_ignore","exclude":[…],"max_size":<bytes>|null,"media_types":[…]}…]}` and a newline, compact, the roots sorted in byte order; the state dir is mode 0700 and the file 0600, written to a temp file and renamed.
  - A bare `hali scan [--prune [--force]]` re-scans every remembered root with its own settings (the config merged in as for one root) as one run: one mutation, one SHACL check, ids minted in byte order of all the files registered, and move matching one-to-one across every root, so a file that moved from one root to another is relinked. Prune is decided after the moves in every root. A remembered root that is gone, not a directory or unreadable is listed as `unavailable`, its Resources are never pruned, and it stays remembered. With no remembered roots a bare scan exits 0 with empty results. Scan flags without a root are a usage error (exit 2). A bare scan never rewrites `roots.json`.
  - `hali scan --list` shows the roots and their settings, read-only.
  - `hali scan --forget-root <root>` stops remembering a root without touching its Resources: only `roots.json` changes. The root is matched on its real path, or on the argument made absolute when it no longer resolves, so a deleted root can be forgotten; one that is not remembered is a usage error with the hint `hali scan --list`.
  - `--list` and `--forget-root` stand alone: any other argument with either is a usage error.
  - **The state dir** is never walked or cataloged by scan or relink, as the data dir. A root that is the state dir or inside it (compared on real paths) is refused as the data dir is: a usage error for `scan <root>` and `relink --root`, `unavailable` in a bare scan, and skipped silently among relink's default roots.
- **Moves within a root:** a new file whose hash matches exactly one Missing Resource is **relinked automatically** and listed in the summary. With several candidates, they are reported and nothing changes.
- **Vanished files** are only reported by default. `--prune` forgets them, with `forget`'s rules: `--yes` is needed without a TTY, and Resources with inbound relations are skipped unless `--force`. Files are never deleted.
- **Filters gate only new registrations:** the hidden rule, ignore files, built-in and user excludes, `max_size` and `--media-type` decide only which uncataloged files a scan registers; an already-cataloged file under the root is never un-cataloged or flagged by them, and is checked with `refresh`'s `stat` rule whatever they say (§9.1 **Scan:**).
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

One `hali` binary (clap 4, derive). Its help, usage errors and version always call it `hali`, whatever name it was started under: the name it was run as (`argv[0]`) is never printed.

### 9.1 Commands
```
# orientation
hali describe [--brief]                  # vocabulary, types, relation kinds, counts, core ontology version,
                                         # store path; skill install state arrives with #28
hali schema [<command>]                  # JSON Schema of each command's --json output and of the error envelope
hali help agents [--snippet]             # embedded agent usage guide; --snippet prints an AGENTS.md block
hali skill install [--agents] [--print]

# Resources
hali add <path>… [--title T] [--type T]… [--tag C]… [--new]
hali new "<title>" [--type T]… [--tag C]…
hali scan [<root>] [--include-hidden] [--no-ignore] [--exclude GLOB]… [--max-size SIZE]
          [--media-type M]… [--prune [--force]] [--list] [--forget-root <root>]
hali find [--tag C]… [--exact] [--type T]… [--title TEXT] [--media-type M]
          [--related-to REF [--via REL]] [--missing] [--stale]
          [--sort modified|added|title] [--limit N] [--offset N] [--fields F,…] [--path-only]
hali show <ref> [--path-only]            # one Resource incl. relations and catalog record
hali edit <ref> [--title T] [--add-type T]… [--remove-type T]… [--add-tag C]… [--remove-tag C]…
hali relate <ref> <relation> <ref>
hali unrelate <ref> <relation> <ref>
hali forget <ref> [--force]
hali refresh [<ref>…|--all]              # exactly one of the two; named: always re-hashed; --all: Stale only
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

**Refresh:** `refresh <ref>…` or `refresh --all` re-records what a Resource's file is now (§6). Exactly one of the two is required; neither, or both, is a usage error (exit 2), raised by the argument parser before the store is read. Refs are Resource references (§9.2: a path or a full UUID), resolved in argument order before any file is read, the first unknown one being `UNKNOWN_RESOURCE`; a Resource named twice counts once. **Which Resources are hashed:** every Resource named by a ref whose file is there is re-hashed, whether Stale or not; `--all` `stat`s every Resource and hashes only the Stale ones, using the same `stat` comparison as `find` (size, or mtime in whole seconds, differs from the record). Nothing but `add`, `refresh`, `scan` and `relink` ever hashes a file. **Missing:** a Resource whose recorded path has no regular file by `stat` (gone, or now a directory, FIFO, device, socket or broken symlink; the same rule as `find`'s `missing`) is never opened; it is listed in `missing`, nothing about it changes, and this is not an error (exit 0). **Errors:** when `stat` sees a regular file that cannot be hashed, a symlink at the recorded real path or a file swapped for a non-regular one after the `stat` is `NOT_A_REGULAR_FILE` (`kind` `other`), a file that cannot be opened or read is `FILE_UNREADABLE`, and a file that disappeared after the `stat` counts as missing. Refresh is all-or-nothing: the first error, in processing order, aborts it and nothing is written; a store that would not conform is `SHACL_VIOLATION`. It adds no error rule. **Writes** (`resources.ttl` only): the Resource's `dcat:byteSize`, `dcterms:modified` (the file's mtime, whole seconds, UTC) and `<id>#sha256 spdx:checksumValue` are replaced with what was just read, and `<id>#sha256 spdx:algorithm` is kept; any other `spdx:checksum` link of the Resource is replaced by the link to `<id>#sha256`, the other node itself being left alone, as `forget` does. The media type, title, Types, tags and relations are never touched, `prov:wasAttributedTo` is unchanged and `--agent` has no effect. When anything changes, the catalog record's `dcterms:modified` is set to now, as `edit` does; a Resource whose triples already match is unchanged (`changes []`), and if nothing changes at all `resources.ttl` is not rewritten. All changes form one mutation with one SHACL check. A file modified during the hash records the pre-read mtime, so the next `stat` shows it Stale again. The file itself is never written. **Trust:** the paths `refresh` opens are the ones the store names (`dcat:downloadURL`), and the hash, size and mtime it reads are written back into the store. A store is trusted input, like a shell rc file: do not run `refresh --all` on a store that came from someone you do not trust (for example a shared repository), because it would record the existence, size and SHA-256 of any readable file that store names. Restricting `--all` to known roots is tracked separately. **Output:** `{"schema_version":1,"refreshed":[<Resource record>…],"missing":[<Resource record>…],"unchanged":<n>}`, the keys in that order. The records are observed after the change, so a refreshed one has `stale:false` and a missing one `missing:true`; the lists are in argument order for refs and in id order for `--all`; `unchanged` counts the Resources considered that changed nothing, hashed or not. `--all` on an absent or empty store prints empty results (`unchanged` 0), exits 0 and creates nothing. `--dry-run` adds `dry_run` and `changes` (§9.5) and does hash, because reading a file is not a write. **Text:** one `refreshed|would refresh resource <id> "<title>" <path>` line per refreshed Resource, then one `missing resource <id> "<title>" <path>` line per missing one, then `unchanged <n>`; every value is escaped for the terminal. `refresh` is not destructive (§9.8): it never asks, never reads stdin and ignores `--yes`.

**Relink:** `relink [--root DIR]…` (a search) or `relink <ref> <path> [--force]` (an explicit relink) repairs Resources whose file moved (§6). **Usage errors** (exit 2, before the store is read): `<ref>` without `<path>`, more than two words, `--root` with `<ref> <path>` and `--force` in a search are each a usage error, as is a `--root` that is not a directory (`relink root <path> is not a directory`) or that is hali's data dir or inside it (`relink root <path> is hali's data dir or inside it`) or hali's state dir or inside it (`relink root <path> is hali's state dir or inside it`); a `--root` that is not UTF-8 is `NON_UTF8_PATH` and one that cannot be read is `FILE_UNREADABLE`. **Roots:** each `--root` is made absolute against the current directory and resolved to its real path; `roots` lists them in argument order without repeats, and `--root` replaces the defaults. With no `--root` the default roots are `files.dir` (§4.1): `[files] dir` (a leading `~` is `$HOME`), else `XDG_DOCUMENTS_DIR` from user-dirs (`$XDG_CONFIG_HOME/user-dirs.dirs`, a `$HOME/…` or absolute value) plus `/hali`, else `~/Documents/hali` (the first one defined is the only one), then the remembered scan roots (§7) in byte order, without repeats; one that is not a usable directory is skipped silently, and with none left `roots` is `[]`. Every root is walked with relink's own settings, not the ones a root was remembered with. A `roots.json` that cannot be used, and a state dir that cannot be resolved, are `STORE_IO` or `STORE_UNPARSEABLE` (exit 3) as for scan, before the store is read; with `--root` it is not read. A `[files] dir` of the wrong type is `CONFIG_INVALID` (exit 3). hali's data dir is never searched: a root never enters it, and a default root that is it or inside it is skipped silently. Nor is the state dir: a root never enters it, and a default root that is the state dir or inside it is skipped silently. **Search:** a Missing Resource is one whose recorded path has no regular file by `stat` (`find`'s `missing` rule). One recorded as empty (byte size 0) is never matched, because every empty file has the same hash; it is listed as unmatched. Each root is walked once with §7's walker (hidden names and ignored files are not walked, and walk skips are not reported). A file is a candidate unless it is empty or its real path is one that some Resource records; a path found under several roots counts once. Size first: a candidate is hashed only if a Missing Resource has its size, once however many Resources it could match, and the read is capped at that size + 1 bytes, so a file that grew is no match and a huge file is never read in full. A candidate matches a Resource when its byte size and SHA-256 are the record's. Matching is one-to-one: a Resource is relinked only when exactly one candidate matches it and no other Missing Resource matches that candidate; otherwise it is ambiguous, with the matching files as its candidates, and nothing about it changes. A Resource with no match is unmatched. A search is not destructive: it never asks, never reads stdin and ignores `--yes`. When nothing is missing it writes nothing and creates no data dir. **Explicit:** `<ref>` is a Resource reference (§9.2; an unknown one is `UNKNOWN_RESOURCE`), resolved before the file is read; any Resource can be relinked, Missing or not. A `<path>` whose real path another Resource records is a usage error (exit 2, nothing written), with or without `--force`; a Resource's own path is fine. `<path>` is resolved to its real path, which is what is recorded; nothing there or not a regular file is `NOT_A_REGULAR_FILE`, and a file that cannot be read is `FILE_UNREADABLE`. The file is hashed in full. If its size or SHA-256 differs from the record it is `HASH_MISMATCH` (exit 1, details `{"resource","path","expected","actual"}`, the two SHA-256s; suggesting `hali relink <id> <path> --force`), unless `--force` is given, which also records the new `dcat:byteSize` and checksum value and sets `refreshed`. A forced relink of other content is destructive and confirms as §9.8 says (`CONFIRMATION_REQUIRED` without terminals); on the same content `--force` changes nothing more and never asks. `--yes` confirms a `--force`; it does not force. **Writes** (`resources.ttl` only): a relink replaces the Resource's `dcat:downloadURL` with the URL of the new real path and its `dcterms:modified` with the file's mtime, and sets the catalog record's `dcterms:modified` to now; nothing else changes. Every relink of a run is one mutation with one SHACL check, and a store that would not conform is `SHACL_VIOLATION` with nothing written. Neither the old file nor the new one is ever written, and if nothing changes `resources.ttl` is not rewritten. **Output** of a search: `{"schema_version":1,"roots":[<real path>…],"counts":{"relinked","ambiguous","unmatched"},"relinked":[{"resource":<Resource record>,"from":"<old path>"}…],"ambiguous":[{"resource":<Resource record>,"candidates":[<path>…]}…],"unmatched":[<Resource record>…],"truncated":<bool>}`, the keys in that order. Each list is in id order and holds at most 100 entries, `candidates` at most 10, in byte order of the path; the counts are always exact and `truncated` is true when a list was cut. Of an explicit relink: `{"schema_version":1,"resource":<Resource record>,"from":"<old path>","refreshed":<bool>}`. The records are observed after the change. `--dry-run` adds `dry_run` and `changes` (§9.5), does hash and never asks. **Text:** `root<TAB><root>` per root; `relinked|would relink resource <id> "<title>" <old path> -> <new path>` per relinked Resource; `ambiguous resource <id> "<title>" <path>` per ambiguous one, each followed by `candidate <path>` lines; `unmatched resource <id> "<title>" <path>` per unmatched one; then `counts<TAB><n> relinked, <n> ambiguous, <n> unmatched`, and `truncated<TAB>true` when a list was cut. An explicit relink prints its one `relinked` line. Every value is escaped for the terminal. **Move detection on add:** `add` of a path that no Resource records, of a non-empty file with the byte size and SHA-256 of a Missing Resource, is refused with `POSSIBLE_MOVE` (exit 1, details `{"path","resources":[<ids, sorted>]}`, suggesting `hali relink <id> <path>`, or bare `hali relink` when several Resources match). It is checked after the paths resolve and before Type and Concept references; a multi-path `add` names the first offender in argument order and adds nothing; `--dry-run` reports the same error; and `add --new` skips the check.

**Scan:** `scan <root> [--include-hidden] [--no-ignore] [--exclude GLOB]… [--max-size SIZE] [--media-type M]… [--prune [--force]]` registers the files under one Scan root, refreshes the changed ones, relinks the ones that moved and reports the vanished ones, or with `--prune` forgets them (§7); `scan [--prune [--force]]` does the same for every remembered root; `scan --list` and `scan --forget-root <root>` show and change the remembered roots. **Remembered roots:** they live in `roots.json` in the state dir (`$XDG_STATE_HOME/hali/`, else `$HOME/.local/state/hali/`; an empty or relative `XDG_STATE_HOME` is ignored), outside the store. A real `scan <root>` that succeeds (changing something or not, after its write and any confirmation) remembers its root's real path with the flags given on the command line, replacing that root's settings; a `--dry-run`, a failed scan and a declined prune remember nothing. Before it asks or writes the store, a real scan creates the state dir (mode 0700) if needed and checks that it can write there; when it cannot, the scan is `STORE_IO` (exit 3) and nothing is written, the store and any prune included. A state dir that cannot be resolved is `STORE_IO` (exit 3); a `roots.json` that cannot be read, is not a regular file (a symlink, a FIFO or a directory; it is never followed or opened blocking), is over 1 MiB, is not UTF-8, or is not of §7's shape (another `version`, a relative or empty root, a field of the wrong type) is `STORE_IO` or `STORE_UNPARSEABLE` naming the file (exit 3); both come before the store is read and nothing is written. **Bare scan:** every remembered root, in byte order, is resolved to its real path and walked with its own settings and the config, and all of them are scanned as one run (one mutation, one SHACL check, ids minted in byte order of every file registered; a path under several roots counts once). Move matching is one-to-one across all the roots, and prune is decided after the moves, over the Missing Resources under any root scanned. A root that is gone, not a directory, unreadable, or hali's data dir or state dir or inside one, is `unavailable` (`{"root","reason"}`, `reason` a short text): its Resources are never pruned, and it stays remembered. A bare scan never rewrites `roots.json`. With no remembered roots it exits 0 with empty results and writes nothing. `--include-hidden`, `--no-ignore`, `--exclude`, `--max-size` and `--media-type` without a root are a usage error (exit 2). Its output is the single-scan object with `"roots":[<real path>…]` (the roots scanned, in byte order) in place of `"root"`, followed by `"unavailable":[…]` (in byte order of the root); `schema_version` stays 1. In text, `root<TAB><root>` per root scanned and `unavailable<TAB><root><TAB><reason>` per unavailable one come first. A bare prune asks `forget <n> missing resources under <k> scan roots? [y/N] ` (§9.8). **`--list`:** read-only; `{"schema_version":1,"roots":[<entry>…]}`, each entry as `roots.json` holds it, in byte order; text is one line per root, the root then a tab-separated `--include-hidden`, `--no-ignore`, `--exclude=<glob>`…, `--max-size=<bytes>` and `--media-type=<pattern>`… for each setting that is not the default; no roots print nothing. **`--forget-root <root>`:** matched on the real path when it resolves, else on the argument made absolute (trailing `/` dropped), so a deleted root can be forgotten; it changes only `roots.json` (`resources.ttl` stays byte-identical, no Resource is forgotten) and never asks. Output `{"schema_version":1,"root":<entry as it was>}`; `--dry-run` adds `"dry_run":true,"changes":[]` and writes nothing. A root that is not remembered is a usage error (exit 2) whose message points at `hali scan --list`. `--list` and `--forget-root` take no other argument (a root, `--prune`, `--force` or a scan flag); either with anything else is a usage error (exit 2) before the config, the state and the store are read. **Order of checks:** the argument parser (a bad `--max-size`, which takes `^[0-9]+(B|KiB|MiB|GiB|TiB)?$` with the suffix case-insensitive and no suffix meaning bytes, binary units only, up to `u64` bytes; a bad `--media-type`, which takes `find`'s patterns), then `--force` without `--prune` (a usage error, `--force is for --prune; a scan without --prune forgets nothing`, before the config and the store are read), then the config (`CONFIG_INVALID`, exit 3: a `[scan]` key of the wrong type, a bad `max_size` or a bad `exclude` pattern), then the `--exclude` globs (§7's rules; a bad one is a usage error), then the root, then the store (exit 3), then the walk, hashing, one SHACL check, the confirmation of a prune and the write; nothing is written when any of them fails. **Root:** made absolute against the current directory and resolved to its real path (a symlink root is scanned at its target), reported as `root`. Not UTF-8 is `NON_UTF8_PATH`; nothing there, or not a directory, is a usage error (`scan root <path> is not a directory; use hali add for a file`); a directory that cannot be read, or a root whose path cannot be resolved for lack of permission (such as one under a parent that cannot be searched), is `FILE_UNREADABLE`. A root that is hali's data dir or inside it (compared on real paths) is a usage error (`scan root <path> is hali's data dir or inside it`), as is one that is the state dir or inside it (`scan root <path> is hali's state dir or inside it`); a data dir or state dir inside the root is never entered, so scan never catalogs its own store or state. **Walk:** §7's walker, run to the end on the calling thread before anything is hashed; `--include-hidden` or `[scan] include_hidden = true` walks hidden names, `--no-ignore` turns off the ignore files, and the `--exclude` globs add to `[scan] exclude`. **New files** (no Resource records their real path): one with `byte_size > max_size` is skipped `too_large` and never opened (`--max-size`, else `[scan] max_size`, else 1 GiB); with `--media-type` (repeatable, ORed), one whose file name alone gives a media type that matches no pattern is never opened, and one whose detected media type matches none is not registered and not reported. The rest are hashed and, unless they are a move (below), registered with exactly the triples `add` writes (untyped, untagged, the default title, the catalog record's `issued` = `modified` = now, read once for the scan, the user Person minted once if absent), ids minted in byte order of the real paths of the files registered (a relinked or ambiguous file takes none). One that vanished before it was opened is dropped silently, one that turned non-regular is `not_regular`, one that cannot be read is `unreadable`. **Cataloged Resources under the root** (the recorded path is the root or starts with `<root>/`, with no empty, `.` or `..` segment) are `stat`ed with `refresh`'s rule whatever the filters say: no regular file there is `missing` (listed, never changed or removed), a Stale one is hashed and updated exactly as `refresh --all` does, the rest are unchanged and never opened; a Stale one that cannot be hashed is skipped (`unreadable` or `not_regular`) and left alone. **Moves:** a new file that would be registered and is not empty is a candidate; one with the byte size and SHA-256 of exactly one Missing Resource, store-wide (one outside the root is `stat`ed only when a new file has its size and SHA-256, and at most once), with no other Missing Resource having that content, relinks that Resource exactly as a `relink` search does (`dcat:downloadURL` and `dcterms:modified` replaced, the catalog record's `modified` set to now) and is not registered. Matching is one-to-one: when several candidates have a Missing Resource's content, or several Missing Resources a candidate's, those Resources are `ambiguous`: nothing changes for them, their candidate files are not registered (so `hali relink <id> <path>` can settle them) and they are never pruned. Empty files, and Missing Resources recorded as empty, never match; a new file whose content only present Resources have is registered as usual. **Prune:** with `--prune`, every Resource under the root still Missing after the moves, and not ambiguous, is forgotten with `forget`'s removals (a format node only when no Resource that stays uses it), in the same mutation; one with inbound relations from another Resource, in any graph and including from another Resource the scan forgets, is kept (it stays in `missing`) unless `--force`, which also removes the triples in resources.ttl that point at it; an inbound relation held only in extensions.ttl then makes the store not conform, so `--force` fails with `SHACL_VIOLATION` and nothing is written. A real run that would forget at least one Resource asks first (§9.8); without both terminals and without `--yes` it is `CONFIRMATION_REQUIRED` with nothing written, and declining writes nothing at all, adds, refreshes and relinks included. A prune that would forget nothing never asks. Files are never written, moved or deleted. **Atomicity:** per-file problems never abort the scan (exit 0, with them in `skipped`); all changes are one mutation with one SHACL check, and a store that would not conform is `SHACL_VIOLATION` with nothing written. If nothing changes, `resources.ttl` is not rewritten and no data dir is created. Nothing is ever written under the root except hali's own data dir when it lies inside the root (that data dir is pruned from the walk), and scan adds no error rule. **Output:** `{"schema_version":1,"root":"<real root>","counts":{"added","refreshed","relinked","ambiguous","unchanged","missing","pruned","skipped"},"media_types":{"<type/subtype>":<n>…},"added":[<Resource record>…],"refreshed":[…],"relinked":[{"resource","from"}…],"ambiguous":[{"resource","candidates":[<path>…]}…],"missing":[…],"pruned":[…],"skipped":[{"path","reason"}…],"truncated":<bool>}`, the keys in that order, every key always present. `from` is the path a relinked Resource was recorded at; `candidates` are at most 10 paths, in byte order. `missing` lists the Resources under the root still cataloged and Missing after the run (ambiguous ones and ones `--prune` kept included); `pruned` the forgotten Resources' records as they were. `media_types` counts the added Resources, its keys sorted. `reason` is `too_large`, `unreadable`, `not_regular`, `non_utf8`, `outside_root` or `bad_ignore_file`, and a `path` that is not UTF-8 is shown lossily. Each list is in byte order of the path (the new path of a relinked Resource) and holds at most 100 entries; the counts are always exact and `truncated` is true when a list was cut. The records are observed after the change. `--dry-run` adds `dry_run` and `changes` (§9.5) and does hash, so `changes` is what the real run writes. **Text:** `name<TAB>value` lines for `root`, `added`, `refreshed`, `relinked`, `ambiguous`, `unchanged`, `missing`, `pruned` and `skipped`; then `media_type<TAB><type><TAB><n>` per media type; then `added|would add resource <id> "<title>" <path>`, `refreshed|would refresh resource …`, `relinked|would relink resource <id> "<title>" <old path> -> <new path>`, `ambiguous resource <id> "<title>" <path>` each followed by its `candidate <path>` lines, `missing resource …`, `forgot|would forget resource <id> "<title>" <path>` and `skipped <reason> <path>` lines, capped as in JSON, then `truncated<TAB>true` when a list was cut; then the `+`/`-` lines under `--dry-run`. Every value is escaped for the terminal. **Progress:** only when stderr is a terminal and `--json` is off, one line on stderr, `scanning: <seen> files, <hashed> hashed`, rewritten in place at most 10 times a second, ending with the totals and cleared before the output; it names no file. Without `--prune` a scan is not destructive (§9.8): it never asks, never reads stdin and ignores `--yes`.

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

**Describe:** `describe [--brief]` is the one-call orientation, read-only (§9.5) and needing no store: it reports the core ontology version, the store path, the counts and, unless `--brief`, every Concept, Type and relation kind. `--json` prints `{"schema_version":1,"core_version":"0.1.0","store":"<data dir>","counts":{"resources","concepts","types","relation_kinds","relations"},"concepts":[<Concept>…],"types":[<Type>…],"relation_kinds":[<relation kind>…]}`, the keys in that order. Each list equals the `results` of `concept list`, `type list` and `relation list` without paging (the records unchanged, labels in the configured language, sorted by label, then id), so there is no `--limit`, `--offset` or `truncated`. **`--brief`** prints the same object without `concepts`, `types` and `relation_kinds`; every other key is identical to the full form. `core_version` is the lexical value of `owl:versionInfo` of `<https://w3id.org/hali>` in the core graph only (a triple about that IRI in `extensions.ttl` never changes it); the binary version is not reported. `store` is the data dir exactly as §4 resolves it (`HALI_DATA_DIR` as given, else `$XDG_DATA_HOME/hali`, else `$HOME/.local/share/hali`), with non-UTF-8 bytes replaced, not canonicalised. The counts: `resources` is the named subjects with `rdf:type hali:Resource` in `resources.ttl` (the `total` of an unfiltered `find`); `concepts`, `types` and `relation_kinds` are the `total` of the three list commands (the four standard relations are included, so an empty store has 4); `relations` is the distinct triples (subject, predicate, object), in any graph (`extensions.ttl` included), whose predicate is a relation kind, dangling ones included, a triple held in two graphs counting once. **An absent or empty data dir** is an empty store: exit 0, empty stderr, counts `{"resources":0,"concepts":0,"types":0,"relation_kinds":4,"relations":0}`, `concepts` and `types` `[]`, `relation_kinds` the four standard relations, byte-identical stdout for both; `describe` never creates the data dir or any file, and there is no `exists` flag. A hand-edited but parseable store is described as it is, without validation. It adds no error rule: `CONFIG_INVALID`, `STORE_IO` (the data dir is unresolvable, not a directory, or not the user's or group- or world-writable, whose fix is `chmod go-w <dir>`; §3.3), `STORE_LOCKED` (a writer held the lock for 5 s; §3.3) and `STORE_UNPARSEABLE` apply, all exit 3. **Text:** one `name<TAB>value` line each for `core_version`, `store`, `resources`, `concepts`, `types`, `relation_kinds` and `relations`; then, unless `--brief`, `concept<TAB><id><TAB><label>` per Concept, `type<TAB><id><TAB><label><TAB><parents joined with ", ">` per Type and `relation_kind<TAB><id><TAB><label><TAB><parents joined with ", ">` per kind, in that order; every value is escaped for the terminal.

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
- **No pager.** No color when stdout isn't a TTY or `NO_COLOR` is set. A usage error is never colored (§9.4).
- **`find` never returns file content.**
- **Escaped for the terminal** means each control character (C0, DEL, C1), line or paragraph separator (U+2028, U+2029), bidi control (U+061C, U+200E, U+200F, U+202A–U+202E, U+2066–U+2069) and invisible format character (U+00AD, U+180E, U+200B, U+2060–U+2064, U+206A–U+206F, U+FEFF, U+FFF9–U+FFFB, U+E0000–U+E007F) is printed as `\u{<hex>}`; everything else, ZWNJ and ZWJ included, is printed as is. `--json` output is never escaped this way. The list is explicit: other invisible format characters (such as the Arabic number signs U+0600–U+0605, U+06DD, U+070F, and script-specific format controls), variation selectors, the combining grapheme joiner, the Mongolian free variation selectors and the Hangul fillers are deliberately not escaped, because they are part of real text in some scripts. They can make two different ids look the same, but they cannot move the cursor, set the title or add a line.

### 9.4 Exit codes and errors
| Code | Meaning |
|---|---|
| 0 | success, **including empty results** (never 1 for "no matches") |
| 1 | rule/domain error (a structured error was emitted) |
| 2 | usage error (bad flags or arguments) |
| 3 | store or environment problem (data dir unresolvable or unreadable, `STORE_LOCKED`, unparseable file) |

- **Under `--json`**, the error goes to stderr:
  ```json
  {"schema_version":1,"error":{"rule":"UNKNOWN_CONCEPT","message":"…","details":{"concept":"ml","suggestions":[]},
   "suggested_fix":"…","suggested_command":"hali --dry-run concept add ml"}}
  ```
- **In text mode**, the output is one `error:` line plus a `hint:` line; the message, the suggested fix and the suggested command are each escaped for the terminal (§9.3), so outside text can neither add lines nor drive the terminal. A usage error that clap reports itself (an unknown subcommand or flag, a missing or invalid argument, an unknown `schema` name) keeps clap's layout without colour. Every argument it repeats is escaped for the terminal, and so is the rest of its text apart from its line breaks.
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
- **Read-only commands** (`describe`, `concept list`, `concept show`, `type list`, `relation list`, `validate`, `schema`, `find`, `show`, `query`) accept `--dry-run` and ignore it: their output is identical to a run without it. `describe` also accepts `--yes` and ignores it, writes nothing and runs no SHACL.
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
- `forget`, `scan --prune`, `concept|type|relation remove` and `relink --force` ask for confirmation **only on a TTY**: when stdin **and** stderr are both terminals. The prompt goes to stderr, `forget resource <id> "<title>" <path>? [y/N] ` for forget, `forget <n> missing resources under <root>? [y/N] ` for `scan --prune` (the root escaped for the terminal; use `--dry-run` for the list), `forget <n> missing resources under <k> scan roots? [y/N] ` for a bare `scan --prune`, `<k>` counting the roots scanned, `remove concept <id> "<label>"? [y/N] ` for `concept remove` and `remove type <id> "<label>"? [y/N] ` for `type remove` and `remove relation <id> "<label>"? [y/N] ` for `relation remove` and `relink resource <id> "<title>" <path>? [y/N] ` for `relink <ref> <path> --force` on other content, `<path>` being the new real path (no trailing newline; the title or label is escaped for the terminal), and the answer is one line from stdin; stdout stays clean and `--json` changes none of this. `y` or `yes` (trimmed, any case) confirms; anything else, end of input or a read error declines, with the same rule as below and `"declined": true`, and nothing is written.
- **Without both terminals** they fail with `CONFIRMATION_REQUIRED` (exit 1, `details {"command": …, "declined": false}`), suggesting `--yes`: its `suggested_command` is the command again with full references and `--yes` (`hali concept remove <id> --yes`; `hali --yes concept remove -- <id>` when the id starts with `-`; likewise `hali type remove <id> --yes` and `hali relation remove <id> --yes`, a relation id with a `:` being single-quoted; `hali forget <id> --yes`, or `hali forget <id> --force --yes` when `--force` was given; `hali relink <id> <path> --force --yes`, the path single-quoted; `hali scan '<root>' [--include-hidden] [--no-ignore] [--exclude=<glob>]… [--max-size=<bytes>] [--media-type=<pattern>]… --prune [--force] --yes` (`hali scan '<root>' --prune --yes` when no other flag was given), repeating only the flags given on the command line, in that order, each value single-quoted when it needs it, `--max-size` as its byte count and `--media-type` as its lowercase pattern; `hali scan --prune [--force] --yes` for a bare `scan --prune`), and `details.command` names the command (`"concept remove"`, `"type remove"`, `"relation remove"`, `"forget"`, `"relink"`, `"scan"`).
- `--yes` is a global flag (§9.1), accepted before or after the subcommand. It confirms; commands that are not destructive ignore it.
- **Order:** usage, store, domain and SHACL errors come before the confirmation. `--dry-run` (§9.5) never asks and never fails with `CONFIRMATION_REQUIRED`. A command that would change nothing never asks.
- Nothing ever waits on stdin: without both terminals it is never read, and `--yes` and `--dry-run` never read it.
- **The lock:** a prompt is shown with the store's exclusive lock held (§3.3), so the change confirmed is the one written; other hali processes get `STORE_LOCKED` after 5 s while it is open.

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
  - **Vocabulary:** `hali describe --json` supplies the Concept, Type and relation kind lists, and `hali show <ref> --json` supplies relations.
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
- `HALI_TEST_PUBLISH_STALL=<marker>` makes a writer create the file `<marker>` just before each publish's `rename` (§3.3) and then sleep, so a test can kill it mid-publish. It too is compiled in only under `test-hooks`; no hook shortens the 5 s lock wait.
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
| Directory walk | `ignore` | 0.4.34 |
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
