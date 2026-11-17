#!/usr/bin/env bash
set -uo pipefail
# Guard (#4 row 8): `generate` has no JSON output and no schema, so the `hali schema` list does
# not change: no snapshot may differ from the merge-base's. A guard, not a red: it passes before
# the change and fails if an implementer re-blesses a snapshot.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

base=origin/main
git rev-parse --verify --quiet "$base" >/dev/null || base=main
git rev-parse --verify --quiet "$base" >/dev/null || {
    echo "MISSING: neither origin/main nor main exists to compare the snapshots against"
    exit 1
}
base=$(git merge-base HEAD "$base") || {
    echo "MISSING: no merge-base with the default branch"
    exit 1
}
shopt -s nullglob
snaps=(tests/snapshots/*schema_list_json.snap)
if [ ${#snaps[@]} -lt 3 ]; then
    echo "MISSING: expected three *schema_list_json.snap files, found ${#snaps[@]}"
    exit 1
fi
if ! changed=$(git diff --name-status "$base" -- 'tests/snapshots/*.snap'); then
    echo "MISSING: could not diff the snapshots against $base"
    exit 1
fi
if [ -n "$changed" ]; then
    echo "CHANGED: snapshots differ from $base (none may change in #4):"
    printf '%s\n' "$changed"
    exit 1
fi
if grep -l generate "${snaps[@]}" >/dev/null; then
    echo "CHANGED: a schema list snapshot mentions generate"
    exit 1
fi
exit 0
