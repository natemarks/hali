#!/usr/bin/env bash
set -uo pipefail
# Criterion 22 (#24, spec §9.5): `scan --prune` is destructive, so the permanent dry-run guard
# (tests/dry_run_guard.rs) must run the scan row with `--prune --force`: the `store` fixture's
# Resources are all Missing and outside src/ontology, so the dry run has nothing to forget there,
# but the row proves the flags parse and a dry run never asks. It passes no --yes.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

guard=tests/dry_run_guard.rs
status=0

row=$(grep -E "^[[:space:]]*\([[:space:]]*\"scan\"[[:space:]]*," "$guard" || true)
if [ -z "$row" ]; then
    echo "MISSING: $guard has no CASES row for \"scan\"; add (\"scan\", \"store\", &[\"scan\", \"src/ontology\", \"--prune\", \"--force\"])"
    exit 1
fi
got=$(printf '%s\n' "$row" | tr -d ' ')
if ! printf '%s\n' "$got" | grep -qE -- '"store".*&\["scan","src/ontology","--prune","--force"\]'; then
    echo "MISSING: the \"scan\" row in $guard must name the \"store\" fixture and have the argv [\"scan\", \"src/ontology\", \"--prune\", \"--force\"]; found: $row"
    status=1
fi
if printf '%s\n' "$row" | grep -q -- '--yes'; then
    echo "WRONG: the \"scan\" row in $guard passes --yes; a dry run never asks: $row"
    status=1
fi
exit "$status"
