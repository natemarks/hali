#!/usr/bin/env bash
# The patterns hold literal backticks, asterisks and brackets, which must not expand.
# shellcheck disable=SC2016
set -uo pipefail
# Criterion 10 (#11): docs/spec.md documents the editable rule. §5.2 names the walk over the
# ancestors of the media type, aliases, cycles (and that the walk is bounded), the missing
# database, the exact-or-`type/*` matching of config entries and built-ins (no subclasses), the
# system-over-editor tie and `CONFIG_INVALID`; §4.1 says what `[open]` takes and no longer
# says the other keys "come from their deciding tickets".
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

status=0
spec=docs/spec.md

need() { # name text pattern...
    local name=$1 text=$2 what
    shift 2
    for what in "$@"; do
        if ! printf '%s\n' "$text" | grep -qiF -- "$what"; then
            echo "MISSING: $name does not mention: $what"
            status=1
        fi
    done
}

section() { # start-regex end-regex
    awk -v s="$1" -v e="$2" '$0 ~ s {on=1; next} $0 ~ e {on=0} on' "$spec"
}

if [ ! -f "$spec" ]; then
    echo "MISSING: $spec"
    exit 1
fi

s52=$(section '^### 5\.2 ' '^### 5\.3 ')
need '§5.2' "$s52" 'ancestor' 'alias' 'cycle' 'no database' 'text/*' 'exactly' \
    '`type/*`' 'subclass' 'CONFIG_INVALID' '[open]' 'system' 'editor' 'case-insensitive' \
    'application/xhtml+xml'

s41=$(section '^### 4\.1 ' '^## 5\. ')
need '§4.1' "$s41" '[open]' 'CONFIG_INVALID' '`*/*`' '`type/*`' 'list of strings'
if printf '%s\n' "$s41" | grep -qF 'deciding tickets'; then
    echo "WRONG: §4.1 still says the other keys come from their deciding tickets"
    status=1
fi

exit "$status"
