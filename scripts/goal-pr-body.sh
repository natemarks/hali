#!/usr/bin/env bash
set -euo pipefail

# goal-pr-body.sh - replace a PR's whole body with stdin.
#
# Usage: scripts/goal-pr-body.sh <pr> < body.md
#
# Uses the REST API instead of `gh pr edit`, which fails on older gh (seen on
# 2.46.0) with "Projects (classic) is being deprecated": it still queries the
# retired projectCards field. Every PR body write in the loop goes through
# here, so the loop does not depend on the installed gh version.

if [ $# -ne 1 ]; then
    echo "Usage: $0 <pr> < body.md" >&2
    exit 1
fi
pr="$1"

gh api -X PATCH "repos/{owner}/{repo}/pulls/$pr" -F body=@- --silent
echo "goal-pr-body: updated PR #$pr"
