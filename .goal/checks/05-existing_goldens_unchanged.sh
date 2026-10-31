#!/usr/bin/env bash
set -uo pipefail
# Criterion 18 (#20): every existing command prints what it printed, so no existing golden
# snapshot may change in this PR; this proves the refactor of `add`'s triple code into a shared
# builder kept its output byte-identical. The one exception is cli_schema__schema_list_json,
# which lists the schema names and gains `refresh`; the new cli_refresh__* and
# cli_dry_run__refresh_* goldens are the goal's own. A guard, not a red: it passes before the
# change and fails if an implementer re-blesses a snapshot.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

base=origin/main
git rev-parse --verify --quiet "$base" >/dev/null || base=main
git rev-parse --verify --quiet "$base" >/dev/null || {
    echo "MISSING: neither origin/main nor main exists to compare the goldens against"
    exit 1
}

status=0
if ! changed=$(git diff --name-status "$base" -- 'tests/snapshots/*.snap' \
    ':(exclude)tests/snapshots/cli_refresh__*' \
    ':(exclude)tests/snapshots/cli_dry_run__refresh_*' \
    ':(exclude)tests/snapshots/cli_schema__schema_list_json.snap'); then
    echo "MISSING: could not diff the existing goldens against $base"
    exit 1
fi
if [ -n "$changed" ]; then
    echo "CHANGED: existing golden snapshots differ from $base (they must not change in #20):"
    printf '%s\n' "$changed"
    status=1
fi
exit "$status"
