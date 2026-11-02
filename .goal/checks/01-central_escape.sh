#!/usr/bin/env bash
set -uo pipefail
# Criterion 10 (#73): text-mode errors are escaped in one place. No per-site escape is left in the
# CLI (the refresh `.map_err` that set `err.message = printable(..)`), and `fn report` renders
# the error through `error_text`.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

status=0
file=src/cli/mod.rs

site=$(grep -nE 'err\.message[[:space:]]*=[[:space:]]*printable' "$file" || true)
if [ -n "$site" ]; then
    while IFS= read -r line; do
        echo "FOUND: per-site escape at $file:${line}"
    done <<<"$site"
    status=1
fi

body=$(awk '/^(pub )?fn report\(/ {on=1} on {print} on && /^}/ {exit}' "$file")
if [ -z "$body" ]; then
    echo "MISSING: $file has no fn report"
    status=1
elif ! printf '%s\n' "$body" | grep -qE 'error_text\('; then
    echo "MISSING: fn report in $file does not call error_text"
    status=1
fi
exit "$status"
