#!/usr/bin/env bash
set -euo pipefail

# goal-worktree.sh - print the worktree path for a PR branch, creating it if
# needed. One worktree per PR lets several loops run while the human grooms
# the next issue.
#
# Usage: scripts/goal-worktree.sh <branch>
#
# Worktrees live in <repo>/.claude/worktrees/<branch>, where Claude Code's
# EnterWorktree can switch a session into them. The branch is checked out
# from (in order): an existing worktree, a local branch, origin/<branch>, or
# a new branch from the default branch.

if [ $# -ne 1 ]; then
    echo "Usage: $0 <branch>" >&2
    exit 1
fi
branch="$1"

common="$(git rev-parse --path-format=absolute --git-common-dir)"
root="$(dirname "$common")"
path="$root/.claude/worktrees/$branch"

existing="$(git worktree list --porcelain | awk -v ref="refs/heads/$branch" '
    /^worktree / { wt = substr($0, 10) }
    $0 == "branch " ref { print wt; exit }')"
if [ -n "$existing" ]; then
    echo "$existing"
    exit 0
fi

git fetch -q origin
mkdir -p "$(dirname "$path")"
if git show-ref --verify --quiet "refs/heads/$branch"; then
    git worktree add -q "$path" "$branch"
elif git show-ref --verify --quiet "refs/remotes/origin/$branch"; then
    git worktree add -q --track -b "$branch" "$path" "origin/$branch"
else
    default="$(git symbolic-ref --quiet --short refs/remotes/origin/HEAD || echo origin/main)"
    git worktree add -q --no-track -b "$branch" "$path" "$default"
fi
echo "$path"
