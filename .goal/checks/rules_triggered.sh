#!/usr/bin/env bash
set -uo pipefail
# Criterion 5 (#15, spec §14.1): every error rule has a test that triggers it. The rules are
# the `Error::new("RULE", ...)` constructors in src/core/mod.rs; a rule counts as triggered
# when a test target other than the schema tests names it. The new rule, UNKNOWN_TYPE, must
# have a constructor, and it and IN_USE must be triggered in both the core and the CLI tests of
# the Type commands.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

status=0
src=src/core/mod.rs

if ! grep -q '"UNKNOWN_TYPE"' "$src"; then
    echo "MISSING: $src has no UNKNOWN_TYPE error constructor (Error::unknown_type)"
    status=1
fi

# Every rule constructor in core: a quoted SCREAMING_SNAKE first argument of Error::new.
rules=$(grep -Eo 'Error::new\(\s*"[A-Z][A-Z0-9_]*"' "$src" | grep -Eo '"[A-Z][A-Z0-9_]*"' | tr -d '"' | sort -u)
# Multi-line calls put the rule on the next line.
rules=$(printf '%s\n%s\n' "$rules" "$(grep -A1 -E 'Error::new\($' "$src" | grep -Eo '^\s*"[A-Z][A-Z0-9_]*"' | tr -d ' "')" | sort -u | grep -v '^$' | grep -vx 'USAGE')

for rule in $rules; do
    if ! grep -lE "\"$rule\"" tests/core_*.rs tests/cli_*.rs 2>/dev/null | grep -qvE 'core_schema|schema_contract'; then
        echo "MISSING: no test triggers error rule $rule (name it in tests/core_*.rs or tests/cli_*.rs)"
        status=1
    fi
done

for layer in tests/core_type.rs tests/cli_type.rs; do
    for rule in UNKNOWN_TYPE IN_USE; do
        if ! grep -q "\"$rule\"" "$layer"; then
            echo "MISSING: $layer does not trigger $rule"
            status=1
        fi
    done
done

exit "$status"
