#!/usr/bin/env bash
set -uo pipefail
# Criterion 8 (#18, spec §9.5): `--dry-run` is supported by the new relation commands, so the
# permanent dry-run guard (tests/dry_run_guard.rs, which enumerates every leaf subcommand of the
# clap tree) needs a CASES row for each. The rows run on the `relations` fixture (the `store`
# fixture has no relation kind and only one Resource): each row names it, as a "relations"
# string next to the argv. No row passes --yes: a dry run never asks for confirmation, and
# `relation list` is read-only.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

status=0
guard=tests/dry_run_guard.rs

# name|pattern for the argv, written on the row with spaces removed. `[^],]+` stands for an
# argument the implementer chooses (a Resource id, a constant, a name).
check_row() {
    local name=$1 argv=$2 row got
    row=$(grep -E "^[[:space:]]*\([[:space:]]*\"$name\"[[:space:]]*," "$guard" || true)
    if [ -z "$row" ]; then
        echo "MISSING: $guard has no CASES row for \"$name\"; add (\"$name\", \"relations\", &[...])"
        status=1
        return
    fi
    got=$(printf '%s\n' "$row" | tr -d ' ')
    if ! printf '%s\n' "$got" | grep -qE -- "\"relations\".*&\[$argv\]"; then
        echo "MISSING: the \"$name\" row in $guard must name the \"relations\" fixture and have the argv /$argv/; found: $row"
        status=1
    fi
    if printf '%s\n' "$row" | grep -q -- '--yes'; then
        echo "WRONG: the \"$name\" row in $guard passes --yes; a dry run must work without it: $row"
        status=1
    fi
}

arg='[^],]+'
check_row "relate" "\"relate\",$arg,$arg,$arg"
check_row "unrelate" "\"unrelate\",$arg,$arg,$arg"
check_row "relation add" "\"relation\",\"add\",$arg,\"--parent\",$arg"
check_row "relation list" "\"relation\",\"list\""
check_row "relation remove" "\"relation\",\"remove\",$arg"

exit "$status"
