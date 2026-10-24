#!/usr/bin/env bash
set -euo pipefail

# goal-settle.sh - `make goal-settle`: retire the goal scaffolding so the
# goal's tests join the permanent suite, then commit `goal: settle #<issue>`.
#
#   GOAL_SETTLE_MV   sed -E script mapping a goal-only file path to its
#                    permanent path (applied to each GOAL_TAG_PATHS match)
#   GOAL_SETTLE_SED  sed -E script that strips the goal tag (applied to each
#                    file matching GOAL_TAG_PATTERN). The goal tag sits on its
#                    own line, so deleting that line is the usual script.
#   GOAL_SETTLE_NO_VERIFY=1  skip the commit hook (goal-selftest only: its
#                    settled probe fails by design)

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source-path=SCRIPTDIR source=goal-lib.sh
source "$SCRIPT_DIR/goal-lib.sh"
GOAL_SETTLE_MV="${GOAL_SETTLE_MV:-}"
GOAL_SETTLE_SED="${GOAL_SETTLE_SED:-}"

if [ ! -f .goal/ISSUE ]; then
    echo "goal-settle: no active goal (.goal/ISSUE missing)" >&2
    exit 1
fi
issue="$(<.goal/ISSUE)"

while IFS= read -r path; do
    [ -n "$path" ] || continue
    new="$(printf '%s\n' "$path" | sed -E -e "$GOAL_SETTLE_MV")"
    if [ -z "$GOAL_SETTLE_MV" ] || [ "$new" = "$path" ]; then
        echo "goal-settle: GOAL_SETTLE_MV does not rename $path" >&2
        exit 1
    fi
    mkdir -p "$(dirname "$new")"
    git mv "$path" "$new"
    echo "goal-settle: moved $path -> $new"
done < <(goal_only_files)

while IFS= read -r path; do
    [ -n "$path" ] || continue
    if [ -z "$GOAL_SETTLE_SED" ]; then
        echo "goal-settle: $path carries the goal tag but GOAL_SETTLE_SED is empty" >&2
        exit 1
    fi
    tmp="$(mktemp)"
    sed -E -e "$GOAL_SETTLE_SED" "$path" > "$tmp"
    cat "$tmp" > "$path"
    rm -f "$tmp"
    git add "$path"
    echo "goal-settle: untagged $path"
done < <(goal_tagged_files)

git rm -rq --ignore-unmatch .goal
rm -rf .goal

"$SCRIPT_DIR/goal-settled.sh"
git commit -q ${GOAL_SETTLE_NO_VERIFY:+--no-verify} -m "goal: settle #$issue"
echo "goal-settle: committed 'goal: settle #$issue'"
