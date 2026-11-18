#!/usr/bin/env bash
set -uo pipefail
# Criterion 18 (#25): once the store lock (#198) lands, `new` is a mutating command: it must be
# in `writes_store`. Vacuous until then: with no `fn writes_store` in src/, this passes; if the
# function exists and `Command::New` is not inside it, this fails.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

file=$(git grep -lE 'fn writes_store' -- src/ | head -n 1 || true)
if [ -z "$file" ]; then
    exit 0
fi
body=$(awk '/fn writes_store/{on=1} on{print} on && /^}/{exit}' "$file")
if ! printf '%s\n' "$body" | grep -qF 'Command::New'; then
    echo "MISSING: fn writes_store in $file does not list Command::New: hali new writes the store and must take the lock"
    exit 1
fi
exit 0
