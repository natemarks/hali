#!/usr/bin/env bash
set -uo pipefail
# Criteria 1 and 2 (#117): on a pseudo-terminal a usage error leaves no live escape sequence. Runs the
# ignored `pty` tests of tests/cli_usage_text.rs, which need script(1) from util-linux.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

if [ ! -x /usr/bin/script ]; then
    echo "PASS (skipped): /usr/bin/script is missing, so the pty tests cannot run here; CI's test-external job runs them"
    exit 0
fi

if ! cargo test --features test-hooks,goal --test cli_usage_text -- --ignored; then
    echo "FAILED: the pty tests of tests/cli_usage_text.rs (run with --ignored) do not pass"
    exit 1
fi
