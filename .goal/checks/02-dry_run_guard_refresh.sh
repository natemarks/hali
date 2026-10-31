#!/usr/bin/env bash
set -uo pipefail
# Criterion 12 (#20, spec §9.5): `refresh` is mutating and supports `--dry-run`, so the permanent
# dry-run guard (tests/dry_run_guard.rs, which enumerates every leaf subcommand of the clap tree)
# needs a CASES row for it. The row runs `hali refresh --all` on the `store` fixture, whose one
# Resource has no file in the test environment: it is reported as missing, which is not an
# error, so the guard also holds for a store whose files are gone. It passes no --yes: refresh
# never asks.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

guard=tests/dry_run_guard.rs
status=0

row=$(grep -E "^[[:space:]]*\([[:space:]]*\"refresh\"[[:space:]]*," "$guard" || true)
if [ -z "$row" ]; then
    echo "MISSING: $guard has no CASES row for \"refresh\"; add (\"refresh\", \"store\", &[\"refresh\", \"--all\"])"
    exit 1
fi
got=$(printf '%s\n' "$row" | tr -d ' ')
if ! printf '%s\n' "$got" | grep -qE -- '"store".*&\["refresh","--all"\]'; then
    echo "MISSING: the \"refresh\" row in $guard must name the \"store\" fixture and have the argv [\"refresh\", \"--all\"]; found: $row"
    status=1
fi
if printf '%s\n' "$row" | grep -q -- '--yes'; then
    echo "WRONG: the \"refresh\" row in $guard passes --yes; refresh never asks: $row"
    status=1
fi
exit "$status"
