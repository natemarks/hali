#!/usr/bin/env bash
set -uo pipefail
# Guard (#5): locking and the safety rules add an error rule but change no existing output, so no
# golden snapshot that exists on the base branch may change in this PR (the goal's own golden,
# cli_lock__store_locked_json_envelope, is a new file). A guard, not a red: it passes before the
# change and fails if an implementer re-blesses a snapshot.
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
    echo "CHANGED: existing golden snapshots differ from $base (they must not change in #5):"
    printf '%s\n' "$changed"
    exit 1
fi
exit 0
