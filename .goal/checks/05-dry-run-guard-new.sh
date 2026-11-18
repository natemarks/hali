#!/usr/bin/env bash
set -uo pipefail
# Criterion 10 (#25, spec §9.5): `new` is mutating and supports `--dry-run`, so the permanent
# dry-run guard (tests/dry_run_guard.rs, which enumerates every leaf subcommand of the clap
# tree) needs a CASES row for it: the `store` fixture as the store and the argv
# ["new", "guard probe"]. The row cannot be written before the subcommand exists (the guard
# would fail on a CASES entry that is not a subcommand), so this check holds it.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

guard=tests/dry_run_guard.rs
row=$(grep -E "^[[:space:]]*\([[:space:]]*\"new\"[[:space:]]*," "$guard" || true)
if [ -z "$row" ]; then
    echo "MISSING: $guard has no CASES row for \"new\"; add (\"new\", \"store\", &[\"new\", \"guard probe\"])"
    exit 1
fi
got=$(printf '%s\n' "$row" | tr -d ' ')
if ! printf '%s\n' "$got" | grep -qE -- '"store".*&\["new","guardprobe"\]'; then
    echo "MISSING: the \"new\" row in $guard must name the \"store\" fixture and have the argv [\"new\", \"guard probe\"]; found: $row"
    exit 1
fi
if printf '%s\n' "$row" | grep -q -- '--yes'; then
    echo "WRONG: the \"new\" row in $guard passes --yes; new never asks: $row"
    exit 1
fi
exit 0
