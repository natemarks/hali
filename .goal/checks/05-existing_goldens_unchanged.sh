#!/usr/bin/env bash
set -uo pipefail
# Criterion 14 (#19): every existing command prints what it printed, so no existing golden
# snapshot may change in this PR. The one exception is cli_schema__schema_list_json, which lists
# the schema names and gains `describe`; the new cli_describe__* goldens are the goal's own. A
# guard, not a red: it passes before the change and fails if an implementer re-blesses a
# snapshot.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

base=origin/main
git rev-parse --verify --quiet "$base" >/dev/null || base=main
git rev-parse --verify --quiet "$base" >/dev/null || {
    echo "MISSING: neither origin/main nor main exists to compare the goldens against"
    exit 1
}

status=0
if ! changed=$(git diff --name-status "$base" -- 'tests/snapshots/*.snap' \
    ':(exclude)tests/snapshots/cli_describe__*' \
    ':(exclude)tests/snapshots/cli_schema__schema_list_json.snap'); then
    echo "MISSING: could not diff the existing goldens against $base"
    exit 1
fi
if [ -n "$changed" ]; then
    echo "CHANGED: existing golden snapshots differ from $base (they must not change in #19):"
    printf '%s\n' "$changed"
    status=1
fi
exit "$status"
