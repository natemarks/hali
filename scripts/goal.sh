#!/usr/bin/env bash
set -uo pipefail

# goal.sh - `make goal`: green only when the active PR's goal is met.
#
# Runs every gate and reports each one, then exits non-zero if any failed:
#   1. the static gate (GOAL_STATIC, default static-check)
#   2. goal-guard.sh: only `goal:` commits touched goal-owned files
#   3. with an active goal (.goal/ exists): goal-tagged tests and scenarios
#      (`make goal-tests`) and every executable .goal/checks/*.sh
# Without .goal/ there is no active goal and only 1-2 run.

GOAL_STATIC="${GOAL_STATIC:-static-check}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
failed=()

gate() {
    local name="$1"
    shift
    echo "=== goal: $name"
    if "$@"; then
        echo "=== PASS $name"
    else
        echo "=== FAIL $name"
        failed+=("$name")
    fi
}

gate "$GOAL_STATIC" make --no-print-directory "$GOAL_STATIC"
gate guard "$SCRIPT_DIR/goal-guard.sh"

if [ -d .goal ]; then
    gate goal-tests make --no-print-directory goal-tests
    for check in .goal/checks/*.sh; do
        [ -e "$check" ] || continue
        gate "$check" "$check"
    done
else
    echo "=== goal: no active goal (.goal/ absent)"
fi

echo
if [ ${#failed[@]} -eq 0 ]; then
    echo "GOAL GREEN"
    exit 0
fi
echo "GOAL RED: ${failed[*]}"
exit 1
