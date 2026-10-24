#!/usr/bin/env bash
set -euo pipefail

# goal-selftest.sh - `make goal-selftest`: prove the loop's mechanics end to
# end in a throwaway worktree holding this checkout's current files
# (committed or not). Re-run it after editing anything in the loop (Makefile
# goal targets, goal tag wiring, scripts/goal-*.sh).
#
# Fixture in scripts/goal-selftest/:
#   dest   repo-relative path where the probe test goes
#   probe  a goal-tagged test that always fails
# A repo without tests (a Terraform envs repo) has no fixture; the probe is
# then a failing .goal/checks script instead.
#
#   GOAL_STATIC         the static gate target (default static-check)
#   GOAL_SELFTEST_LINK  untracked dirs to share from this checkout
#                       (default ".venv node_modules")

GOAL_STATIC="${GOAL_STATIC:-static-check}"
ROOT="$(git rev-parse --show-toplevel)"
FIXTURE="$ROOT/scripts/goal-selftest"
dest=""
[ -f "$FIXTURE/dest" ] && dest="$(<"$FIXTURE/dest")"
tmp="$(mktemp -d)"
wt="$tmp/selftest"
log="$tmp/out.log"

# shellcheck disable=SC2329  # invoked by the EXIT trap
cleanup() {
    git -C "$ROOT" worktree remove --force "$wt" >/dev/null 2>&1 || true
    rm -rf "$tmp"
}
trap cleanup EXIT

git worktree add -q --detach "$wt" HEAD
# Overlay uncommitted and untracked (not ignored) files, so a fresh scaffold
# can be tested before it is committed
(cd "$ROOT" && git ls-files -z --cached --others --exclude-standard |
    while IFS= read -r -d '' f; do [ -e "$f" ] && printf '%s\0' "$f"; done |
    tar --null -T - -cf -) | tar -xf - -C "$wt"
cd "$wt"
export GIT_AUTHOR_NAME=goal-selftest GIT_AUTHOR_EMAIL=goal-selftest@localhost
export GIT_COMMITTER_NAME=goal-selftest GIT_COMMITTER_EMAIL=goal-selftest@localhost
export GOAL_SETTLE_NO_VERIFY=1
git add -A
git diff --cached --quiet || commit_base=1
for dir in ${GOAL_SELFTEST_LINK:-.venv node_modules}; do
    if [ -e "$ROOT/$dir" ] && [ ! -e "$dir" ]; then
        ln -s "$ROOT/$dir" "$dir"
    fi
done

commit() { git commit -q --no-verify -m "$1"; }

[ -n "${commit_base:-}" ] && commit "selftest: working tree"

failures=0
# expect <pass|fail> <description> <command...>; output lands in $log
expect() {
    local want="$1" desc="$2" got
    shift 2
    if "$@" >"$log" 2>&1; then got=pass; else got=fail; fi
    if [ "$got" = "$want" ]; then
        echo "ok    $desc"
    else
        echo "WRONG $desc (expected $want, got $got): $*"
        sed 's/^/      /' "$log" | tail -20
        failures=$((failures + 1))
    fi
}
# expect_line <description> <regex>: the last command's output matches
expect_line() {
    if grep -qE "$2" "$log"; then
        echo "ok    $1"
    else
        echo "WRONG $1 (no line matching '$2')"
        failures=$((failures + 1))
    fi
}

echo "== no active goal"
expect pass "make goal is green" make -s goal
expect pass "make goal-settled passes" make -s goal-settled

mkdir -p .goal/checks
echo selftest > .goal/ISSUE
if [ -n "$dest" ]; then
    echo "== probe: a goal-tagged failing test"
    mkdir -p "$(dirname "$dest")"
    cp "$FIXTURE/probe" "$dest"
    echo "$dest" > .goal/locked
    git add "$dest"
    probe="$dest" gate=goal-tests
else
    echo "== probe: a failing .goal/checks script (no test fixture)"
    printf '#!/bin/sh\necho "goal selftest probe"\nexit 1\n' > .goal/checks/00-selftest.sh
    chmod +x .goal/checks/00-selftest.sh
    probe=.goal/checks/00-selftest.sh gate=.goal/checks/00-selftest.sh
fi
git add .goal
commit "goal: selftest probe"
expect pass "static gate ($GOAL_STATIC) stays green" make -s "$GOAL_STATIC"
if [ -n "$dest" ]; then
    expect fail "make goal-tests runs the probe and fails" make -s goal-tests
fi
expect fail "make goal is red" make -s goal
expect_line "red on $gate" "^GOAL RED:.*$gate"
expect fail "make goal-settled fails" make -s goal-settled

echo "== guard: a non-goal commit edits the probe"
echo >> "$probe"
git add "$probe"
commit "selftest: tamper"
expect fail "make goal is red" make -s goal
expect_line "red on guard" '^GOAL RED:.*guard'
git reset -q --hard HEAD~1

echo "== settle"
expect pass "make goal-settle untags, removes .goal/, commits" make -s goal-settle
expect pass "make goal-settled passes" make -s goal-settled
if [ -n "$dest" ]; then
    expect fail "make unit now runs the probe (it fails by design)" make -s unit
fi

echo
if [ "$failures" -eq 0 ]; then
    echo "goal-selftest: OK"
    exit 0
fi
echo "goal-selftest: $failures expectation(s) wrong"
exit 1
