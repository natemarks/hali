#!/usr/bin/env bash
set -uo pipefail
# #4 row 1: the generated man page is clean roff: `groff -man -ww -z` prints no warning.
# Skipped (exit 0, with a message) when groff is not installed: the goal does not fail for a
# missing tool, but this row is then unchecked here.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

if ! command -v groff >/dev/null 2>&1; then
    echo "SKIPPED: groff is not installed; install groff to check the man page (row 1)"
    exit 0
fi
cargo build --quiet --locked || {
    echo "MISSING: cargo build failed"
    exit 1
}
bin="${CARGO_TARGET_DIR:-target}/debug/hali"
page=$(mktemp) || exit 1
trap 'rm -f "$page"' EXIT
if ! env -i "$bin" generate man >"$page" 2>/dev/null || [ ! -s "$page" ]; then
    echo "MISSING: \`hali generate man\` does not print a man page"
    exit 1
fi
if ! warnings=$(groff -man -ww -z <"$page" 2>&1); then
    echo "FAILED: groff rejects the man page:"
    printf '%s\n' "$warnings"
    exit 1
fi
if [ -n "$warnings" ]; then
    echo "WARNINGS: groff -man -ww -z reports:"
    printf '%s\n' "$warnings"
    exit 1
fi
exit 0
