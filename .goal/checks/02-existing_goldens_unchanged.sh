#!/usr/bin/env bash
set -uo pipefail
# Guard (#11): the output shape does not change and no existing golden holds a type whose
# `editable` changes (they only hold text/plain, text/x-forget and application/octet-stream),
# so no golden snapshot that exists on the base branch may change in this PR (the goal's own
# goldens are new files). A guard, not a red: it passes before the change and fails if an
# implementer re-blesses a snapshot.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

base=origin/main
git rev-parse --verify --quiet "$base" >/dev/null || base=main
git rev-parse --verify --quiet "$base" >/dev/null || {
    echo "MISSING: neither origin/main nor main exists to compare the goldens against"
    exit 1
}

if ! changed=$(git diff --name-status --diff-filter=MDRT "$base" -- 'tests/snapshots/*.snap'); then
    echo "MISSING: could not diff the existing goldens against $base"
    exit 1
fi
if [ -n "$changed" ]; then
    echo "CHANGED: existing golden snapshots differ from $base (they must not change in #11):"
    printf '%s\n' "$changed"
    exit 1
fi
exit 0
