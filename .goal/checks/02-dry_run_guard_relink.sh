#!/usr/bin/env bash
set -uo pipefail
# Criterion 20b (#21, spec §9.5): `relink` is mutating and supports `--dry-run`, so the permanent
# dry-run guard (tests/dry_run_guard.rs, which enumerates every leaf subcommand of the clap tree)
# needs a CASES row for it. The row searches src/ontology (two Turtle files, a root that exists in
# every checkout) with the `store` fixture as the store, whose Resources' files are gone. It
# passes no --yes: a search never asks.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

guard=tests/dry_run_guard.rs
status=0

row=$(grep -E "^[[:space:]]*\([[:space:]]*\"relink\"[[:space:]]*," "$guard" || true)
if [ -z "$row" ]; then
    echo "MISSING: $guard has no CASES row for \"relink\"; add (\"relink\", \"store\", &[\"relink\", \"--root\", \"src/ontology\"])"
    exit 1
fi
got=$(printf '%s\n' "$row" | tr -d ' ')
if ! printf '%s\n' "$got" | grep -qE -- '"store".*&\["relink","--root","src/ontology"\]'; then
    echo "MISSING: the \"relink\" row in $guard must name the \"store\" fixture and have the argv [\"relink\", \"--root\", \"src/ontology\"]; found: $row"
    status=1
fi
if printf '%s\n' "$row" | grep -q -- '--yes'; then
    echo "WRONG: the \"relink\" row in $guard passes --yes; a search never asks: $row"
    status=1
fi
exit "$status"
