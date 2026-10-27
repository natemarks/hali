#!/usr/bin/env bash
set -uo pipefail
# Criterion 6 (#12): tests/dry_run_guard.rs CASES has a `find` row, so the dry-run guard
# covers the new read-only subcommand (it must run `find` with valid argv, i.e. `find` itself).
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

if ! grep -Eq '^[[:space:]]*\("find",[[:space:]]*&\["find"' tests/dry_run_guard.rs; then
    echo 'MISSING: tests/dry_run_guard.rs CASES has no ("find", &["find", ...]) row'
    exit 1
fi
