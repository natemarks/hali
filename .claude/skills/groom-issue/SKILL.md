---
name: groom-issue
description: Turn one GitHub issue into a draft PR, in its own worktree, whose goal is defined and red, ready for /drive-pr.
argument-hint: <issue-number> [--update]
disable-model-invocation: true
---

# Groom issue

Arguments: `$ARGUMENTS`: the issue number, then `--update` to re-groom an issue whose PR already exists (after a scope stop, or when the issue changed). If the number is absent, ask.

Grooming turns an issue into an executable **contract**: a draft PR whose `make goal` is red, failing in one place per acceptance criterion. The human owns the contract's terms (goal, acceptance criteria, scope, acceptable risk); you find gaps and draft, and the human decides, in **one approval**. The loop's layout and the PR status block format are in the "AI loop" section of `CLAUDE.md`.

## 1. Read

`gh issue view <n> --comments --json title,body,labels,comments`. An issue labelled for triage (`needs-triage`, `needs-info`, or their strings in `docs/agents/triage-labels.md`) is not ready: stop and point the human at `/triage <n>`. A triage agent brief in the comments, or a `to-tickets` body, is input for the planner.

## 2. Worktree

Work in the PR's own worktree, so other loops and the human's checkout stay untouched:

- **New**: branch `<n>-<slug>` (slug: the title, kebab-case, at most five words).
- **`--update`**: the branch of the open PR whose body says `Closes #<n>` (`gh pr list --state open --search "<n> in:body" --json number,headRefName,body`).

Run `scripts/goal-worktree.sh <branch>` and switch this session into the path it prints with `EnterWorktree` (`path`). Everything after this happens there.

## 3. Plan

Dispatch the **planner** agent with the issue number (and the PR number on `--update`, plus the human's decision that triggered it).

## 4. Approve

Present one screen: the contract gaps with their proposed wording (skip the heading when there are none), the goal map, the approach, the security call, and the open questions. The human approves, edits, or answers each item in one reply; iterate until they approve the whole screen.

With approval, write any accepted contract wording back to the issue (`gh issue edit <n> --body-file -`) so the issue stays the single source of truth, and write the approved plan to `.goal/plan.md`.

Done when the human approves the goal map, and every acceptance criterion on it can be checked.

## 5. Define the goal

New: `mkdir -p .goal/checks && echo <n> > .goal/ISSUE`, then dispatch the **test-author** in define mode. On `--update`, dispatch it in update mode with the changed rows.

## 6. Verify red

Run the checks yourself; the test-author's report is a claim, and this is the evidence.

- The static gate passes, so the commit hook stays green.
- `make goal 2>&1 | tee .goal/last-run.log` ends `GOAL RED`, failing each goal map check with that check's expected failure.
- `make goal-settled` fails.

A check that passes, or fails for another reason, goes back to the test-author in update mode with the output. When a check passes because the behaviour already exists, take that to the human: the issue may already be done.

Done when every acceptance criterion maps to at least one check failing with its expected failure, and the static gate is green.

## 7. Publish

New:

```bash
git push -u origin HEAD
gh pr create --draft --title "<issue title>" --body-file - <<'EOF'
<one-line summary>

Closes #<n>
EOF
```

Then write the status block with state `red`, round 0 (`scripts/goal-pr-status.sh <pr>`). On `--update`: `git push`, then the same status block call, noting "re-groomed" in the round log.

Report the PR URL, the goal map, and how to start the loop: `/drive-pr` here (this session is already in the worktree), or `/drive-pr <pr>` from any session in this repo, which switches into the worktree.
