- **Worktree reclaim now preserves branches held by live task claims.**
  The reclaimer reads the authoritative Lodestar board before reporting or
  deleting worktrees, refuses every branch named by an unexpired claim, and
  refreshes that state at the destructive boundary. If the board cannot be
  read, reclamation fails closed instead of treating a new clean worktree as
  abandoned residue.
