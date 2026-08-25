- **`--adopt-worktree` takes a worktree that holds a peer's uncommitted work,
  and its only pre-flight is a pull-request lookup — OPEN.** — Observed
  2026-08-26. `checkWorktreeOwnership` in
  [`scripts/worktree-owner.mjs`](../scripts/worktree-owner.mjs) resolves the
  verdict through `ownershipVerdict`, which returns `action: "record"` for
  `adopt` *before* considering the recorded owner. The adopt path's only
  pre-flight is `checkExistingPullRequests`, which asks GitHub whether the
  branch already has a pull request and is deliberately advisory ("Never a hard
  refusal", so a stale or unauthenticated `gh` cannot block a genuine rescue).
  Nothing in that path looks at the working tree.

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

  Two signals the adopt path never consults, deliberately named here rather
  than patched: whether the working tree is dirty, and whether a live Lodestar
  claim names this branch as its `owner_branch`. Left for later on purpose —
  this is the enforcement path of a non-waivable invariant, so tightening it is
  an ADR-sized decision about what a handover means, not a drive-by change. A
  refusal that is too eager would break the genuine rescue case
  `gaps.d/rescuing-a-lapsed-lease-can-duplicate-a-published-pr.md` exists to
  support.

  PORTABLE: an ownership-transfer command whose only safety check is a *remote*
  lookup will happily take *local* work, because the two are independent. Any
  "take this over" verb needs to consult the local state it is about to become
  responsible for, not just the published record of it.
