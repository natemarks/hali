#!/usr/bin/env bash
# Temporary check for #8: the goal tests and the shared helper are lint- and dead-code-clean
# with the goal feature on, so settling them cannot break the commit hook.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

if ! out=$(cargo clippy --all-targets --features test-hooks,goal -- -D warnings 2>&1); then
    echo "lint-goal: clippy fails with the goal feature on (goal tests do not build or are not clean)"
    echo "$out" | tail -n 15 >&2
    exit 1
fi
if ! out=$(RUSTFLAGS="-D dead_code" cargo check --all-targets --features test-hooks,goal 2>&1); then
    echo "lint-goal: dead code with the goal feature on"
    echo "$out" | tail -n 15 >&2
    exit 1
fi
echo "lint-goal: clippy and dead-code clean with the goal feature"
