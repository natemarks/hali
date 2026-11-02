#!/usr/bin/env bash
set -uo pipefail
# Criterion 10 (#131): the walker adds no command and no output, so no existing golden snapshot
# may change in this PR. A guard, not a red: it passes before the change and fails if an
# implementer re-blesses a snapshot.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

base=origin/main
git rev-parse --verify --quiet "$base" >/dev/null || base=main
git rev-parse --verify --quiet "$base" >/dev/null || {
    echo "MISSING: neither origin/main nor main exists to compare the goldens against"
    exit 1
}

if ! changed=$(git diff --name-status "$base" -- 'tests/snapshots/*.snap'); then
    echo "MISSING: could not diff the existing goldens against $base"
    exit 1
fi
if [ -n "$changed" ]; then
    echo "CHANGED: golden snapshots differ from $base (they must not change in #131):"
    printf '%s\n' "$changed"
    exit 1
fi
exit 0
