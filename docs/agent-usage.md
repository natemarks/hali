# Using hali from an AI agent

This is the usage guide for AI agents (and the humans who drive them). `hali help agents`
prints it; `hali help agents --snippet` prints the block below, to paste into another
repository's `AGENTS.md` or `CLAUDE.md`.

<!-- hali:snippet:start -->
## hali

hali keeps an RDF ontology (SKOS, OWL 2, SHACL) that describes the user's local files in
place: their titles, Types, Concepts and relations. Run `hali help agents` for the full
guide.

- Set `HALI_AGENT=<your agent name>` on every hali command you run.
- Read first: `hali describe --json`, then `hali find --json` and `hali show <ref> --json`;
  read a file at its `path` directly.
- Before any change, run it with `hali --dry-run` first, show the user the `changes`, and run
  it for real only with the user's approval. Never pass `--yes` on your own.
- Turn errors into questions with their `suggested_command`.
- Report absolute paths.
<!-- hali:snippet:end -->

## Words

A **Resource** is one cataloged file. A **Type** is a class of Resources (`Paper`, `Note`). A
**Concept** is a controlled tag a Resource carries (`ml`), in one scheme with broader and
narrower Concepts. A **relation kind** links two Resources (`annotates`). hali never writes,
moves or deletes a cataloged file; it writes only its own store.

## A first session

The walkthrough below runs in order on a small store: a paper `~/papers/ldp.pdf` (Type
`Paper`, tagged `ml`, narrower than `computing`), two notes under the Scan root `~/notes`, of
which `ideas.md` annotates the paper, and one Missing Resource whose file moved to
`~/archive/old.pdf`. A trailing `# exit N` is the exit code to expect; without one it is 0.

```sh
# orient: the vocabulary, the counts and the store path
HALI_AGENT=claude hali describe --json
# read: find by tag (narrower Concepts included), then show one Resource
hali find --tag computing --json
hali show ~/papers/ldp.pdf --json
hali find --related-to ~/papers/ldp.pdf --json
# an unknown Concept is an error that suggests the closest ones
hali find --tag computers --json  # exit 1
# preview a change; nothing is written
hali --dry-run --json edit ~/notes/todo.md --add-tag computing
HALI_AGENT=claude hali --dry-run new "Reading list" --tag ml
# Relink: what is Missing, and where it went
hali find --missing --json
hali --dry-run relink --json
# anything find cannot express
hali query "SELECT ?title WHERE { ?r dcterms:subject hali:ml ; dcterms:title ?title }"
hali scan --list
```

## 1. Orientation

Set `HALI_AGENT` to your agent name (see "Agent identity" below), then run
`hali describe --json` once. It prints the core ontology version, the store path, the counts
and every Concept, Type and relation kind, so you can use the user's own vocabulary instead of
guessing. `hali describe --brief --json` prints only the version, the store and the counts.
`hali schema` lists the names of the JSON Schemas of every `--json` output; `hali schema find`
prints one,
and `hali schema error` the error envelope's.

## 2. Read first

Answer questions with read-only commands; they never write and never ask:

- `hali find --tag ml --json`, `hali find --type Paper --json`, `hali find --title ldp --json`
  and `hali find --related-to ~/papers/ldp.pdf --via annotates --json` list Resources. `--tag`
  includes narrower Concepts (`--exact` turns that off) and `--type` includes subtypes.
  Repeated filters are ANDed. Lists are paged: read `total`, `truncated` and `next_offset`,
  and pass `--offset` for the next page. `--fields id,path,title` narrows each record.
- `hali show ~/papers/ldp.pdf --json` shows one Resource with its relations and its catalog
  record. A Resource is named by its path or by its full UUID, never a prefix.
- To read a file, open the `path` of its record directly. hali never returns file content.
- `hali validate --json` checks the store against the SHACL shapes.
- `missing` (the file is gone) and `stale` (it changed since it was cataloged) are computed on
  every read. `hali find --missing --json` and `hali find --stale --json` list them.

An empty result is exit 0 with an empty list, never an error.

## 3. Changes: dry run first, then the user's approval

Every command that writes accepts `--dry-run`. Put it straight after `hali`, before the
subcommand: `hali --dry-run concept add "linked data" --broader computing`. A dry run prints
the same payload as the real run plus `"dry_run": true` and `changes`, the exact triples it
would add or remove, and writes nothing (no store, no file). It takes a shared lock to load the store, so it can wait
and report `STORE_LOCKED`.

1. Run the command with `hali --dry-run` (and `--json` if you parse it).
2. Show the user what would change: the summary and the `changes`.
3. Run it for real, the same command without `--dry-run`, only after the user's approval.

Never run a real mutation the user has not approved, and never grow the vocabulary (a new
Concept, Type or relation kind) without asking. The commands that write are `add`, `new`,
`edit`, `forget`, `refresh`, `scan`, `relate`, `unrelate`, `relink` and the `add`, `rename`,
`edit` and `remove` of `concept`, `type` and `relation`.

## 4. Unknown Concepts

A Concept is named by its slug or any of its labels, compared case-insensitively. Anything
else is `UNKNOWN_CONCEPT` (exit 1), never an empty result. Its `details.suggestions` holds up
to 3 close Concepts, and its `suggested_command` is a dry run that would add it:

```json
{"schema_version":1,"error":{"rule":"UNKNOWN_CONCEPT","message":"no Concept matches computers","details":{"concept":"computers","suggestions":[{"id":"computing","label":"computing"}]},"suggested_fix":"Use the slug or a label of an existing Concept, or add the Concept first.","suggested_command":"hali --dry-run concept add computers"}}
```

Ask the user: use one of the suggestions (`computing`), or add the new Concept (run the
suggested dry run, show it, then add it for real once they approve). `UNKNOWN_TYPE` and
`UNKNOWN_RELATION` work the same way: their `suggested_command` is a dry run such as
`hali --dry-run type add Article`, or a list such as `hali relation list`.

## 5. Errors and exit codes

- exit 0: success, including empty results.
- exit 1: a rule or domain error; a structured error was printed.
- exit 2: a usage error: a bad flag or argument (clap's text, also under `--json`).
- exit 3: a store or environment problem (the data dir, `STORE_LOCKED`, an unparseable file).

Under `--json` an error goes to stderr as one JSON object, and stdout stays empty:
`{"schema_version":1,"error":{"rule","message","details","suggested_fix","suggested_command"}}`.
`rule` is a stable SCREAMING_SNAKE name to branch on. `suggested_fix` says what to do, and
`suggested_command` is the command to try next (or null). Turn the error into a question for
the user, quoting the `suggested_command`; do not run it blindly, because it may write. In
text mode the same error is one `error:` line and one `hint:` line on stderr.

`STORE_LOCKED` (exit 3) means another hali process held the store for 5 seconds: retry later.

## 6. Confirmation and `--yes`

`forget`, `scan --prune`, `concept remove`, `type remove`, `relation remove` and
`relink --force` are destructive. On a terminal they ask `[y/N]`; without one (as for an agent) they
refuse with `CONFIRMATION_REQUIRED` (exit 1) and write nothing:

```json
{"schema_version":1,"error":{"rule":"CONFIRMATION_REQUIRED","message":"forget needs confirmation, but stdin or stderr is not a terminal; nothing was written","details":{"command":"forget","declined":false},"suggested_fix":"Run it again with --yes to confirm, or with --dry-run to see what it would change.","suggested_command":"hali forget <id> --yes"}}
```

Never answer a hali `[y/N]` confirmation yourself: not by typing `y`, not by piping `yes`, not
by adding `--yes`. Only the user may. If a prompt appears or `CONFIRMATION_REQUIRED` is
returned, stop and ask the user.

`--yes` confirms without asking. Never add `--yes` on your own: use it only with the user's
explicit consent to that exact command, after they have seen its dry run (for example
`hali --dry-run forget ~/notes/todo.md`). `forget` never deletes the file; it removes the
Resource and its catalog record from the ontology. A Resource that others relate to is
refused with `FORGET_BLOCKED_BY_RELATIONS` unless `--force`, which removes those relations too.

## 7. Paths

Always report absolute paths to the user: the `path` of a record is the file's real path
(symlinks resolved), absolute. You may pass a path relative to the current directory, or a
`~/` path your shell expands, but repeat the absolute `path` from the output. Use
`--path-only` for one path per line: `hali find --tag ml --path-only`,
`hali show ~/papers/ldp.pdf --path-only`, and `hali new "Meeting notes" --path-only` (after a
dry run and the user's approval) for the path of the file it creates. hali never opens an editor.

## 8. SPARQL queries

Use `hali query "<sparql>"` for anything `find` cannot express. It runs one read-only SPARQL 1.1 query
(SELECT, ASK, CONSTRUCT or DESCRIBE) over all the store's graphs; the prefixes `rdf`, `rdfs`,
`owl`, `xsd`, `skos`, `dcterms`, `dcat`, `prov`, `foaf`, `spdx`, `sh` and `hali` are
predeclared. An update (INSERT, DELETE, ...) is refused with `READ_ONLY_QUERY`. Write `LIMIT`
in the query: there is no `--limit`, and over 100,000 rows or 30 seconds is `QUERY_FAILED`.
With `--json`, SELECT and ASK print the W3C SPARQL results JSON format:

```
hali query "SELECT ?r ?title WHERE { ?r a hali:Paper ; dcterms:title ?title } LIMIT 20" --json
hali query "ASK { ?r dcterms:subject hali:ml }" --json
```

## 9. Agent identity

`HALI_AGENT=<name>` (or `--agent <name>`, which overrides it) names the AI agent on whose
behalf hali runs. The name is self-asserted: hali does not check it, and anyone can claim any
name. It is recorded only when a new catalog record is written: by `add`, `scan` and `new`, on
the Resources they newly catalog (as `prov:wasAttributedTo` a `prov:SoftwareAgent` acting for
the user). Every other command accepts it and ignores it; an existing record's attribution
never changes. A name with a control character, an empty `--agent`, or a `HALI_AGENT` that is
not UTF-8 is a usage error (exit 2). `hali show <ref> --json` shows the attribution under
`catalog_record.attributed_to`.

```
HALI_AGENT=claude hali --dry-run add ~/papers/ldp.pdf --type Paper --tag ml
hali --dry-run --agent claude scan ~/notes
```

## 10. Scan roots

`hali --dry-run scan ~/notes` previews a scan of one directory: the new files it would
register (untyped and untagged), the changed ones it would refresh, the moved ones it would
relink and the vanished ones it reports as `missing`. A real `hali scan ~/notes` also
remembers `~/notes` as a Scan root, with the flags given, in `roots.json` in hali's state dir
(`$XDG_STATE_HOME/hali/`, else `~/.local/state/hali/`), outside the store.

- `hali scan --list --json` shows the remembered roots and their settings (read-only).
- `hali scan` (bare, no root) re-scans every remembered root with its own settings, as one
  run. Preview it with `hali --dry-run scan`.
- `hali scan --forget-root ~/notes` stops remembering a root; its Resources stay cataloged.

`--prune` forgets the Missing Resources under the scanned roots (files are never deleted), with
`forget`'s rules. It needs confirmation, so preview it with `hali --dry-run --json scan --prune`.
`scan --prune --yes` is destructive: it forgets them without asking, under every remembered
root for a bare scan, so never run it without the user's approval of that dry run.

## More

Every `--help` starts with examples: `hali find --help`, or `hali help find`. `hali --help`
lists every command and the global flags `--json`, `--dry-run`, `--yes` and `--agent`.
