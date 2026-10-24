#!/usr/bin/env bash
set -euo pipefail

# loop-protect.sh - `make loop-protect`: make the loop's checks required on
# the default branch, so only a green, settled PR can merge.
#
# Usage: scripts/loop-protect.sh [--apply]
#
# Without --apply it prints what it would change and the exact command. With
# --apply it runs that command. It adds checks and never removes any.
#
#   LOOP_REQUIRED_CHECKS  status checks to require (default "static-check goal-settled")

LOOP_REQUIRED_CHECKS="${LOOP_REQUIRED_CHECKS:-static-check goal-settled}"
apply=0
[ "${1:-}" = "--apply" ] && apply=1

repo="$(gh repo view --json nameWithOwner -q .nameWithOwner)"
default="$(gh repo view --json defaultBranchRef -q .defaultBranchRef.name)"
read -ra checks <<< "$LOOP_REQUIRED_CHECKS"

if [ "$(gh api "repos/$repo/rulesets" --jq length 2>/dev/null || echo 0)" -gt 0 ]; then
    echo "loop-protect: $repo uses rulesets. Add these required status checks to the"
    echo "ruleset that targets $default (Settings > Rules > Rulesets): ${checks[*]}"
    exit 0
fi

base="repos/$repo/branches/$default/protection"
if gh api "$base/required_status_checks" >/dev/null 2>&1; then
    plan="add required checks [${checks[*]}] to existing protection on $default"
    cmd=(gh api -X POST "$base/required_status_checks/contexts" --silent)
    for check in "${checks[@]}"; do cmd+=(-f "contexts[]=$check"); done
    run() { "${cmd[@]}"; }
elif gh api "$base" >/dev/null 2>&1; then
    echo "loop-protect: $default is protected without required status checks; add"
    echo "${checks[*]} in Settings > Branches > $default, then re-run make loop-doctor."
    exit 0
else
    plan="protect $default: changes arrive by PR (0 approvals; raise it for a team repo), checks [${checks[*]}] required"
    contexts="$(printf '"%s",' "${checks[@]}")"
    payload="{\"required_status_checks\":{\"strict\":false,\"contexts\":[${contexts%,}]},\"enforce_admins\":false,\"required_pull_request_reviews\":{\"required_approving_review_count\":0},\"restrictions\":null}"
    cmd=(gh api -X PUT "$base" --silent --input -)
    run() { printf '%s' "$payload" | "${cmd[@]}"; }
fi

echo "loop-protect: will $plan"
echo "  ${cmd[*]}"
[ -n "${payload:-}" ] && echo "  payload: $payload"
if [ "$apply" -eq 1 ]; then
    run
    echo "loop-protect: done; check with make loop-doctor"
else
    echo "loop-protect: dry run; apply with: make loop-protect APPLY=1"
fi
