#!/usr/bin/env bash
set -uo pipefail

# loop-doctor.sh - `make loop-doctor`: check everything the AI loop depends
# on and print the fix for each miss. Run it after setup, and whenever the
# loop misbehaves.
#
#   GOAL_STATIC           the static gate target (default static-check)
#   LOOP_REQUIRED_CHECKS  status checks the default branch must require
#   LOOP_READY_LABEL      the triage label for agent-ready issues

GOAL_STATIC="${GOAL_STATIC:-static-check}"
LOOP_REQUIRED_CHECKS="${LOOP_REQUIRED_CHECKS:-static-check goal-settled}"
LOOP_READY_LABEL="${LOOP_READY_LABEL:-ready-for-agent}"
fails=0
warns=0

pass() { echo "PASS  $1"; }
warn() { echo "WARN  $1"; echo "      fix: $2"; warns=$((warns + 1)); }
fail() { echo "FAIL  $1"; echo "      fix: $2"; fails=$((fails + 1)); }

echo "== GitHub"
if gh auth status >/dev/null 2>&1; then pass "gh is authenticated"; else fail "gh is not authenticated" "gh auth login"; fi
repo="$(gh repo view --json nameWithOwner -q .nameWithOwner 2>/dev/null || true)"
default="$(gh repo view --json defaultBranchRef -q .defaultBranchRef.name 2>/dev/null || true)"
if [ -n "$repo" ]; then pass "origin is GitHub repo $repo (default branch $default)"; else fail "origin is not a GitHub repo gh can see" "git remote add origin <github-url>"; fi
if grep -qi github docs/agents/issue-tracker.md 2>/dev/null; then
    pass "issue tracker is GitHub (docs/agents/issue-tracker.md)"
else
    fail "docs/agents/issue-tracker.md missing or not GitHub" "run /setup-matt-pocock-skills"
fi
if [ -n "$repo" ]; then
    if gh label list --repo "$repo" --search "$LOOP_READY_LABEL" --json name -q '.[].name' 2>/dev/null | grep -qx "$LOOP_READY_LABEL"; then
        pass "label '$LOOP_READY_LABEL' exists"
    else
        warn "label '$LOOP_READY_LABEL' missing, so /loop-status finds no issues to groom" "gh label create $LOOP_READY_LABEL"
    fi
fi

echo "== Merge gate"
if [ -n "$repo" ] && [ -n "$default" ]; then
    required="$( {
        gh api "repos/$repo/branches/$default/protection/required_status_checks" --jq '.contexts[]' 2>/dev/null
        gh api "repos/$repo/rules/branches/$default" --jq '.[] | select(.type == "required_status_checks") | .parameters.required_status_checks[].context' 2>/dev/null
    } | sort -u)"
    for check in $LOOP_REQUIRED_CHECKS; do
        if printf '%s\n' "$required" | grep -qx "$check"; then
            pass "'$check' is a required check on $default"
        else
            fail "'$check' is not a required check on $default, so an unsettled or failing PR can merge" "make loop-protect"
        fi
    done
fi

echo "== Project"
hook="$(git rev-parse --path-format=absolute --git-common-dir)/hooks/pre-commit"
if [ -x "$hook" ]; then pass "commit hook installed"; else fail "commit hook not installed" "make hooks (or: pre-commit install)"; fi
for target in goal goal-tests goal-settled goal-settle goal-selftest unit "$GOAL_STATIC"; do
    if make -n "$target" >/dev/null 2>&1; then pass "make $target"; else fail "make $target is not defined" "re-run /scaffold-ai-loop"; fi
done
for script in goal goal-guard goal-settled goal-settle goal-selftest goal-worktree goal-pr-status loop-status; do
    if [ -x "scripts/$script.sh" ]; then pass "scripts/$script.sh"; else fail "scripts/$script.sh missing or not executable" "re-run /scaffold-ai-loop"; fi
done
missing=""
for agent in planner test-author implementer reviewer security-reviewer; do
    [ -f ".claude/agents/$agent.md" ] || missing+=" agent:$agent"
done
for skill in groom-issue drive-pr loop-status; do
    [ -f ".claude/skills/$skill/SKILL.md" ] || missing+=" skill:/$skill"
done
if [ -z "$missing" ]; then pass "five agents and three loop skills"; else fail "missing:$missing" "re-run /scaffold-ai-loop"; fi
if grep -q 'gh pr merge' .claude/settings.json 2>/dev/null; then
    pass ".claude/settings.json pre-approves loop commands and denies merge"
else
    fail ".claude/settings.json lacks the loop permissions" "re-run /scaffold-ai-loop"
fi
for probe in .goal/x.log .claude/worktrees/x; do
    if git check-ignore -q "$probe"; then pass "$probe is gitignored"; else fail "$probe is not gitignored" "add $(dirname "$probe")/ to .gitignore"; fi
done

echo "== Models"
settings=("$HOME/.claude/settings.json" .claude/settings.json .claude/settings.local.json)
setting() {
    local key="$1" val="${!1:-}"
    if [ -z "$val" ]; then
        val="$(grep -ho "\"$key\"[[:space:]]*:[[:space:]]*\"[^\"]*\"" "${settings[@]}" 2>/dev/null | tail -1 | sed -E 's/.*:[[:space:]]*"([^"]*)"/\1/')"
    fi
    printf '%s' "$val"
}
if [ "$(setting CLAUDE_CODE_USE_BEDROCK)" = "1" ]; then
    for alias in OPUS SONNET HAIKU; do
        key="ANTHROPIC_DEFAULT_${alias}_MODEL"
        val="$(setting "$key")"
        if [ -n "$val" ]; then
            pass "$key=$val (confirm it is enabled in your Bedrock account and region)"
        else
            fail "$key is unset, so the agents' model alias falls back to an older model" "add \"$key\": \"us.anthropic.claude-$(echo "$alias" | tr '[:upper:]' '[:lower:]')-5-5\" to env in ~/.claude/settings.json"
        fi
    done
else
    pass "not on Bedrock: model aliases resolve directly"
fi

echo
echo "loop-doctor: $fails failed, $warns warnings"
[ "$fails" -eq 0 ]
