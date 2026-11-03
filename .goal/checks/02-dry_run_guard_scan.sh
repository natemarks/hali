#!/usr/bin/env bash
set -uo pipefail
# Criterion 15b (#22, spec §9.5): `scan` is mutating and supports `--dry-run`, so the permanent
# dry-run guard (tests/dry_run_guard.rs, which enumerates every leaf subcommand of the clap tree)
# needs a CASES row for it. The row scans src/ontology (two Turtle files, a root that exists in
# every checkout) with the `store` fixture as the store. It passes no --yes: scan never asks.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

guard=tests/dry_run_guard.rs
status=0

row=$(grep -E "^[[:space:]]*\([[:space:]]*\"scan\"[[:space:]]*," "$guard" || true)
if [ -z "$row" ]; then
    echo "MISSING: $guard has no CASES row for \"scan\"; add (\"scan\", \"store\", &[\"scan\", \"src/ontology\"])"
    exit 1
fi
got=$(printf '%s\n' "$row" | tr -d ' ')
if ! printf '%s\n' "$got" | grep -qE -- '"store".*&\["scan","src/ontology"\]'; then
    echo "MISSING: the \"scan\" row in $guard must name the \"store\" fixture and have the argv [\"scan\", \"src/ontology\"]; found: $row"
    status=1
fi
if printf '%s\n' "$row" | grep -q -- '--yes'; then
    echo "WRONG: the \"scan\" row in $guard passes --yes; scan never asks: $row"
    status=1
fi
exit "$status"
