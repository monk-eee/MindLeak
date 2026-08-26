- **Added:** `--adopt-worktree` now refuses by default when the branch being
  adopted has a live, unexpired Lodestar claim held by a session other than
  the caller's (ADR-0130). Unlike the existing pull-request and dirty-tree
  pre-flights — both deliberately advisory, since a PR or a dirty tree can be
  genuinely ambiguous — this one is not: `task_claim`'s own compare-and-swap
  already treats "claimed, unexpired, a different owner" as an unconditional
  loss, so an adopt that bypassed it was a real inconsistency between two
  enforcement paths for the same invariant. `--override-active-claim` exists
  for the rare, genuine case a human has independently confirmed the record
  is wrong. A lapsed lease — the actual rescue case this mechanism exists to
  support — never satisfies the check, so it is unaffected; an unreachable or
  unbuilt Lodestar server degrades to the existing advisory-only behaviour,
  never a way to block a genuine rescue. This closes the remote half of
  `gaps.d/adopt-worktree-takes-a-peers-uncommitted-work.md`, whose local half
  (a dirty-tree warning) shipped earlier; with both signals closed, the
  fragment is retired.
