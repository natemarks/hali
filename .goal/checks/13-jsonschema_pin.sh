#!/usr/bin/env bash
set -uo pipefail
# Criterion 13 (#6): `jsonschema` is an exact-pinned (=0.60.1) dev-dependency with default
# features off, and Cargo.lock resolves it to 0.60.1.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

fail=0
miss() {
    echo "MISSING: $1"
    fail=1
}

dev=$(awk '/^\[/ {on = ($0 == "[dev-dependencies]")} on' Cargo.toml)
line=$(grep -E '^jsonschema[[:space:]]*=' <<<"$dev" || true)

if [ -z "$line" ]; then
    miss "Cargo.toml [dev-dependencies] has no jsonschema entry"
else
    grep -q '=0\.60\.1' <<<"$line" || miss "jsonschema must be pinned with version = \"=0.60.1\" (got: $line)"
    grep -Eq 'default-features[[:space:]]*=[[:space:]]*false' <<<"$line" || miss "jsonschema must set default-features = false (got: $line)"
fi
if grep -Eq '^\[dependencies\]' Cargo.toml && awk '/^\[/ {on = ($0 == "[dependencies]")} on' Cargo.toml | grep -Eq '^jsonschema'; then
    miss "jsonschema must be a dev-dependency only"
fi
grep -A1 '^name = "jsonschema"$' Cargo.lock | grep -q '^version = "0\.60\.1"$' || miss "Cargo.lock does not resolve jsonschema to 0.60.1"

exit "$fail"
