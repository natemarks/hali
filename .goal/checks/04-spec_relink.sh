#!/usr/bin/env bash
# The patterns hold literal backticks and a literal tilde, which must not expand.
# shellcheck disable=SC2016,SC2088
set -uo pipefail
# Criterion 21 (#21): docs/spec.md states the relink contract: a **Relink:** paragraph in §9.1,
# the §9.8 prompt of `relink --force`, the files.dir chain in §4.1 and §6, and §2.1 naming relink
# as what changes `dcat:downloadURL`.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

status=0
spec=docs/spec.md

para=$(grep -E '^\*\*Relink:\*\*' "$spec" || true)
if [ -z "$para" ]; then
    echo "MISSING: $spec has no **Relink:** paragraph in §9.1"
    status=1
else
    for what in '--root' '--force' '--new' 'roots' 'counts' 'relinked' 'ambiguous' 'unmatched' \
        'candidates' 'truncated' 'refreshed' 'dry_run' 'changes' 'from' \
        'HASH_MISMATCH' 'POSSIBLE_MOVE' 'CONFIRMATION_REQUIRED' 'UNKNOWN_RESOURCE' \
        'NOT_A_REGULAR_FILE' 'FILE_UNREADABLE' 'SHACL_VIOLATION' 'CONFIG_INVALID' \
        'files.dir' 'user-dirs' '~/Documents/hali' 'one-to-one' 'size + 1' 'empty' \
        'resources.ttl' 'one mutation' 'real path' 'never asks' 'stdin' 'usage error' \
        '100' '10'; do
        if ! printf '%s\n' "$para" | grep -qF -- "$what"; then
            echo "MISSING: the **Relink:** paragraph does not mention: $what"
            status=1
        fi
    done
fi

if ! grep -qF 'relink resource <id> "<title>" <path>? [y/N] ' "$spec"; then
    echo "MISSING: §9.8 in $spec does not give the prompt: relink resource <id> \"<title>\" <path>? [y/N] "
    status=1
fi

dir_line=$(grep -E '^dir = ' "$spec" || true)
if ! printf '%s\n' "$dir_line" | grep -qF 'relink'; then
    echo "MISSING: the [files] dir line of §4.1 in $spec does not say that relink searches it"
    status=1
fi

roots=$(grep -E 'Default roots' "$spec" || true)
if [ -z "$roots" ]; then
    echo "MISSING: §6 in $spec has no Default roots bullet"
    status=1
else
    for what in 'files.dir' 'user-dirs' 'skipped'; do
        if ! printf '%s\n' "$roots" | grep -qF -- "$what"; then
            echo "MISSING: the §6 Default roots bullet does not mention: $what"
            status=1
        fi
    done
fi

url=$(grep -E '^\| `dcat:downloadURL`' "$spec" || true)
if ! printf '%s\n' "$url" | grep -qF 'relink'; then
    echo "MISSING: the dcat:downloadURL row of §2.1 in $spec does not say that relink changes it"
    status=1
fi
exit "$status"
