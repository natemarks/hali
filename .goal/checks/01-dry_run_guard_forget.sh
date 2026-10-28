#!/usr/bin/env bash
set -uo pipefail
# Criterion 4 (#14, spec §9.5): `--dry-run` is supported by forget, so the permanent dry-run
# guard (tests/dry_run_guard.rs, which enumerates every leaf subcommand of the clap tree) needs
# a CASES row for it. The row must not pass --yes: a dry run never asks for confirmation.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

status=0
guard=tests/dry_run_guard.rs

row=$(grep -E '^[[:space:]]*\([[:space:]]*"forget"[[:space:]]*,[[:space:]]*&\[' "$guard" || true)
if [ -z "$row" ]; then
    echo "MISSING: $guard has no CASES row for forget; add (\"forget\", &[\"forget\", STORED])"
    status=1
else
    if ! printf '%s\n' "$row" | grep -Eq '&\[[[:space:]]*"forget"[[:space:]]*,[[:space:]]*STORED[[:space:]]*\]'; then
        echo "MISSING: the forget row in $guard must be (\"forget\", &[\"forget\", STORED]); found: $row"
        status=1
    fi
    if printf '%s\n' "$row" | grep -q -- '--yes'; then
        echo "WRONG: the forget row in $guard passes --yes; a dry run must work without it: $row"
        status=1
    fi
fi

exit "$status"
