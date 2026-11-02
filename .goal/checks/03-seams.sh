#!/usr/bin/env bash
set -uo pipefail
# Criterion 8 (#131): the seams hold. `src/core` has no `std::fs`, `ignore::` or `storage::walk`,
# and the `ignore` crate is named only in src/storage/walk.rs.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

status=0

if [ ! -f src/storage/walk.rs ]; then
    echo "MISSING: src/storage/walk.rs (the walker seam, #131)"
    status=1
fi

found=$(grep -rnE 'std::fs|\bignore::|storage::walk' src/core || true)
if [ -n "$found" ]; then
    while IFS= read -r line; do
        echo "FOUND: forbidden in src/core: $line"
    done <<<"$found"
    status=1
fi

# `ignore::` as a path (not a word in a comment such as "ignore::" mid-sentence is unlikely).
found=$(grep -rnE '(^|[^[:alnum:]_])ignore::' src | grep -v '^src/storage/walk.rs:' || true)
if [ -n "$found" ]; then
    while IFS= read -r line; do
        echo "FOUND: the ignore crate outside src/storage/walk.rs: $line"
    done <<<"$found"
    status=1
fi
found=$(grep -rnE '^[[:space:]]*(pub )?use ignore\b|extern crate ignore' src | grep -v '^src/storage/walk.rs:' || true)
if [ -n "$found" ]; then
    while IFS= read -r line; do
        echo "FOUND: the ignore crate outside src/storage/walk.rs: $line"
    done <<<"$found"
    status=1
fi
exit "$status"
