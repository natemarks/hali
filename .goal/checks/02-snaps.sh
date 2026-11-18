#!/usr/bin/env bash
set -uo pipefail
# Criterion 15 and the guard (#25): `new` joins the schema list, so exactly three existing
# goldens change, each by gaining `"new",` right after `"forget",` (before `"query"`; the schema list
# of cli_schema, cli_relink and cli_scan); no other existing golden may change. The new
# cli_new__* and cli_dry_run__new_* goldens are the goal's own and locked.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

base=origin/main
git rev-parse --verify --quiet "$base" >/dev/null || base=main
git rev-parse --verify --quiet "$base" >/dev/null || {
    echo "MISSING: neither origin/main nor main exists to compare the goldens against"
    exit 1
}

status=0
lists=(
    tests/snapshots/cli_schema__schema_list_json.snap
    tests/snapshots/cli_relink__schema_list_json.snap
    tests/snapshots/cli_scan__schema_list_json.snap
)
for snap in "${lists[@]}"; do
    if ! grep -qF '"forget","new","' "$snap" || [ "$(grep -o '"new"' "$snap" | wc -l)" -ne 1 ]; then
        echo "MISSING: $snap does not list \"new\" exactly once, right after \"forget\""
        status=1
        continue
    fi
    # Apart from that one name, the snapshot is what main has.
    if ! git show "$base:$snap" >/dev/null 2>&1; then
        echo "MISSING: $snap does not exist on $base"
        status=1
        continue
    fi
    if ! diff -q <(git show "$base:$snap") <(sed 's/"forget","new","/"forget","/' "$snap") >/dev/null; then
        echo "CHANGED: $snap differs from $base by more than the name \"new\""
        status=1
    fi
done

if ! changed=$(git diff --name-status "$base" -- 'tests/snapshots/*.snap' \
    ':(exclude)tests/snapshots/cli_new__*' \
    ':(exclude)tests/snapshots/cli_dry_run__new_*' \
    ':(exclude)tests/snapshots/cli_schema__schema_list_json.snap' \
    ':(exclude)tests/snapshots/cli_relink__schema_list_json.snap' \
    ':(exclude)tests/snapshots/cli_scan__schema_list_json.snap'); then
    echo "MISSING: could not diff the existing goldens against $base"
    exit 1
fi
if [ -n "$changed" ]; then
    echo "CHANGED: existing golden snapshots differ from $base (they must not change in #25):"
    printf '%s\n' "$changed"
    status=1
fi
exit "$status"
