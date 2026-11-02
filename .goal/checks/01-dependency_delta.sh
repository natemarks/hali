#!/usr/bin/env bash
set -uo pipefail
export LC_ALL=C # sort and comm must agree
# Criterion 7 (#131): the dependency delta is exactly `ignore = "=0.4.34"` in Cargo.toml (right
# after the clap line) and exactly the packages ignore 0.4.34 and globset 0.4.18 added to
# Cargo.lock; no other package is added, removed or changes version.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

base=$(git merge-base HEAD origin/main 2>/dev/null || git merge-base HEAD main 2>/dev/null || true)
if [ -z "$base" ]; then
    echo "MISSING: no merge base with origin/main or main (shallow clone?); run 'git fetch origin main' so the dependency delta can be compared"
    exit 1
fi

status=0

# Cargo.toml: the diff against the merge base, ignoring headers, is exactly one added line.
toml_diff=$(git diff --unified=0 "$base" -- Cargo.toml | grep -E '^[+-]' | grep -vE '^(\+\+\+|---) ' || true)
if [ "$toml_diff" != '+ignore = "=0.4.34"' ]; then
    if ! grep -qxF 'ignore = "=0.4.34"' Cargo.toml; then
        echo 'MISSING: ignore = "=0.4.34" in Cargo.toml'
    fi
    if [ -n "$toml_diff" ] && [ "$toml_diff" != '+ignore = "=0.4.34"' ]; then
        echo "CHANGED: Cargo.toml must differ from $base by exactly the added line ignore = \"=0.4.34\"; the diff is:"
        printf '%s\n' "$toml_diff"
    fi
    status=1
else
    # It sits right after the clap line.
    clap_line=$(grep -nE '^clap = ' Cargo.toml | head -n1 | cut -d: -f1)
    ignore_line=$(grep -nxF 'ignore = "=0.4.34"' Cargo.toml | head -n1 | cut -d: -f1)
    if [ -z "$clap_line" ] || [ "$ignore_line" != "$((clap_line + 1))" ]; then
        echo 'CHANGED: the line ignore = "=0.4.34" must come right after the clap line in Cargo.toml'
        status=1
    fi
fi

# Cargo.lock: the set of "name version" pairs, before and after.
pkgs() {
    awk '
        /^name = / { gsub(/"/, "", $3); name = $3 }
        /^version = / { gsub(/"/, "", $3); print name " " $3 }
    ' | sort
}
if ! was=$(git show "$base:Cargo.lock" | pkgs) || [ -z "$was" ]; then
    echo "MISSING: could not read Cargo.lock at $base"
    exit 1
fi
now=$(pkgs <Cargo.lock)
added=$(comm -13 <(printf '%s\n' "$was") <(printf '%s\n' "$now"))
removed=$(comm -23 <(printf '%s\n' "$was") <(printf '%s\n' "$now"))
expected=$(printf 'globset 0.4.18\nignore 0.4.34\n')

if [ "$added" != "$expected" ]; then
    for want in "ignore 0.4.34" "globset 0.4.18"; do
        printf '%s\n' "$added" | grep -qxF "$want" || echo "MISSING: package $want in Cargo.lock"
    done
    extra=$(printf '%s\n' "$added" | grep -vxF -e "ignore 0.4.34" -e "globset 0.4.18" || true)
    if [ -n "$extra" ]; then
        echo "CHANGED: Cargo.lock gains packages beyond ignore and globset (or changes a version):"
        printf '%s\n' "$extra"
    fi
    status=1
fi
if [ -n "$removed" ]; then
    echo "CHANGED: Cargo.lock loses packages or versions that main has:"
    printf '%s\n' "$removed"
    status=1
fi
exit "$status"
