#!/usr/bin/env bash
# The patterns hold literal backticks, angle brackets and quotes, which must not expand.
# shellcheck disable=SC2016
set -uo pipefail
# Criterion 11 (#5): docs/spec.md describes the store lock and the store-file safety rules.
# §3.3 gives the lock file, the modes, the 5 s wait and `STORE_LOCKED` (`reason`), the absent data
# dir, the atomic publish, the reads of the store files (256 MiB cap, FIFOs, symlinks) and the
# data dir check; §9.1's **Describe:** paragraph names the data-dir check and `STORE_LOCKED`;
# §9.8 says a prompt holds the lock; §14.2 lists `HALI_TEST_PUBLISH_STALL`; §14.4 keeps the
# concurrency test.
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

s33=$(section '^### 3\.3 ' '^### 3\.4 ')
need '§3.3' "$s33" '.lock' '256 MiB' '`reason`' 'STORE_LOCKED' 'shared' 'exclusive' '5 s' \
    '`changed`' '`timeout`' 'O_EXCL' 'rename' 'symlink' 'FIFO' '0600' '0700' \
    'chmod go-w' 'prompt' 'STORE_IO' 'CLOEXEC' 'network'

describe=$(grep -E '^\*\*Describe:\*\*' "$spec" || true)
if [ -z "$describe" ]; then
    echo "MISSING: $spec has no **Describe:** paragraph in §9.1"
    exit 1
fi
need 'the **Describe:** paragraph' "$describe" 'STORE_LOCKED' 'chmod go-w'

s98=$(section '^### 9\.8 ' '^### 9\.9 ')
need '§9.8' "$s98" 'lock'

s142=$(section '^### 14\.2 ' '^### 14\.3 ')
need '§14.2' "$s142" 'HALI_TEST_PUBLISH_STALL' 'test-hooks'

s144=$(section '^### 14\.4 ' '^### 14\.5 ')
need '§14.4' "$s144" '8' '5 s'

exit "$status"
