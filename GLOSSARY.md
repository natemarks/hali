# hali

hali keeps an ontology that describes the files on one machine, in place, so they can be found by meaning (what they are, what they're about, how they relate) rather than by where they sit.

## Language

### The things described

**Resource**:
A single file that the ontology describes, cataloged where it already lives. hali never moves, renames or rewrites a Resource's file.
_Avoid_: note, document, item, entry, file (when you mean the cataloged thing)

**Missing Resource**:
A Resource whose file is no longer at its recorded location.
_Avoid_: broken, dead, orphan

**Stale Resource**:
A Resource whose file has changed (size or modification time) since hali last recorded its content hash.
_Avoid_: dirty, outdated

**Media type**:
The file format of a Resource (e.g. `text/markdown`, `application/pdf`), as identified by the shared MIME database.
_Avoid_: MIME type (in user-facing text), file type, format, Type

### The vocabulary

**Type**:
What kind of thing a Resource is to the user (Paper, Receipt, Note), arranged in a hierarchy. Distinct from Media type.
_Avoid_: class (in user-facing text), category, kind

**Concept**:
A controlled tag with exactly one preferred label, optional aliases, and optional broader Concepts.
_Avoid_: tag (in code and docs; "tag" is fine as a CLI flag name), topic, keyword, label

**Concept scheme**:
The controlled vocabulary a Concept belongs to. v1 has exactly one: topics.
_Avoid_: taxonomy, tag list

**Broader Concept**:
A more general Concept that another Concept falls under. A Resource tagged with a Concept is found when searching for any of its broader Concepts.
_Avoid_: parent tag, category

**Relation kind**:
A named, directed link between two Resources. It is always a standard relation or a specialisation of one.
_Avoid_: link type, edge, association

**Relation**:
One directed link of a Relation kind from one Resource to another. Only the stored direction exists; the other is derived.
_Avoid_: link, edge

### Recording changes

**Catalog record**:
The entry recording when and by whom a Resource was cataloged, as opposed to facts about the file itself.
_Avoid_: metadata record, index entry

**Agent**:
Whoever made a change to the ontology: the user, or an AI agent acting on the user's behalf.
_Avoid_: actor, author, owner

**Forget**:
To remove a Resource from the ontology while leaving its file untouched.
_Avoid_: delete, remove, archive

**Relink**:
To point a Missing Resource at its file's new location.
_Avoid_: repair, fix, move

**Scan root**:
A directory the user has asked hali to scan, remembered with its own scan settings.
_Avoid_: watched folder, library, source
