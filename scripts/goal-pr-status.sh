#!/usr/bin/env bash
set -euo pipefail

# goal-pr-status.sh - replace the loop status block in a PR body with stdin,
# leaving the rest of the body (and any human edits to it) untouched.
#
# Usage: scripts/goal-pr-status.sh <pr> < status.md
#
# The write goes through goal-pr-body.sh (gh pr edit fails on older gh).
#
# The block sits between <!-- goal-status:start --> and <!-- goal-status:end -->
# and is appended when the body has none. Its first line should be the
# machine-readable state that `make loop-status` reads:
#   <!-- goal-state: red|green|stopped:<reason>|ready -->

if [ $# -ne 1 ]; then
    echo "Usage: $0 <pr> < status.md" >&2
    exit 1
fi
pr="$1"
start="<!-- goal-status:start -->"
end="<!-- goal-status:end -->"

block="$(cat)"
body="$(gh pr view "$pr" --json body -q .body)"

if printf '%s\n' "$body" | grep -qF "$start"; then
    # The block travels through ENVIRON: awk -v would rewrite its backslashes
    new="$(printf '%s\n' "$body" | BLOCK="$block" awk -v start="$start" -v end="$end" '
        $0 == start { print start; print ENVIRON["BLOCK"]; skip = 1; next }
        $0 == end   { print end; skip = 0; next }
        !skip       { print }')"
else
    new="$(printf '%s\n\n%s\n%s\n%s\n' "$body" "$start" "$block" "$end")"
fi

printf '%s' "$new" | "$(dirname "${BASH_SOURCE[0]}")/goal-pr-body.sh" "$pr" >/dev/null
echo "goal-pr-status: updated PR #$pr"
