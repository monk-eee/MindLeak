- **`make reclaim` no longer orphans the branch belonging to a worktree it
  just dismantled.** `git worktree remove` deletes a worktree's contents and
  its `.git` link *before* unlinking the directory itself, so a Windows handle
  on that directory (an open editor, a shell sitting inside it) failed the
  command after the worktree was already gone. Reclaim read the non-zero exit
  as a refusal, reported `kept <branch>`, and skipped deleting the branch — and
  the `git worktree prune` at the end of the run then deregistered the gutted
  worktree, putting that branch permanently beyond the reach of a tool that
  only inspects registered worktrees. Measured 2026-08-28: three branches
  orphaned this way in a single run. Reclaim now tells a refusal that happened
  before git touched anything (a dirty tree, which is a real keep) apart from a
  failed final unlink, retries that unlink with the same `fs-retry` helper the
  build output already uses, and finishes the reclaim either way. A leftover
  empty directory is reported as named residue rather than as a failure, so a
  completed reclaim is never announced as `kept`.
