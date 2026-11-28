---
name: hali
description: Find, open, tag or catalog local files and documents; ontology, concepts, types, related files; hali. Uses the hali CLI, which keeps an RDF ontology (SKOS, OWL 2, SHACL) of the user's local files in place, to find files by Concept, Type, title or relation, show one with its catalog record, and preview any change as a dry run before the user approves it.
metadata:
  hali-version: "{{HALI_VERSION}}"
allowed-tools: Bash(hali describe *), Bash(hali find *), Bash(hali show *), Bash(hali query *), Bash(hali schema *), Bash(hali validate *), Bash(hali help *), Bash(hali --dry-run *)
---
# hali

hali keeps an RDF ontology (SKOS, OWL 2, SHACL) that describes the user's local files in
place: their titles, Types, Concepts and the relations between them. It never writes, moves
or deletes a cataloged file; it writes its own store and state, the new files that `new`
creates, and the files of this skill (`hali skill install`). The full usage guide is
[references/usage.md](references/usage.md) (the same text as `hali help agents`).

## 1. Name yourself

On a command that writes, pass `--agent <name>` (your agent name) after the subcommand, so
the command still starts with `hali --dry-run` or the subcommand. The dry run comes first, the
real run only after the user's approval (section 4):

```
hali --dry-run add ~/papers/ldp.pdf --type Paper --agent claude
hali add ~/papers/ldp.pdf --type Paper --agent claude
```

The name is recorded only on the catalog records that `add`, `scan` and `new` create, so
read-only commands need no name: run them without one. `HALI_AGENT=<name>` in the environment
works too, but use it only for writes, never in front of a read-only command.

## 2. Orient first

Run `hali describe --json` once, before anything else. It prints the core ontology version,
the store path, the counts and every Concept, Type and relation kind, so use the user's own
vocabulary instead of guessing. `hali describe --brief --json` prints only the version, the
store and the counts.

## 3. Read before anything else

Answer questions with read-only commands; they never write and never ask:

- `hali find --tag ml --json` lists the Resources tagged `ml` or a narrower Concept;
  `hali find --type Paper --json`, `hali find --title ldp --json` and
  `hali find --related-to ~/papers/ldp.pdf --json` filter by Type, title and related files.
  Lists are paged: read `total`, `truncated` and `next_offset`, and pass `--offset`.
- `hali show ~/papers/ldp.pdf --json` shows one Resource with its relations and its catalog
  record. Name a Resource by its path or by its full UUID, never a prefix.
- To read a file, open the `path` of its record directly. hali never returns file content.
- `hali validate --json` checks the store; `hali schema find` prints the JSON Schema of an
  output, and `hali schema error` the error envelope's.

An empty result is exit 0 with an empty list, never an error.

## 4. Changes: a dry run first, then the user's approval

Every command that writes accepts the dry-run flag. Put it straight after `hali`, before the
subcommand, so it is always read as the flag:

```
hali --dry-run --json concept add "linked data" --broader computing
hali --dry-run edit ~/notes/ideas.md --add-tag ml
```

1. Run the command with `hali --dry-run` first (and `--json` to parse it). It writes nothing.
2. Show the user the summary and the `changes`: the exact triples it would add or remove.
3. Run the same command without the dry-run prefix only after the user's approval.

Never run a real change the user has not approved. Never grow the vocabulary (a new Concept,
Type or relation kind) without asking first.

## 5. Unknown Concepts

A Concept is named by its slug or one of its labels, compared case-insensitively. Anything else
is `UNKNOWN_CONCEPT` (exit 1), never an empty result. Its `details.suggestions` lists up to 3
close Concepts and its `suggested_command` is a dry run of a `concept add`. Ask the user: use
one of the suggestions, or add the new Concept (show its dry run, then add it for real once
they approve). `UNKNOWN_TYPE` and `UNKNOWN_RELATION` work the same way.

## 6. Errors are questions

Under `--json` an error goes to stderr as one JSON object, and stdout stays empty:
`{"schema_version":1,"error":{"rule","message","details","suggested_fix","suggested_command"}}`.
Branch on `rule`, then turn the error into a question for the user that quotes its
`suggested_command`. Do not run that command blindly: it may write. Exit codes: 0 success
(empty results included), 1 a rule error, 2 a usage error, 3 a store or environment problem
(`STORE_LOCKED`: another hali process holds the store, so retry later).

## 7. Confirmation

`forget`, `scan --prune`, `concept remove`, `type remove`, `relation remove` and
`relink --force` are destructive. Without a terminal they refuse with `CONFIRMATION_REQUIRED`
and write nothing. Never answer a confirmation yourself, and never add `--yes` on your own: use
it only with the user's explicit consent to that exact command, after they have seen its dry
run. If a prompt appears or `CONFIRMATION_REQUIRED` is returned, stop and ask the user.

## 8. Paths

Always report absolute paths to the user: the `path` of a record is the file's real path
(symlinks resolved). You may pass a relative or `~/` path, but repeat the absolute `path` from
the output. `hali find --tag ml --path-only` prints one path per line.

## 9. SPARQL

Use `hali query "<sparql>"` for anything `find` cannot express. It runs one read-only SPARQL
1.1 query (SELECT, ASK, CONSTRUCT or DESCRIBE) over all the store's graphs, with the prefixes
`rdf`, `rdfs`, `owl`, `xsd`, `skos`, `dcterms`, `dcat`, `prov`, `foaf`, `spdx`, `sh` and
`hali` predeclared. Write a `LIMIT` in the query:

```
hali query "SELECT ?r ?title WHERE { ?r a hali:Paper ; dcterms:title ?title } LIMIT 20" --json
```

## 10. Keeping this skill current

`hali describe --json` ends with a `skill` key: `installed` (the version of this skill), the
`binary` version and `stale`. When `stale` is true, suggest that the user runs
`hali skill install` to update this skill.
