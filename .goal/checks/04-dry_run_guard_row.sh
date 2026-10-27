#!/usr/bin/env bash
set -uo pipefail
# Criterion 4 (#6): tests/dry_run_guard.rs CASES has the `schema` row, so the dry-run guard
# covers the new subcommand.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

if grep -Eq '^\s*\("schema",\s*&\["schema"\]\),' tests/dry_run_guard.rs; then
    exit 0
fi
echo 'MISSING: tests/dry_run_guard.rs CASES has no ("schema", &["schema"]) row'
exit 1
