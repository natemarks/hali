#!/usr/bin/env bash
# The patterns hold literal backticks, which must not expand.
# shellcheck disable=SC2016
set -uo pipefail
# Criterion 11 (#18): docs/spec.md states the relations contract and GLOSSARY.md the Relation.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

status=0
spec=docs/spec.md

need() {
    local what=$1 pattern=$2
    if ! grep -qE -- "$pattern" "$spec"; then
        echo "MISSING: $spec does not state $what (pattern: $pattern)"
        status=1
    fi
}

need "a **Relations:** paragraph in §9.1" '^\*\*Relations:\*\*'
need "relation add in the §9.1 Relations paragraph" '\*\*`relation add <name> --parent <P>`:\*\*'
need "relation list in the §9.1 Relations paragraph" '\*\*`relation list \[--limit N\] \[--offset N\]`\*\*'
need "relation remove in the §9.1 Relations paragraph" '\*\*`relation remove <R>`:\*\*'
need "relate in the §9.1 Relations paragraph" '\*\*`relate <A> <R> <B>`:\*\*'
need "unrelate in the §9.1 Relations paragraph" '\*\*`unrelate <A> <R> <B>`:\*\*'
need "forget with --force in the §9.1 Relations paragraph" '\*\*`forget <ref> \[--force\]`:\*\*'
need "the standard parent usage message" '--parent must be a standard relation: dcterms:relation, dcterms:isPartOf, dcterms:references or prov:wasDerivedFrom'
need "the standard relation removal usage message" 'is a standard relation and cannot be removed'
need "the self-relation usage message" 'a Resource cannot be related to itself'
need "IN_USE for a relation kind (details used, subproperties)" '"relation":"<id>","used":<n>,"subproperties":<m>'
need "the LABEL_TAKEN relation details" '`details \{"label","language","relation"\}`'
need "the FORGET_BLOCKED_BY_RELATIONS details" '`details \{"resource","relations","from"\}`'
need "the FORGET_BLOCKED_BY_RELATIONS message" 'inbound relations from <m> other Resources'
need "the forget --force retry command" 'hali forget <id> --force --yes'
need "the UNKNOWN_RELATION details in §9.2" 'details \{"relation": <ref>, "suggestions"'
need "the UNKNOWN_RELATION message in §9.2" 'no relation kind matches <ref>'
need "the relation reference rule in §9.2 (a bare name is never a standard relation)" 'A bare name never resolves to a standard relation'
need "relations in show (§9.3)" '"direction":"out"\|"in"'
need "the show text line for a relation (§9.3)" 'relation<TAB>-> <relation id>'
need "relation list in the §9.5 read-only list" 'type list`, `relation list`, `validate`'
need "the relation remove prompt in §9.8" 'remove relation <id> "<label>"\? \[y/N\] '
need "the relation remove retry command in §9.8" 'hali relation remove <id> --yes'
need "relation remove in the §9.8 details.command list" '`"relation remove"`'
need "the find --related-to semantics in §9.7" '`--related-to REF` matches a Resource X when'
need "the find --via semantics in §9.7" '`--via` matches the relation \*\*and every sub-property\*\*'
need "the standard relations in the core ontology (§3.1)" 'the four standard relations \(`dcterms:relation`, `dcterms:isPartOf`, `dcterms:references` and `prov:wasDerivedFrom`'
need "the target shape in §2.7" 'hali:RelationTargetShape'
need "the parent shape in §2.7" 'hali:RelationParentShape'

# The §9.1 forget sentence that postponed --force and the error to #18 must be gone.
if grep -qE 'arrive with #18' "$spec"; then
    echo "WRONG: $spec still says --force and FORGET_BLOCKED_BY_RELATIONS 'arrive with #18'"
    status=1
fi
# ...and so must the one that postponed the relation remove case of IN_USE.
if grep -qE 'relation `remove` adds its case later' "$spec"; then
    echo "WRONG: $spec still says relation remove adds its IN_USE case later"
    status=1
fi

if ! grep -qE '^\*\*Relation\*\*:' GLOSSARY.md; then
    echo "MISSING: GLOSSARY.md has no **Relation**: entry"
    status=1
fi

exit "$status"
