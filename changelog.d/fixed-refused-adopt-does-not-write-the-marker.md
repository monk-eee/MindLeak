### Fixed

- **A refused `worktree-owner.mjs --adopt-worktree` no longer leaves the
  ownership marker already overwritten.** The marker was written as a side
  effect of resolving the verdict, strictly before the adopt gates (existing
  pull request, dirty tree, live foreign Lodestar claim) could refuse and exit
  4. A correctly-refused adopt therefore recorded the refused session as the
  worktree's owner, so the legitimate claim-holder's next commit was refused by
  this same script naming the wrong owner — the exact collision the script
  exists to prevent, caused by the script, and recoverable only by editing the
  marker back by hand. The CLI now resolves every gate first and performs a
  single write at one call site reached only once nothing has exited, so a
  refusal is a no-op on disk rather than a non-zero exit an operator is trusted
  to notice and undo. An uncontested adopt, and `--override-active-claim`,
  still record as before.
- `checkWorktreeOwnership` accepts `record: false` to compute the verdict and
  resolve the marker path without touching the disk, and returns that path;
  the new `recordWorktreeOwner` performs the write. Recording remains the
  default, so `scripts/scoped-commit.mjs`'s existing call is unchanged.
