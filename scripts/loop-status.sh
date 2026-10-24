#!/usr/bin/env bash
set -euo pipefail

# loop-status.sh - `make loop-status`: what needs the human, and the next
# command for each item. Reads the goal-state line that /drive-pr keeps in
# each PR body (see goal-pr-status.sh).
#
#   LOOP_READY_LABEL  the triage label for agent-ready issues

LOOP_READY_LABEL="${LOOP_READY_LABEL:-ready-for-agent}"

prs="$(gh pr list --state open --limit 100 --json number,title,headRefName,body --jq '
    .[] | select(.body | test("<!-- goal-state: ")) |
    [.number, (.body | capture("<!-- goal-state: (?<s>[^ ]+) -->").s),
     .headRefName, ((.body | capture("(?i)closes #(?<n>[0-9]+)").n) // "-"), .title] | @tsv')"

needs="" looping=""
while IFS=$'\t' read -r pr state branch issue title; do
    [ -n "$pr" ] || continue
    item="PR #$pr $title"
    case "$state" in
        ready) needs+="  $item: settled and ready. Review the diff and CI, then merge."$'\n' ;;
        stopped:budget) needs+="  $item: out of rounds. Read its status, then /drive-pr $pr <rounds>"$'\n' ;;
        stopped:no-progress) needs+="  $item: no progress. Comment the approach to take on the PR, then /drive-pr $pr"$'\n' ;;
        stopped:scope) needs+="  $item: needs a scope or criterion decision. Edit issue #$issue, then /groom-issue $issue --update"$'\n' ;;
        stopped:guard) needs+="  $item: goal files edited outside goal commits. Inspect the PR's commits, then /drive-pr $pr"$'\n' ;;
        stopped:*) needs+="  $item: stopped (${state#stopped:}). Read its status, then /drive-pr $pr"$'\n' ;;
        *) looping+="  $item: goal $state. Resume in .claude/worktrees/$branch with /drive-pr"$'\n' ;;
    esac
done <<< "$prs"

claimed="$(gh pr list --state open --limit 100 --json body --jq '.[].body' | grep -oiE 'closes #[0-9]+' | grep -oE '[0-9]+' | sort -u || true)"
groom=""
while IFS=$'\t' read -r number title; do
    [ -n "$number" ] || continue
    printf '%s\n' "$claimed" | grep -qx "$number" && continue
    groom+="  #$number $title: /groom-issue $number"$'\n'
done < <(gh issue list --state open --label "$LOOP_READY_LABEL" --limit 100 --json number,title --jq '.[] | [.number, .title] | @tsv')

cleanup=""
root="$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")"
while IFS=$'\t' read -r path branch; do
    [[ "$path" == "$root/.claude/worktrees/"* ]] || continue
    open="$(gh pr list --state open --head "$branch" --json number --jq length)"
    if [ "$open" = "0" ]; then
        cleanup+="  $branch has no open PR (merged, closed, or grooming unfinished): git worktree remove $path"$'\n'
    fi
done < <(git worktree list --porcelain | awk '/^worktree / { wt = substr($0, 10) } /^branch / { print wt "\t" substr($0, 19) }')

section() {
    echo "$1"
    if [ -n "$2" ]; then printf '%s' "$2"; else echo "  (none)"; fi
    echo
}
section "Needs you" "$needs"
section "In the loop" "$looping"
section "Ready to groom (label $LOOP_READY_LABEL)" "$groom"
section "Clean up" "$cleanup"
