#!/usr/bin/env bash
# The patterns hold literal backticks, angle brackets and quotes, which must not expand.
# shellcheck disable=SC2016
set -uo pipefail
# Criterion 17 (#23): docs/spec.md describes the remembered scan roots: §4 names roots.json, §6
# says relink searches the remembered roots by default, §7 describes the settings, the bare
# scan, --list, --forget-root and the state dir, §9.1 **Scan:** and **Relink:** carry the
# behaviour and the output, §9.8 gives the bare-prune prompt and retry, and the "not there yet"
# notes are gone. A root that is the state dir or inside it is refused by scan and relink
# (`state dir or inside it`, named in §7 and in both paragraphs). An agent guide (docs/agent-usage.md or skill/SKILL.md), when the repo has one,
# says the same in short.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

status=0
spec=docs/spec.md

need() { # name text pattern...
    local name=$1 text=$2 what
    shift 2
    for what in "$@"; do
        if ! printf '%s\n' "$text" | grep -qF -- "$what"; then
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

if grep -nF '#23' "$spec" >/dev/null; then
    echo "WRONG: $spec still mentions #23 (a 'not there yet' note):"
    grep -nF '#23' "$spec" | cut -c1-160
    status=1
fi
if grep -F 'are not there yet' "$spec" >/dev/null; then
    echo "WRONG: $spec still says remembered roots are not there yet"
    status=1
fi

s4=$(section '^## 4\. ' '^### 4\.1 ')
need '§4' "$s4" 'roots.json'

s6=$(section '^## 6\. ' '^## 7\. ')
need '§6' "$s6" 'remembered' 'byte order' '--root' 'own settings'
if printf '%s\n' "$s6" | grep -qF 'join the defaults with the remembered-roots ticket'; then
    echo "WRONG: §6 still says remembered roots join the defaults with a ticket"
    status=1
fi

s7=$(section '^## 7\. ' '^## 8\. ')
need '§7' "$s7" 'roots.json' 'one run' 'unavailable' '--list' '--forget-root' \
    'hali scan --list' 'usage error' 'never rewrite' 'state dir' 'never walked' \
    'state dir or inside it'

relink=$(grep -E '^\*\*Relink:\*\*' "$spec" || true)
if [ -z "$relink" ]; then
    echo "MISSING: $spec has no **Relink:** paragraph in §9.1"
    exit 1
fi
need 'the **Relink:** paragraph' "$relink" 'remembered' 'files.dir' 'byte order' \
    'own settings' 'STORE_IO' 'STORE_UNPARSEABLE' 'state dir or inside it'

scan=$(grep -E '^\*\*Scan:\*\*' "$spec" || true)
if [ -z "$scan" ]; then
    echo "MISSING: $spec has no **Scan:** paragraph in §9.1"
    exit 1
fi
need 'the **Scan:** paragraph' "$scan" 'roots.json' '`--list`' '`--forget-root`' \
    'unavailable' '"roots"' '"schema_version":1,"roots"' 'XDG_STATE_HOME' \
    'STORE_IO' 'STORE_UNPARSEABLE' '1 MiB' 'hali scan --list' 'dry_run' \
    'never pruned' 'byte order' 'one-to-one' 'schema_version' 'state dir or inside it'
if printf '%s\n' "$scan" | grep -qF 'are not there yet'; then
    echo "WRONG: the **Scan:** paragraph still says the remembered roots are not there yet"
    status=1
fi
if printf '%s\n' "$scan" | grep -qF 'with no root, two roots or one of those flags it is a usage error'; then
    echo "WRONG: the **Scan:** paragraph still says a bare scan is a usage error"
    status=1
fi

s98=$(section '^### 9\.8 ' '^### 9\.9 ')
need '§9.8' "$s98" 'forget <n> missing resources under <k> scan roots? [y/N] ' \
    'hali scan --prune [--force] --yes'

for guide in docs/agent-usage.md skill/SKILL.md; do
    if [ -f "$guide" ]; then
        text=$(cat "$guide")
        need "$guide" "$text" 'scan --list' '--forget-root' 'roots.json'
    fi
done
exit "$status"
