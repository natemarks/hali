#!/usr/bin/env bash
set -euo pipefail

# goal-settled.sh - `make goal-settled`: fail while goal scaffolding remains.
#
# A PR is settled when .goal/ is gone and no test still carries the goal tag,
# so every goal test now runs in `make unit`. CI runs this on ready PRs; make
# it a required check (`make loop-protect`) so an unsettled PR cannot merge.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source-path=SCRIPTDIR source=goal-lib.sh
source "$SCRIPT_DIR/goal-lib.sh"
unsettled=0

if [ -n "$(git ls-files .goal)" ] || [ -d .goal ]; then
    echo "goal-settled: .goal/ still exists"
    unsettled=1
fi

tagged="$(goal_tagged_files)"
if [ -n "$tagged" ]; then
    echo "$tagged"
    echo "goal-settled: goal tags remain in the files above"
    unsettled=1
fi

only="$(goal_only_files)"
if [ -n "$only" ]; then
    echo "$only"
    echo "goal-settled: goal-only test files remain (above)"
    unsettled=1
fi

if [ "$unsettled" -ne 0 ]; then
    echo "goal-settled: run 'make goal-settle' once 'make goal' is green" >&2
    exit 1
fi
echo "goal-settled: OK"
