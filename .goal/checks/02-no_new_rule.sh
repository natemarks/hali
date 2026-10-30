#!/usr/bin/env bash
set -uo pipefail
# Criterion 10 (#19, spec §14.1): `describe` introduces no error rule. The error envelope schema
# (src/core/schemas/error.json) is unchanged, no new SCREAMING_SNAKE rule string appears in
# src/core/mod.rs, and the new src/core/describe.rs (if any) names no rule. A guard, not a red:
# it passes before the change and fails if an implementer adds a rule.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

base=origin/main
git rev-parse --verify --quiet "$base" >/dev/null || base=main
git rev-parse --verify --quiet "$base" >/dev/null || {
    echo "MISSING: neither origin/main nor main exists to compare against"
    exit 1
}

status=0

if ! git diff --quiet "$base" -- src/core/schemas/error.json; then
    echo "CHANGED: src/core/schemas/error.json differs from $base; describe adds no error rule"
    status=1
fi

rules() {
    grep -oE '"[A-Z][A-Z_]{3,}"' | sort -u
}
old=$(git show "$base:src/core/mod.rs" | rules)
new=$(rules <src/core/mod.rs)
added=$(comm -13 <(printf '%s\n' "$old") <(printf '%s\n' "$new"))
if [ -n "$added" ]; then
    echo "NEW RULE: src/core/mod.rs gained rule strings (describe adds none):"
    printf '%s\n' "$added"
    status=1
fi

if [ -e src/core/describe.rs ] && grep -qE 'Error::|"[A-Z][A-Z_]{3,}"' src/core/describe.rs; then
    echo "NEW RULE: src/core/describe.rs builds an error or names a rule (describe cannot fail)"
    status=1
fi
exit "$status"
