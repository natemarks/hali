#!/usr/bin/env bash
set -uo pipefail
# Criterion (#10): every non-goal `cargo test` recipe line in the Makefile enables
# `--features test-hooks` (CLI goldens and cli_resource::test_hooks_are_compiled_in need it
# once settled). Covers unit, unit-update-golden, test-external and integration.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

missing=0
target=""
while IFS= read -r line; do
    case "$line" in
        [A-Za-z0-9_.-]*:*) target="${line%%:*}" ;;
    esac
    case "$line" in
        $'\t'*cargo\ test*)
            case "$line" in
                *--features*goal*) continue ;; # goal-tests carries the goal tag
            esac
            if ! printf '%s' "$line" | grep -Eq -- '--features[ =][^ ]*test-hooks'; then
                echo "MISSING: Makefile recipe '$target' runs cargo test without --features test-hooks: ${line#"${line%%[![:space:]]*}"}"
                missing=1
            fi
            ;;
    esac
done < Makefile
exit "$missing"
