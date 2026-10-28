#!/usr/bin/env bash
set -uo pipefail
# Criterion 9 (#17, spec §9.5): `--dry-run` is supported by the four new Concept commands, so the
# permanent dry-run guard (tests/dry_run_guard.rs, which enumerates every leaf subcommand of the
# clap tree) needs a CASES row for each. No row passes --yes: a dry run never asks for
# confirmation, and `concept show` is read-only.
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

check_row "concept edit" '"concept", "edit", "ml", "--add-alt", "guard probe"'
check_row "concept remove" '"concept", "remove", "a-first"'
check_row "concept rename" '"concept", "rename", "ml", "guard probe"'
check_row "concept show" '"concept", "show", "ml"'

exit "$status"
