---
name: drive-pr
description: Loop agents on a groomed PR until make goal is green, review it, settle the goal, and hand the PR back for human merge.
argument-hint: [pr-number] [rounds]
disable-model-invocation: true
---

# Drive PR

Arguments: `$ARGUMENTS`: an optional PR number, then optional **rounds** (default 6). Without a PR number, use the current branch's PR (`gh pr view --json number`); with neither, ask.

You are the orchestrator. Agents do the work; you hold the evidence: run `make goal` yourself after every round and trust its output over any agent's report. One implementer owns the change at a time. The PR is yours to drive, and merging it belongs to the human. The PR's **status block** is the human's dashboard and your memory: update it every round. Its format, and the loop's layout, are in the "AI loop" section of `CLAUDE.md`.

## 0. Resume

```bash
gh pr view <pr> --json number,headRefName,baseRefName,isDraft,body,url
```

Run `scripts/goal-worktree.sh <headRefName>`; when this session is elsewhere, switch into the printed path with `EnterWorktree` (`path`). Then `git pull --ff-only`.

Read `.goal/ISSUE`, `.goal/plan.md` and the status block. With no `.goal/`, the PR is not groomed: stop and point the human at `/groom-issue`. A round log in the status block means a resumed run: carry on from its last row, reading any PR comments posted since, which may hold the human's decision. Rounds count from this invocation.

## 1. Round

1. Run `make goal 2>&1 | tee .goal/last-run.log`; its last line reads `GOAL GREEN` or `GOAL RED: <gates>`. **Green** → go to Review. **Red** → note the failing checks.
2. **Guard** in the failing gates: a non-`goal:` commit changed goal-owned files. Restore them from the last `goal:` commit (`git checkout <sha> -- <paths>`), commit as `goal: restore <paths> changed by <bad-sha>`, and tell the next implementer what happened. A second guard failure in this run is a **guard** stop.
3. **No progress**: the same failing checks as after the previous implementer round. The first time, dispatch the **planner** to re-plan, write its approach into `.goal/plan.md`, commit as `goal: replan #<issue>`, and run the next implementer on `opus`. The second time is a **no-progress** stop.
4. Dispatch the **implementer** with the issue and PR numbers and this round's focus (the failing checks, any restore or new approach). Use `opus` when the plan calls the change cross-cutting; otherwise its default.
5. Route the report:
   - **Faulty check**: weigh its evidence against the issue. When it holds, dispatch the **test-author** in update mode with it. When fixing it would change what an acceptance criterion means, that is a **scope** stop.
   - **Scope question**: a **scope** stop.
6. Push, then update the status block (`scripts/goal-pr-status.sh <pr>`): state `red` or `green`, each criterion's ✅/❌, and a round-log row.
7. Out of rounds → a **budget** stop. Otherwise, next round.

## 2. Review

`make goal` is green. Dispatch in parallel, each with the issue number and the base ref:

- the **reviewer**, on `opus` when the change is cross-cutting or high-impact;
- the **security-reviewer**, when the plan's security call is sensitive, the diff touches a security-sensitive path from `CLAUDE.md`, or the issue is labelled `security`.

Route each finding:

- **blocking, with a failing check that would catch it**: batch them to the **test-author** in update mode. The goal turns red on them, and the loop resumes at Round.
- **blocking, without one**: batch them as the next implementer round's focus, then review again.
- **advisory** and **follow-up**: collect for the hand-back.

Each review pass counts as a round and gets a round-log row. Done when a pass returns no blocking findings.

## 3. Settle

Run `make goal-settle`: it removes the goal tags, deletes `.goal/`, and commits `goal: settle #<issue>`. Then confirm the static gate passes (the goal's tests now run in `make unit`) and push.

## 4. Hand back

Rewrite the PR body (`gh pr edit <pr> --body-file -`):

- what changed and why, with `Closes #<issue>`;
- the goal map, with each criterion mapped to the permanent test or scenario that now proves it;
- the commands you ran and their results;
- review outcome: blocking findings fixed, advisory findings left, and the security-reviewer's "needs qualified human review" list;
- risks, migration and rollout notes;
- follow-up issue drafts (title and one line each).

Then set the status block to state `ready` and run `gh pr ready <pr>`; CI now runs `goal-settled`. Report the PR URL, the rounds used, and the follow-up drafts. The human reviews the diff and CI, creates the follow-ups, merges, and removes the worktree (`make loop-status` lists it).

## Stop

A stop hands control to the human. Push, set the status block to state `stopped:<reason>` with the decision needed and the resume command, and report the same. The PR stays a draft.

| Stop | The human | Resume with |
|---|---|---|
| `budget` | reads the status block | `/drive-pr <pr> <rounds>` |
| `no-progress` | comments the approach to take on the PR | `/drive-pr <pr>` |
| `scope` | edits the issue (criteria, scope) | `/groom-issue <issue> --update`, then `/drive-pr <pr>` |
| `guard` | inspects the PR's commits | `/drive-pr <pr>` |
