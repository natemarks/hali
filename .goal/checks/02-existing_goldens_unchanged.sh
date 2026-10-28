#!/usr/bin/env bash
set -uo pipefail
# Criterion 7 (#15): `add`, `edit` and `find` behave as before for everything that does not use
# the new flags, so the goldens of the existing commands must not change in this PR. A guard,
# not a red: it passes before the change and fails if an implementer re-blesses a snapshot.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

base=origin/main
git rev-parse --verify --quiet "$base" >/dev/null || base=main
git rev-parse --verify --quiet "$base" >/dev/null || {
    echo "MISSING: neither origin/main nor main exists to compare the goldens against"
    exit 1
}

status=0
pathspecs=(
    'tests/snapshots/cli_resource__*.snap'
    'tests/snapshots/cli_tag__*.snap'
    'tests/snapshots/cli_find__find_json*.snap'
    'tests/snapshots/cli_find__find_tag*.snap'
    'tests/snapshots/cli_dry_run__add_*.snap'
    'tests/snapshots/cli_dry_run__edit_*.snap'
)
if ! changed=$(git diff --name-status "$base" -- "${pathspecs[@]}") ; then
    echo "MISSING: could not diff the existing goldens against $base"
    exit 1
fi
if [ -n "$changed" ]; then
    echo "CHANGED: existing golden snapshots differ from $base (they must not change in #15):"
    printf '%s\n' "$changed"
    status=1
fi
exit "$status"
