#!/usr/bin/env bash
set -uo pipefail
# Criterion 9 (#10): tests/dry_run_guard.rs CASES has a row for each new subcommand, so the
# dry-run guard covers them. The rows need valid argv (a real file for `add`, an id for
# `show`/`edit`), so each is matched loosely on its leading words.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

missing=0
for name in add show edit; do
    if ! grep -Eq "^[[:space:]]*\(\"$name\",[[:space:]]*&\[\"$name\"," tests/dry_run_guard.rs; then
        echo "MISSING: tests/dry_run_guard.rs CASES has no (\"$name\", &[\"$name\", ...]) row"
        missing=1
    fi
done
if ! grep -Eq '"edit"[^)]*"--title"' tests/dry_run_guard.rs; then
    echo 'MISSING: the "edit" CASES row must pass --title (edit with no flag is a usage error)'
    missing=1
fi
exit "$missing"
