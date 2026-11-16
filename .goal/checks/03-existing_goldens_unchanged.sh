#!/usr/bin/env bash
set -uo pipefail
# Criterion 23 (#24): every existing command prints what it printed, so no existing golden
# snapshot may change in this PR, except the two scan goldens, which gain exactly the new empty
# keys (`counts.relinked`, `counts.ambiguous`, `counts.pruned`, and the lists `relinked`,
# `ambiguous`, `pruned`); the four new goldens are the goal's own. No schema name is added, so
# even cli_schema__schema_list_json stays as it is. A guard, not a red: it passes before the
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
    ':(exclude)tests/snapshots/cli_scan__scan_json.snap' \
    ':(exclude)tests/snapshots/cli_dry_run__scan_dry_run_json.snap' \
    ':(exclude)tests/snapshots/cli_scan__scan_moves_json.snap' \
    ':(exclude)tests/snapshots/cli_scan__scan_prune_json.snap' \
    ':(exclude)tests/snapshots/cli_dry_run__scan_prune_dry_run_json.snap' \
    ':(exclude)tests/snapshots/cli_scan__prune_confirmation_required_json_envelope.snap'); then
    echo "MISSING: could not diff the existing goldens against $base"
    exit 1
fi
if [ -n "$changed" ]; then
    echo "CHANGED: existing golden snapshots differ from $base (they must not change in #24):"
    printf '%s\n' "$changed"
    status=1
fi

for snap in cli_scan__scan_json cli_dry_run__scan_dry_run_json; do
    path=tests/snapshots/$snap.snap
    if ! old=$(git show "$base:$path"); then
        echo "MISSING: $base has no $path"
        status=1
        continue
    fi
    want=$(printf '%s\n' "$old" | sed \
        -e 's/"refreshed":0,"unchanged"/"refreshed":0,"relinked":0,"ambiguous":0,"unchanged"/' \
        -e 's/"missing":0,"skipped"/"missing":0,"pruned":0,"skipped"/' \
        -e 's/"refreshed":\[\],"missing":\[\],/"refreshed":[],"relinked":[],"ambiguous":[],"missing":[],"pruned":[],/')
    got=$(cat "$path" 2>/dev/null || true)
    if [ "$want" != "$got" ]; then
        echo "CHANGED: $path is not $base's with only the new keys inserted:"
        diff <(printf "%s\n" "$want") <(printf "%s\n" "$got") | cut -c1-300 | head -20
        status=1
    fi
done
exit "$status"
