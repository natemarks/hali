#!/usr/bin/env bash
set -uo pipefail
# #4 row 13: exact pins for the two new crates with default features, and Cargo.lock gains only
# clap_complete, clap_mangen and roff (nothing else added, removed or re-versioned).
# `cargo audit` is part of make static-check, so it is not repeated here.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

export LC_ALL=C
fail=0
pin() {
    if ! grep -qE "^$1[[:space:]]*=[[:space:]]*\"=$2\"[[:space:]]*\$" Cargo.toml; then
        echo "MISSING: Cargo.toml has no exact pin \`$1 = \"=$2\"\` with default features"
        fail=1
    fi
}
pin clap_complete 4.6.12
pin clap_mangen 0.3.4
if grep -q 'unstable' Cargo.toml; then
    echo "FOUND: Cargo.toml mentions an unstable feature"
    fail=1
fi

base=origin/main
git rev-parse --verify --quiet "$base" >/dev/null || base=main
base=$(git merge-base HEAD "$base") || {
    echo "MISSING: no merge-base with the default branch"
    exit 1
}
pairs() {
    awk '/^name = /{n=$3} /^version = /{print n, $3}' | sort
}
now=$(pairs <Cargo.lock)
was=$(git show "$base:Cargo.lock" | pairs)
removed=$(comm -23 <(printf '%s\n' "$was") <(printf '%s\n' "$now"))
added=$(comm -13 <(printf '%s\n' "$was") <(printf '%s\n' "$now") | awk '{print $1}' | tr -d '"' | tr '\n' ' ')
if [ -n "$removed" ]; then
    echo "CHANGED: Cargo.lock removed or re-versioned packages:"
    printf '%s\n' "$removed"
    fail=1
fi
if [ "$added" != "clap_complete clap_mangen roff " ]; then
    echo "MISSING: Cargo.lock should add exactly clap_complete, clap_mangen and roff; added: ${added:-nothing}"
    fail=1
fi
if ! cargo tree --locked -e normal --prefix none >/dev/null 2>&1; then
    echo "FAILED: cargo tree --locked -e normal does not resolve"
    fail=1
fi
exit "$fail"
