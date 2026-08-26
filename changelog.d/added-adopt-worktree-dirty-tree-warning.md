- **Added:** adopting a worktree with `--adopt-worktree` now warns when the
  worktree holds uncommitted changes — staged, unstaged, or untracked —
  naming the affected paths, rather than silently transferring ownership of
  a peer's live, unfinished work. This is the local counterpart to the
  existing remote pull-request check: the adopt path could already tell you
  the branch was published elsewhere, but had no way to tell you the
  worktree in front of you was not actually idle. Advisory only, exactly
  like the pull-request check — it never blocks a genuine rescue — because a
  worktree can be legitimately dirty at a deliberate handover. See
  `gaps.d/adopt-worktree-takes-a-peers-uncommitted-work.md` for the incident
  this closes the local half of; the remote half (whether a live Lodestar
  claim still names this branch as its `owner_branch`) remains open and
  deliberately deferred as an ADR-sized decision.
