- **Fixed:** the delivery queue (`scripts/delivery-queue.mjs`) no longer
  trusts a cached `mergeStateStatus: DIRTY` verdict at face value. GitHub
  recomputes that field lazily, and after a burst of merges it can keep
  reporting `DIRTY` for a branch that in fact merges cleanly — the queue would
  strand it in the blocked list and repeat the same false refusal every tick.
  `nextAction` now accepts an injected `verifyDirty(pr)` predicate (a no-op by
  default, so existing behaviour is unchanged unless a caller opts in); the
  real implementation (`verifyDirtyWithGit`, wired into `main()`) fetches
  `origin/main` and the branch ref and asks `git merge-tree --write-tree`
  whether they actually conflict, never touching the working tree, index, or
  any ref. A verdict `verifyDirty` disproves is corrected to `BEHIND` for that
  tick so the branch is updated like any other; a verdict it confirms still
  reports and steps over exactly as before. Closes
  `gaps.d/the-delivery-queue-trusts-a-stale-conflict-verdict.md`.
