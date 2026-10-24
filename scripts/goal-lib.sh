# shellcheck shell=bash
# goal-lib.sh - helpers sourced by goal-settle.sh and goal-settled.sh.
#
#   GOAL_TAG_PATTERN  extended regex matching the goal tag inside test files
#   GOAL_TAG_PATHS    space-separated git pathspecs of goal-only test files

# Files that legitimately name the goal tag: the loop's own tooling and docs
GOAL_EXCLUDE=(':!scripts/goal*' ':!scripts/loop-*' ':!Makefile' ':!CLAUDE.md' ':!AGENTS.md' ':!README.md' ':!.claude/' ':!.github/' ':!docs/')

# Tracked files that carry the goal tag
goal_tagged_files() {
    [ -n "${GOAL_TAG_PATTERN:-}" ] || return 0
    git grep -lE "$GOAL_TAG_PATTERN" -- . "${GOAL_EXCLUDE[@]}" || true
}

# Tracked files that are goal-only by path
goal_only_files() {
    [ -n "${GOAL_TAG_PATHS:-}" ] || return 0
    local specs
    read -ra specs <<< "$GOAL_TAG_PATHS"
    git ls-files -- "${specs[@]}"
}
