#!/usr/bin/env bash
set -uo pipefail
# #4 row 12: `make install` builds the release binary with `--locked` and no cargo features, and
# aborts when the binary contains HALI_TEST_. The installed (release) binary has no HALI_TEST_.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

if ! plan=$(make -n install 2>&1); then
    echo "MISSING: \`make -n install\` fails:"
    printf '%s\n' "$plan" | tail -5
    exit 1
fi
build=$(printf '%s\n' "$plan" | grep -E 'cargo build' || true)
case "$build" in
    *--release*) ;;
    *) echo "MISSING: make install has no \`cargo build --release\`"; exit 1 ;;
esac
case "$build" in
    *--locked*) ;;
    *) echo "MISSING: the release build in make install lacks --locked"; exit 1 ;;
esac
case "$build" in
    *--features*|*--all-features*|*test-hooks*)
        echo "FOUND: the release build in make install enables features: $build"
        exit 1 ;;
esac
if ! printf '%s\n' "$plan" | grep -q 'HALI_TEST_'; then
    echo "MISSING: make install does not check the binary for HALI_TEST_"
    exit 1
fi
cargo build --release --locked --quiet || {
    echo "MISSING: cargo build --release --locked failed"
    exit 1
}
bin="${CARGO_TARGET_DIR:-target}/release/hali"
if grep -aq 'HALI_TEST_' "$bin"; then
    echo "FOUND: the release binary contains HALI_TEST_"
    exit 1
fi
exit 0
