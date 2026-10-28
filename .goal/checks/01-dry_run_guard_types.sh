#!/usr/bin/env bash
set -uo pipefail
# Criterion 5 (#15, spec §9.5): `--dry-run` is supported by the new Type commands, so the
# permanent dry-run guard (tests/dry_run_guard.rs, which enumerates every leaf subcommand of the
# clap tree) needs a CASES row for each. No row passes --yes: a dry run never asks for
# confirmation, and `type list` is read-only.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

status=0
guard=tests/dry_run_guard.rs

# name|expected argv, as it must appear inside `&[ ... ]` (spaces ignored when comparing)
check_row() {
    local name=$1 want=$2 row got
    row=$(grep -E "^[[:space:]]*\([[:space:]]*\"$name\"[[:space:]]*,[[:space:]]*&\[" "$guard" || true)
    if [ -z "$row" ]; then
        echo "MISSING: $guard has no CASES row for \"$name\"; add (\"$name\", &[$want])"
        status=1
        return
    fi
    got=$(printf '%s\n' "$row" | tr -d ' ')
    if ! printf '%s\n' "$got" | grep -qF "&[$(printf '%s' "$want" | tr -d ' ')]"; then
        echo "MISSING: the \"$name\" row in $guard must be (\"$name\", &[$want]); found: $row"
        status=1
    fi
    if printf '%s\n' "$row" | grep -q -- '--yes'; then
        echo "WRONG: the \"$name\" row in $guard passes --yes; a dry run must work without it: $row"
        status=1
    fi
}

check_row "type add" '"type", "add", "GuardProbe"'
check_row "type list" '"type", "list"'
check_row "type remove" '"type", "remove", "Paper"'

exit "$status"
