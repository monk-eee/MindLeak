- **The pre-commit stash race reports a failure that names the wrong thing —
  GUARDED, not fixed.** — `pre-commit` stashes every unstaged change before
  running hooks and restores it afterwards. Alone that is invisible; in a fleet
  it corrupts. If a second agent writes to the same working tree inside that
  window, the restore collides and hooks report `files were modified by this
  hook` — from `check-added-large-files` and `check-merge-conflict`, which modify
  nothing, about files the committer never touched. Observed Jul 2026: three
  consecutive commit attempts failed this way, each blamed a different innocent
  hook, and the real cause (two agents in one checkout) appeared nowhere in the
  output. The natural response is to retry, which widens the window. — Medium
  impact, high cost to diagnose: no data is lost, but the diagnosis is
  actively misleading and can consume an entire session. — `scoped-commit.mjs`
  now refuses (exit 3) when more than one worktree is attached and unstaged
  files outside the declared paths are live, naming them and pointing at
  `git worktree add`. That closes the sanctioned path only: a bare `git commit`
  can still walk into it, because the stash happens inside `pre-commit` itself
  and no hook can observe the tree as it was before its own framework moved it.
  The real fix is ADR-0038 isolation — one worktree per workstream.
