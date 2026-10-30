#!/usr/bin/env bash
set -uo pipefail
# Criterion 9 (#19, spec §9.5): `describe` is read-only and ignores `--dry-run`, so the
# permanent dry-run guard (tests/dry_run_guard.rs, which enumerates every leaf subcommand of the
# clap tree) needs a CASES row for it. The row runs on the `relations` fixture (a store with
# Concepts, Types and relation kinds to describe) and is a single line `hali describe`; it does
# not pass --yes: a read-only command never asks for confirmation.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

guard=tests/dry_run_guard.rs
status=0

row=$(grep -E "^[[:space:]]*\([[:space:]]*\"describe\"[[:space:]]*," "$guard" || true)
if [ -z "$row" ]; then
    echo "MISSING: $guard has no CASES row for \"describe\"; add (\"describe\", \"relations\", &[\"describe\"])"
    exit 1
fi
got=$(printf '%s\n' "$row" | tr -d ' ')
if ! printf '%s\n' "$got" | grep -qE -- '"relations".*&\["describe"\]'; then
    echo "MISSING: the \"describe\" row in $guard must name the \"relations\" fixture and have the argv [\"describe\"]; found: $row"
    status=1
fi
if printf '%s\n' "$row" | grep -q -- '--yes'; then
    echo "WRONG: the \"describe\" row in $guard passes --yes; a read-only command must work without it: $row"
    status=1
fi
exit "$status"
