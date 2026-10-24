---
name: loop-status
description: Show what in the AI loop needs you (stopped loops, PRs ready to merge, issues ready to groom, worktrees to clean up) and the next command for each.
disable-model-invocation: true
---

# Loop status

Run `make loop-status` and show its output as it is: it lists, with a next command for each item,

- **Needs you**: PRs settled and ready to merge, and stopped loops with their stop reason;
- **In the loop**: PRs whose goal is still red or green, with where to resume them;
- **Ready to groom**: agent-ready issues no open PR closes yet;
- **Clean up**: loop worktrees whose branch has no open PR.

Then recommend the one item to take first, with its command. Merges come before stopped loops, since a merge can unblock other work. Stopped loops come before new grooming. Among issues to groom, take one whose blockers ("Blocked by") are all closed.

When the human picks an item, open it: for a stopped loop, show the PR's status block and the decision it needs; for a ready PR, show its body and `gh pr checks <pr>`.
