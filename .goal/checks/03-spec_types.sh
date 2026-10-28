#!/usr/bin/env bash
# The patterns hold literal backticks, which must not expand.
# shellcheck disable=SC2016
set -uo pipefail
# Criterion 8 (#15): docs/spec.md states the Types contract.
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

need "a **Types:** paragraph in §9.1" '^\*\*Types:\*\*'
need "type add <Name> [--parent T] in the §9.1 Types paragraph" '\*\*`type add <Name> \[--parent T\]`:\*\*'
need "type list in the §9.1 Types paragraph" '\*\*`type list \[--limit N\] \[--offset N\]`\*\*'
need "type remove <T> in the §9.1 Types paragraph" '\*\*`type remove <T>`:\*\*'
need "IN_USE for a Type (details typed, subtypes)" '"type":"<id>","typed":<n>,"subtypes":<m>'
need "add --type in the §9.1 Types paragraph" '\*\*`add --type T…`\*\*'
need "edit --add-type/--remove-type in the §9.1 Types paragraph" '\*\*`edit --add-type T… --remove-type T…`\*\*'
need "the edit usage message with the type flags" 'edit needs --title, --add-type, --remove-type, --add-tag or --remove-tag'
need "the UNKNOWN_TYPE details in §9.2" 'details \{"type": <ref>, "suggestions"'
need "the UNKNOWN_TYPE message in §9.2" 'no Type matches <ref>'
need "the Type reference rule in §9.2 (hali:Resource is a usage error)" 'resolves to `hali:Resource`, is a usage error'
need "the LABEL_TAKEN Type details" '`details \{"label","language","type"\}`'
need "type list in the §9.5 read-only list" 'Read-only commands\*\* \(`concept list`, `concept show`, `type list`'
need "the type remove prompt in §9.8" 'remove type <id> "<label>"\? \[y/N\] '
need "the type remove retry command in §9.8" 'hali type remove <id> --yes'
need "the find --type semantics in §9.7" '`--type` matches the Type \*\*and every subclass\*\*'
need "the §2.8 example typed a hali:Resource , hali:Paper" 'a hali:Resource , hali:Paper'

# The §2.8 example must not keep the old untyped-only form.
if grep -qE 'a hali:Paper ;' "$spec"; then
    echo "WRONG: $spec still has the Resource 'a hali:Paper ;' (§2.8 must say 'a hali:Resource , hali:Paper')"
    status=1
fi

exit "$status"
