#!/usr/bin/env bash
set -uo pipefail
# Criterion 13 (#19): the security-sensitive seams are untouched. `describe` reads through the
# existing `storage::load` and the existing `paths::data_dir` call, adds no dependency and no
# write. A guard, not a red: it passes before the change and fails if src/paths, src/storage,
# Cargo.toml or Cargo.lock change.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

base=origin/main
git rev-parse --verify --quiet "$base" >/dev/null || base=main
git rev-parse --verify --quiet "$base" >/dev/null || {
    echo "MISSING: neither origin/main nor main exists to compare against"
    exit 1
}

status=0
paths=(src/paths src/storage Cargo.toml Cargo.lock)
if ! git diff --quiet "$base" -- "${paths[@]}"; then
    echo "CHANGED: a security-sensitive path differs from $base (describe must not touch these):"
    git diff --name-status "$base" -- "${paths[@]}"
    status=1
fi
untracked=$(git ls-files --others --exclude-standard -- "${paths[@]}")
if [ -n "$untracked" ]; then
    echo "NEW FILES: untracked files in a security-sensitive path:"
    printf '%s\n' "$untracked"
    status=1
fi
exit "$status"
