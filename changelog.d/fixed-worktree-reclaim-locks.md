- Worktree reclamation and artifact sweeping honor explicit Git worktree locks
  before deleting build output. Reclamation rereads worktree facts after the
  preview and refuses missing or changed identities, preserving retained
  checkouts and their artifacts while leaving unlocked cleanup unchanged.
