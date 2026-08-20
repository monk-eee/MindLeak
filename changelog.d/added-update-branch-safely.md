- **Added:** `scripts/update-branch-safely.mjs <pr-number>` runs `gh pr
  update-branch` with the same tree-mismatch guard the delivery queue already
  applies to its own queued updates, for anyone reconciling a branch by hand
  outside the queue (gaps.d/update-branch-can-silently-drop-a-conflicts-losing-side.md).
