#!/usr/bin/env bash
set -uo pipefail
# Criterion 14 (#9, spec §9.5): `query` is a new leaf subcommand, so the permanent dry-run guard
# (tests/dry_run_guard.rs) must have a CASES row for it with valid argv: the guard then proves
# `hali --dry-run query ...` writes nothing, with and without a store, in text and JSON.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

guard=tests/dry_run_guard.rs
row=$(grep -E "^[[:space:]]*\([[:space:]]*\"query\"[[:space:]]*," "$guard" || true)
if [ -z "$row" ]; then
    echo "MISSING: $guard has no CASES row for \"query\"; add (\"query\", \"relations\", &[\"query\", \"ASK {}\"])"
    exit 1
fi
got=$(printf '%s\n' "$row" | tr -d ' ')
if ! printf '%s\n' "$got" | grep -qE -- '&\["query","[^"]+"\]'; then
    echo "MISSING: the \"query\" row in $guard must have the argv [\"query\", \"<sparql>\"]; found: $row"
    exit 1
fi
exit 0
