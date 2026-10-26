#!/usr/bin/env bash
# Goal #7: the goal's own (goal-tagged) tests must be lint-clean, or settling breaks the hook.
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

if ! out=$(cargo clippy --all-targets --features test-hooks,goal -- -D warnings 2>&1); then
    echo "$out" | tail -40 >&2
    echo "clippy-goal: clippy fails with the goal feature on; fix the goal tests' lints" >&2
    exit 1
fi
echo "clippy-goal: ok"
