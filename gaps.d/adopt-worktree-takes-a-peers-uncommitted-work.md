- **`--adopt-worktree` takes a worktree that holds a peer's uncommitted work,
  and its only pre-flight was a pull-request lookup — NARROWED 2026-08-26, one
  residual still OPEN.** — Observed 2026-08-26. `checkWorktreeOwnership` in
  [`scripts/worktree-owner.mjs`](../scripts/worktree-owner.mjs) resolves the
  verdict through `ownershipVerdict`, which returns `action: "record"` for
  `adopt` *before* considering the recorded owner. The adopt path's only
  pre-flight was `checkExistingPullRequests`, which asks GitHub whether the
  branch already has a pull request and is deliberately advisory ("Never a hard
  refusal", so a stale or unauthenticated `gh` cannot block a genuine rescue).
  Nothing in that path looked at the working tree.

  The incident: a worktree was created for a slice of ADR-0125 work, then left
  clean and idle when its Lodestar claim was refused. A peer adopted it and
  began writing there. About twenty minutes later the original creator ran
  `--adopt-worktree` on it, believing it was still their own idle worktree. It
  held the peer's uncommitted work at that moment — five modified files under
  `crates/ackplane-server/src/` plus an untracked
  `migrations/0038_work_task_command_execution.sql`. The branch had no pull
  request, so the pre-flight found nothing and printed nothing, and ownership
  transferred silently away from the agent actively writing there. `git switch`
  in the same command *did* refuse ("Your local changes would be overwritten"),
  which is the only reason the collision was noticed at all. Recovered by
  restoring the previous marker value by hand; the peer's tree was untouched.

  Impact is wider than one mistake, because the marker is load-bearing twice
  over. It is what the non-waivable invariant
  `goal:an-agent-commits-only-in-a-working-tree-it-owns` is enforced from, and
  it is what [`scripts/worktree-reclaim.mjs`](../scripts/worktree-reclaim.mjs)
  reads to decide `keep` against `reclaim`. That tool currently reports `135
  more are held by other sessions` out of roughly 139 worktrees, and its own
  remedy line suggests `--adopt-worktree` for a worktree "whose session is
  provably gone". Following that suggestion in bulk would take worktrees that
  still hold live uncommitted work, since a session being gone from the ledger
  says nothing about whether a dirty tree was left behind.

  **Fixed: the local signal.** The adopt path never consulted whether the
  working tree is dirty, which is the local counterpart to the PR check's
  remote one — the incident's own uncommitted files (`crates/ackplane-
  server/...`, the untracked migration) are exactly what this now surfaces.
  `checkWorkingTreeDirty` (`scripts/worktree-owner.mjs`) runs `git status
  --porcelain` and reports every staged, unstaged, and untracked path;
  `dirtyWorkingTreeWarning` renders it the same way the PR warning already
  was — printed loudly on `--adopt-worktree`, never a refusal, for the same
  reason the PR check never refuses: a worktree can be genuinely,
  legitimately dirty at a deliberate handover, and refusing outright would
  break the rescue case
  `gaps.d/rescuing-a-lapsed-lease-can-duplicate-a-published-pr.md` exists to
  support. Advisory, not enforcement, so this does not touch the ADR-sized
  question below.

  Caught its own bug on the way in, worth recording as its own lesson: the
  first implementation reused the module's existing `capture` helper, which
  calls `.trim()` on its *entire* return value. For a single-line git command
  that is harmless; for multi-line `git status --porcelain` output it silently
  eats the leading status-code space off the first line only, shifting every
  fixed-offset slice on that line by one column (`" M tracked.txt"` reads as
  `"racked.txt"`). The unit tests for the pure parser never caught it, because
  they injected raw strings directly rather than going through the trimming
  helper — only the end-to-end test (a real git worktree, a real dirty file,
  the real CLI invocation) reproduced it. Fixed by giving multi-line callers
  an untrimmed capture (`captureUntrimmed`) instead of reusing the
  single-line one. Recorded because it is the same lesson this fragment's own
  PORTABLE note gives one level down: a helper that is correct for the shape
  of input it was written for can corrupt a different shape silently, and
  only a test that exercises the real wiring — not just the pure function —
  catches it.

  **Still OPEN — the remote signal.** Whether a live Lodestar claim names
  this branch as its `owner_branch` is not checked, deliberately left for
  later: this is the enforcement path of a non-waivable invariant, and asking
  a running Lodestar server from a standalone git hook is an ADR-sized
  decision about what a handover means and what the hook may depend on being
  reachable, not a drive-by addition alongside a git-only, dependency-free
  check.

  PORTABLE: an ownership-transfer command whose only safety check is a *remote*
  lookup will happily take *local* work, because the two are independent. Any
  "take this over" verb needs to consult the local state it is about to become
  responsible for, not just the published record of it.
