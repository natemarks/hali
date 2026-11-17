#!/usr/bin/env bash
set -uo pipefail
# Guard (#9): every existing command prints what it printed, so no golden snapshot that exists
# on the base branch may change in this PR (the goal's own goldens are new files). The one
# exception is the schema list: `hali schema --json` gains the name `query`, so the three
# goldens that hold the list (cli_schema__, cli_scan__ and cli_relink__schema_list_json) may
# change, each held to exactly that one added entry. A guard, not a red: it passes before the
# change and fails if an implementer re-blesses any other snapshot.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

base=origin/main
git rev-parse --verify --quiet "$base" >/dev/null || base=main
git rev-parse --verify --quiet "$base" >/dev/null || {
    echo "MISSING: neither origin/main nor main exists to compare the goldens against"
    exit 1
}

status=0
if ! changed=$(git diff --name-status "$base" -- 'tests/snapshots/*.snap' \
    ':(exclude)tests/snapshots/cli_query__*' \
    ':(exclude)tests/snapshots/cli_schema__schema_list_json.snap' \
    ':(exclude)tests/snapshots/cli_scan__schema_list_json.snap' \
    ':(exclude)tests/snapshots/cli_relink__schema_list_json.snap'); then
    echo "MISSING: could not diff the existing goldens against $base"
    exit 1
fi
if [ -n "$changed" ]; then
    echo "CHANGED: existing golden snapshots differ from $base (they must not change in #9):"
    printf '%s\n' "$changed"
    status=1
fi

for name in cli_schema__schema_list_json cli_scan__schema_list_json cli_relink__schema_list_json; do
    snap=tests/snapshots/$name.snap
    diff_body=$(git diff -U0 "$base" -- "$snap" | grep -E '^[+-]' | grep -vE '^(\+\+\+|---) ')
    [ -z "$diff_body" ] && continue
    removed=$(printf '%s\n' "$diff_body" | grep '^-' | cut -c2-)
    added=$(printf '%s\n' "$diff_body" | grep '^+' | cut -c2-)
    # The list is one JSON line: removing the one new name from it must give the base line back.
    if [ "$(printf '%s\n' "$removed" | wc -l)" -ne 1 ] ||
        [ "$(printf '%s\n' "$added" | wc -l)" -ne 1 ] ||
        [ "${added/\"query\",/}" != "$removed" ]; then
        echo "CHANGED: $snap may differ from $base only by the added \"query\" entry:"
        printf '%s\n' "$diff_body"
        status=1
    fi
done
exit "$status"
