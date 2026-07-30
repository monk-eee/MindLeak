- The fleet can now reclaim its own disk. `scripts/worktree-reclaim.mjs` reports
  worktrees whose commits have landed on `origin/main` and, when told to,
  removes them along with their local branch, their merged remote branch, and
  their build output. `make reclaim` reports; `make reclaim ARGS="--reclaim
  --remote"` acts.
  This exists because cleanup never happens on goodwill. The agent that created
  a worktree has finished and moved on by the time it is safe to remove, so the
  mess is always somebody else's and it grows every time the fleet works
  correctly. Measured 2026-07-30: 88 worktrees, 86 carrying `target/`, 61
  carrying `node_modules`, one sampled `target/` holding 82,891 entries. On the
  first real run the tool found 22 reclaimable worktrees holding **62.32 GiB**
  of build output.
  Reporting is the default and acting is explicit, because the failure mode of a
  cleanup tool is deleting work somebody still needed and no report can be
  un-deleted. It refuses the bare primary, protected branches, any tree with
  uncommitted **or untracked** changes, any tree mid-build, any tree whose
  ownership marker names another session, and any branch whose commits have not
  landed. Every refusal names the rule that stopped it, so a worktree that is
  kept does not read like one the tool failed to notice.
  Landing is judged by patch equivalence (`git cherry`), not commit identity. A
  squash or rebase merge lands every line under a new commit id, so
  `git merge-base --is-ancestor` answers "no" for work that is fully merged —
  the mistake that previously led an agent here to declare 245 merged lines lost
  and queue a PR to restore code already on main.
  The decision for each worktree is a pure function of gathered facts, so all
  six refusals are tested without creating or destroying anything. The tests are
  weighted toward what the tool must *not* take, because a cleanup tool tested
  only on what it deletes has not been tested on what matters.
