#!/usr/bin/env bash
# The patterns hold literal backticks, which must not expand.
# shellcheck disable=SC2016
set -uo pipefail
# Criterion 16 (#20): docs/spec.md states the refresh contract (§9.1, §6) and GLOSSARY.md defines
# Refresh. Written by the test author in the goal commit: a guard, not a red.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

status=0
spec=docs/spec.md

para=$(grep -E '^\*\*Refresh:\*\*' "$spec" || true)
if [ -z "$para" ]; then
    echo "MISSING: $spec has no **Refresh:** paragraph in §9.1"
    status=1
else
    for what in '--all' 'missing' 'unchanged' 'refreshed' 'FILE_UNREADABLE' 'NOT_A_REGULAR_FILE' \
        'UNKNOWN_RESOURCE' 'SHACL_VIOLATION' 'all-or-nothing' 'only the Stale ones' 'ignores `--yes`' \
        'is not rewritten' 'dry_run'; do
        if ! printf '%s\n' "$para" | grep -qF -- "$what"; then
            echo "MISSING: the **Refresh:** paragraph does not mention: $what"
            status=1
        fi
    done
fi

if ! grep -qF 'named Resources are always re-hashed; `--all` hashes only Stale ones; Missing ones are reported' "$spec"; then
    echo "MISSING: the §6 refresh bullet in $spec does not say: named Resources are always re-hashed; \`--all\` hashes only Stale ones; Missing ones are reported"
    status=1
fi

if ! grep -qE '^\*\*Refresh\*\*:' GLOSSARY.md; then
    echo "MISSING: GLOSSARY.md has no **Refresh**: entry"
    status=1
elif ! grep -qF "To re-record a Resource's content hash, size and modification time from its file as it is now." GLOSSARY.md; then
    echo "MISSING: the GLOSSARY.md Refresh entry lacks the agreed definition"
    status=1
fi
exit "$status"
