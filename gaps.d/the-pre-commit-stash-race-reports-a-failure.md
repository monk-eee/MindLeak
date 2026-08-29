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
  **The checkout half is CLOSED, 2026-08-29 — measured, not reasoned.**
  `post-checkout` no longer runs through pre-commit at all. It is installed
  directly by `node scripts/install-hooks.mjs` (wired into `make setup`), so the
  one hook this repository runs at that stage — `worktree-owner
  --stage=post-checkout`, which writes an ownership marker and never reads the
  working tree — runs without a snapshot.

  Before and after on the same single-file `git checkout HEAD -- <path>` with an
  otherwise identical tree: **22 lines of output, a patch written to the shared
  user-level cache (`~/.cache/pre-commit/patch<ts>-<pid>`), the tree discarded,
  20 hooks evaluated, and the patch reapplied — versus 0 lines and no snapshot
  at all.** For an operation that changes no index and makes no commit.

  This removes the window rather than narrowing it: on the checkout path there
  is now no patch file, no shared cache directory, and no interval in which the
  tree lives outside the repository, so nothing can collide there regardless of
  the precise mechanism by which the 24 Aug cross-worktree copy occurred. That
  mechanism was never pinned down and is deliberately *not* claimed as fixed —
  the fragment above attributes it to `refs/stash` being repository-wide, which
  is an inference: pre-commit does not use `git stash`, it writes a patch file,
  so the `git stash list` evidence cited there is about unrelated manual
  stashes. The honest statement is that the checkout no longer opens a window,
  not that the window's contents were understood.

  `hook-health` was taught the difference between "no hook" and "the wrong
  installer's hook", because every clone made before this change still has
  pre-commit's `post-checkout` shim sitting there — running, reporting healthy,
  and doing exactly the thing being removed.

  **Still OPEN for `pre-commit` and `pre-push`.** Those stages genuinely need
  the tree to hold exactly what is staged, so the snapshot is the price of
  admission and cannot simply be dropped. A bare `git commit` in a shared
  checkout can still hit the original race; `scoped-commit.mjs` remains the
  guarded path (exit 3), and ADR-0038 isolation remains the standing advice.

  **The observation that motivated the fix — reproduced 24 Aug with proper
  isolation already in place.** One agent, two worktrees, each holding exactly
  one workstream (`docs/claim-transfers-drill-result` and
  `docs/gap-fragment-restaleness-sweep`), no other agent attached to either. A
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
  from tripping a snapshot/restore that can read or write across every worktree.
  (The `refs/stash` attribution above is an inference and reads as fact; it is
  almost certainly wrong in its specifics, since pre-commit writes a patch file
  rather than using `git stash`, and those ten stash entries are unrelated
  manual ones. The *observation* — worktree A briefly holding worktree B's
  uncommitted edit — stands; the mechanism named for it does not.)
