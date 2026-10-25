#!/usr/bin/env bash
# Goal #3, criterion 5: HALI_TEST_NOW / HALI_TEST_ID_SEED exist, are read only by cli,
# and are compiled in only under the `test-hooks` feature.
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

fail() {
    echo "test-hooks-gated: $*" >&2
    exit 1
}

for var in HALI_TEST_NOW HALI_TEST_ID_SEED; do
    grep -rq "$var" src || fail "$var is not implemented in src/"
done

# Only cli may read the hooks.
if grep -rlE 'HALI_TEST_' src | grep -v '^src/cli/' | grep -q .; then
    fail "HALI_TEST_* is referenced outside src/cli/ (core must stay environment-free)"
fi

# Every mention sits in a file that gates it behind the feature.
while IFS= read -r file; do
    grep -q 'feature = "test-hooks"' "$file" ||
        fail "$file mentions HALI_TEST_* without cfg(feature = \"test-hooks\")"
done < <(grep -rlE 'HALI_TEST_' src)

# Behavioural: a default build must not contain the hook names; a test-hooks build must.
cargo build -q 2>&1 || fail "cargo build failed"
if grep -aq 'HALI_TEST_' target/debug/hali; then
    fail "default build contains HALI_TEST_* (not gated)"
fi
cargo build -q --features test-hooks 2>&1 || fail "cargo build --features test-hooks failed"
grep -aq 'HALI_TEST_ID_SEED' target/debug/hali ||
    fail "test-hooks build lacks HALI_TEST_ID_SEED"
grep -aq 'HALI_TEST_NOW' target/debug/hali || fail "test-hooks build lacks HALI_TEST_NOW"
# Leave the default build in place.
cargo build -q 2>&1 || true
echo "test-hooks-gated: ok"
