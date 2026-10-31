#!/usr/bin/env bash
set -uo pipefail
# Criterion 17 (#20): the security-sensitive seams are untouched and no error rule is added.
# `refresh` reads through the existing `storage::stat` and `storage::inspect`, adds no dependency
# and no rule: src/storage, src/paths, src/media_type, Cargo.toml, Cargo.lock and the error
# schema are as on the base branch, and no new SCREAMING_SNAKE rule string appears in
# src/core/mod.rs or src/core/resource.rs. A guard, not a red: it passes before the change.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

base=origin/main
git rev-parse --verify --quiet "$base" >/dev/null || base=main
git rev-parse --verify --quiet "$base" >/dev/null || {
    echo "MISSING: neither origin/main nor main exists to compare against"
    exit 1
}

status=0
paths=(src/storage src/paths src/media_type Cargo.toml Cargo.lock src/core/schemas/error.json)
if ! git diff --quiet "$base" -- "${paths[@]}"; then
    echo "CHANGED: a protected path differs from $base (refresh must not touch these):"
    git diff --name-status "$base" -- "${paths[@]}"
    status=1
fi
untracked=$(git ls-files --others --exclude-standard -- "${paths[@]}")
if [ -n "$untracked" ]; then
    echo "NEW FILES: untracked files in a protected path:"
    printf '%s\n' "$untracked"
    status=1
fi

rules() {
    grep -oE '"[A-Z][A-Z_]{3,}"' | sort -u
}
for file in src/core/mod.rs src/core/resource.rs; do
    old=$(git show "$base:$file" | rules)
    new=$(rules <"$file")
    added=$(comm -13 <(printf '%s\n' "$old") <(printf '%s\n' "$new"))
    if [ -n "$added" ]; then
        echo "NEW RULE: $file gained rule-like strings (refresh adds no error rule):"
        printf '%s\n' "$added"
        status=1
    fi
done
exit "$status"
