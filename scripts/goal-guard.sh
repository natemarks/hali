#!/usr/bin/env bash
set -euo pipefail

# goal-guard.sh - fail if a non-`goal:` commit on this branch changed a
# goal-owned file.
#
# Goal-owned: everything under .goal/, plus each path listed in .goal/locked
# (one per line; a trailing / locks a directory).
# Only the test-author's `goal: ...` commits may change them, so the
# implementer meets the goal instead of moving it.

BASE_REF="${GOAL_BASE_REF:-}"
if [ -z "$BASE_REF" ]; then
    BASE_REF="$(git symbolic-ref --quiet --short refs/remotes/origin/HEAD 2>/dev/null || echo origin/main)"
fi

if ! base="$(git merge-base HEAD "$BASE_REF" 2>/dev/null)"; then
    echo "goal-guard: cannot find merge-base with $BASE_REF; set GOAL_BASE_REF" >&2
    exit 1
fi

locked=(".goal/")
if [ -f .goal/locked ]; then
    while IFS= read -r line; do
        [ -n "$line" ] && locked+=("$line")
    done < .goal/locked
fi

is_locked() {
    local path="$1" lock
    for lock in "${locked[@]}"; do
        case "$lock" in
            */) [[ "$path" == "$lock"* ]] && return 0 ;;
            *) [ "$path" = "$lock" ] && return 0 ;;
        esac
    done
    return 1
}

violations=0
while IFS=$'\t' read -r sha subject; do
    [ -n "$sha" ] || continue
    [[ "$subject" == goal:* ]] && continue
    while IFS= read -r path; do
        if is_locked "$path"; then
            echo "goal-guard: ${sha:0:9} \"$subject\" changed goal-owned $path"
            violations=1
        fi
    done < <(git diff-tree --no-commit-id --name-only -r "$sha")
done < <(git log --no-merges --format='%H%x09%s' "$base..HEAD")

if [ "$violations" -ne 0 ]; then
    echo "goal-guard: only 'goal: ...' commits (test-author) may change goal-owned files" >&2
    exit 1
fi
echo "goal-guard: OK"
