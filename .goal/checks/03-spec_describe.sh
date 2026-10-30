#!/usr/bin/env bash
# The patterns hold literal backticks, which must not expand.
# shellcheck disable=SC2016
set -uo pipefail
# Criterion 12 (#19): docs/spec.md states the describe contract (§9.1, §9.4, §9.5).
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

status=0
spec=docs/spec.md

para=$(grep -E '^\*\*Describe:\*\*' "$spec" || true)
if [ -z "$para" ]; then
    echo "MISSING: $spec has no **Describe:** paragraph in §9.1"
    status=1
else
    for what in 'core_version' 'store' 'counts' '--brief' 'relation_kinds' 'relations' \
        'owl:versionInfo' 'absent or empty' 'never creates' 'name<TAB>value'; do
        if ! printf '%s\n' "$para" | grep -qF -- "$what"; then
            echo "MISSING: the **Describe:** paragraph does not mention: $what"
            status=1
        fi
    done
fi

need() {
    local what=$1 pattern=$2
    if ! grep -qE -- "$pattern" "$spec"; then
        echo "MISSING: $spec does not state $what (pattern: $pattern)"
        status=1
    fi
}

need "describe in the §9.5 read-only list" 'Read-only commands\*\* \(`describe`, '
need "the §9.4 exit-3 row wording" '\| 3 \| store or environment problem \(data dir unresolvable or unreadable, `STORE_LOCKED`, unparseable file\) \|'
need "the skill install state marked as arriving with #28 in the §9.1 command block" 'skill install state arrives with #28'

if grep -qE '\(no store, ' "$spec"; then
    echo "WRONG: $spec still has the exit-3 wording '(no store, ...'; an absent store is an empty store"
    status=1
fi
exit "$status"
