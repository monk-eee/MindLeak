### Fixed

- The delivery queue (`scripts/delivery-queue.mjs`) no longer trusts a
  successful `gh pr update-branch` call at face value. It now computes the
  expected post-merge tree locally (`git merge-tree --write-tree`) before
  calling `update-branch`, and compares the branch's actual tree against
  that expectation once it returns. A mismatch is reported loudly instead of
  the plain `updated #N` line — this is the exact shape of defect that once
  let PR #507's merge silently drop one side of a real three-way merge with
  a clean exit code and no failing check. See
  `gaps.d/update-branch-can-silently-drop-a-conflicts-losing-side.md`.
