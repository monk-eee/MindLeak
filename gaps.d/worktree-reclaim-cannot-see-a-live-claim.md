- **Worktree reclaim cannot see a live claim, so it deletes a claimant's
  worktree in the window before its first commit.** `classifyWorktree` in
  [`scripts/worktree-reclaim.mjs`](../scripts/worktree-reclaim.mjs) decides from
  git state alone — it keeps a worktree only when it is bare, the running one,
  detached, protected, dirty, mid-build, owned by another session, or unlanded —
  and never consults the Lodestar board. Two of those guards miss a freshly
  claimed task by construction: the `lodestar-owner` marker is stamped on the
  *first commit*, so a worktree that has not committed yet has no owner to
  refuse on; and `hasLanded` reports `true` for empty `git cherry` output, so a
  branch with zero commits reads as fully landed rather than as "nothing to
  judge". A brand-new worktree therefore classifies as `merged and idle` and is
  removed, together with its branch under `--remote`. Measured 2026-08-01:
  `worktree-reclaim --reclaim --remote` reported `reclaimed
  fix/task-claim-surfaces-other-goals` while `task:dac5c578f54e` was live-claimed
  by another session naming that exact branch as its `owner_branch`; the branch
  had 0 commits ahead of `origin/main` at the time, which is precisely why every
  rule said yes. — Impact: disruption rather than data loss. Uncommitted and
  untracked work is still protected by the `dirty` guard, so nothing an agent
  had typed was destroyed, but the claimant loses its checkout and branch during
  the longest stretch of a new task — setup, reading, and planning, all before
  anything is committed — and the claim is left naming a branch that no longer
  exists. The tool cannot distinguish "abandoned residue" from "started three
  minutes ago", because the only evidence it consults appears at first commit. —
  Recorded, not fixed: a fix should consult the board and keep any branch named
  as a live claim's `owner_branch`, or treat a zero-commit branch as unjudgeable
  rather than landed, or grant a creation-age grace — but choosing among those
  is a design decision about how much the cleanup tool is allowed to know.
