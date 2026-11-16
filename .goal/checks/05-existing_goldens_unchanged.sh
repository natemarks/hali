#!/usr/bin/env bash
set -uo pipefail
# Criterion 22 (#21): every existing command prints what it printed, so no existing golden
# snapshot may change in this PR; this proves the `add` change (the POSSIBLE_MOVE check runs
# before `add_all` and adds nothing to a successful add) kept every output byte-identical. The one
# exceptions are the two schema-list goldens, cli_schema__schema_list_json and
# cli_scan__schema_list_json, which list the schema names and both gain `relink` (the second is
# held to exactly that one added entry); the
# new cli_relink__* and cli_dry_run__relink_* goldens are the goal's own. A guard, not a red: it
# passes before the change and fails if an implementer re-blesses a snapshot.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

base=origin/main
git rev-parse --verify --quiet "$base" >/dev/null || base=main
git rev-parse --verify --quiet "$base" >/dev/null || {
    echo "MISSING: neither origin/main nor main exists to compare the goldens against"
    exit 1
}

status=0
if ! changed=$(git diff --name-status "$base" -- 'tests/snapshots/*.snap' \
    ':(exclude)tests/snapshots/cli_relink__*' \
    ':(exclude)tests/snapshots/cli_dry_run__relink_*' \
    ':(exclude)tests/snapshots/cli_schema__schema_list_json.snap' \
    ':(exclude)tests/snapshots/cli_scan__schema_list_json.snap'); then
    echo "MISSING: could not diff the existing goldens against $base"
    exit 1
fi
if [ -n "$changed" ]; then
    echo "CHANGED: existing golden snapshots differ from $base (they must not change in #21):"
    printf '%s\n' "$changed"
    status=1
fi
scan_list=tests/snapshots/cli_scan__schema_list_json.snap
diff_body=$(git diff -U0 "$base" -- "$scan_list" | grep -E '^[+-]' | grep -vE '^(\+\+\+|---) ')
if [ -n "$diff_body" ]; then
    removed=$(printf '%s\n' "$diff_body" | grep '^-' | cut -c2-)
    added=$(printf '%s\n' "$diff_body" | grep '^+' | cut -c2-)
    # The list is one JSON line: removing the one new name from it must give the base line back.
    if [ "$(printf '%s\n' "$removed" | wc -l)" -ne 1 ] ||
        [ "$(printf '%s\n' "$added" | wc -l)" -ne 1 ] ||
        [ "${added/\"relink\",/}" != "$removed" ]; then
        echo "CHANGED: $scan_list may differ from $base only by the added \"relink\" entry:"
        printf '%s\n' "$diff_body"
        status=1
    fi
fi
exit "$status"
