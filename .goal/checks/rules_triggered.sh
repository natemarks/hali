#!/usr/bin/env bash
set -uo pipefail
# Criterion 8 (#18, spec §14.1): every error rule has a test that triggers it. The rules are
# the `Error::new("RULE", ...)` constructors in src/core/mod.rs; a rule counts as triggered
# when a test target other than the schema tests names it. The new rules, UNKNOWN_RELATION and
# FORGET_BLOCKED_BY_RELATIONS, must have a constructor, and they, IN_USE and LABEL_TAKEN must
# be triggered in both the core and the CLI tests of the relation commands.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

status=0
src=src/core/mod.rs

for rule in UNKNOWN_RELATION FORGET_BLOCKED_BY_RELATIONS; do
    if ! grep -q "\"$rule\"" "$src"; then
        echo "MISSING: $src has no $rule error constructor"
        status=1
    fi
done

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

for layer in tests/core_relation.rs tests/cli_relation.rs; do
    for rule in UNKNOWN_RELATION IN_USE LABEL_TAKEN FORGET_BLOCKED_BY_RELATIONS; do
        if ! grep -q "\"$rule\"" "$layer"; then
            echo "MISSING: $layer does not trigger $rule"
            status=1
        fi
    done
done

exit "$status"
