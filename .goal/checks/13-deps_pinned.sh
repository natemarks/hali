#!/usr/bin/env bash
set -uo pipefail
# (#10) New dependencies pinned exactly (AGENTS.md Rules): xdg-mime =0.4.0, sha2 =0.11.1,
# percent-encoding =2.3.2. Cargo.lock must resolve to the same versions.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

missing=0
for pin in "xdg-mime 0.4.0" "sha2 0.11.1" "percent-encoding 2.3.2"; do
    name="${pin% *}"
    version="${pin#* }"
    if ! awk '/^\[dependencies\]/{f=1;next} /^\[/{f=0} f' Cargo.toml |
        grep -Eq "^${name}[[:space:]]*=[[:space:]]*(\"=$version\"|\{[^}]*version[[:space:]]*=[[:space:]]*\"=$version\")"; then
        echo "MISSING: Cargo.toml [dependencies] has no exact pin $name = \"=$version\""
        missing=1
    fi
    if ! grep -A1 "^name = \"$name\"\$" Cargo.lock | grep -q "^version = \"$version\"\$"; then
        echo "MISSING: Cargo.lock does not resolve $name to $version"
        missing=1
    fi
done
exit "$missing"
