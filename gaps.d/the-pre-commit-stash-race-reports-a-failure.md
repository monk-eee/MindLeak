- **Bare commits can still hit the pre-commit stash race and report the wrong
  failure — GUARDED, OPEN.** `pre-commit` stashes every unstaged change before
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
  **Narrower than that fix implies — reproduced 24 Aug with proper isolation
  already in place.** One agent, two worktrees, each holding exactly one
  workstream (`docs/claim-transfers-drill-result` and
  `docs/gap-fragment-restaleness-sweep`), no other agent attached to either. A
  plain `git checkout -- <file>` in worktree A (discarding a local edit, not a
  commit) fired the `post-checkout` hook chain, printed pre-commit's
  "Stashing unstaged files" / "Restored changes from" pair, and the restore
  left worktree A holding an exact, uncommitted copy of an edit that had only
  ever been made in worktree B (`gaps.d/the-board-hands-one-adr-to-several-agents-at-once.md`,
  15 insertions/4 deletions, byte-identical) — worktree B's own copy of that
  edit was still intact afterward, so nothing was lost, but worktree A briefly
  held content it should not have. `git stash list` from either worktree shows
  the same entries (ten, oldest from 3 Aug, one per paused worktree),
  confirming `refs/stash` is genuinely repository-wide, not worktree-scoped,
  in this checkout. **This means one-worktree-per-workstream is necessary but
  not sufficient**: it stops two agents racing the same checkout, but it does
  not stop a single, disciplined agent's own `git checkout` in one worktree
  from tripping a stash push/pop that can read or write across every worktree
  sharing the same `refs/stash`. Left for later: not fixed this run — a real
  fix would need either a per-worktree stash scope (git has no such thing) or
  the hook chain avoiding `git stash` in favour of a mechanism that cannot
  cross worktree boundaries.
