- **`worktree-reclaim` reports `kept ... Permission denied` for worktrees it
  has already reclaimed, so the operator reads a completed reclaim as a
  no-op.** Observed 2026-08-28 running
  `node scripts/worktree-reclaim.mjs --reclaim --remote` on Windows against
  `agents/claim-work-feature`, `agents/mindleak-fresh-session-coordination`,
  and `agents/mindleak-lodestar-session-setup`. Each printed
  `worktree-reclaim: kept <branch> — error: failed to delete '<path>':
  Permission denied`, but `git worktree list` afterwards no longer showed any
  of them and every directory was left empty (0 items): the worktree really
  was deregistered and its 2.74 GiB of build output really was freed. Only the
  final `rmdir` of the now-empty directory failed, because another process
  (an editor or a shell whose working directory is inside it) still holds a
  handle — the same class of Windows lock `scripts/fs-retry.mjs` was added for
  in PR #798, but applied to build output rather than to the directory removal
  itself. Impact: the summary undercounts what was reclaimed and names
  branches as held when they are gone, so an operator may re-run the command,
  believe the disk is still occupied, or (worse) trust `kept` as evidence that
  a branch's worktree still exists. Left for later: the honest fix is for
  reclaim to distinguish "worktree removed, empty directory could not be
  unlinked" from "worktree kept", and to reuse the existing `fs-retry` helper
  for that unlink rather than reporting a completed reclaim as a failure.
  Found while reclaiming after ADR-0115 slice 1 merged (PR #799); not fixed
  this run because it sits outside that task's declared scope.
