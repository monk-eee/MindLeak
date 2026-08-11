- **A worktree that has been created but not committed in has no recorded
  owner.** — Observed live: a session created `../MindLeak-ackplane-federation`
  on `feat/ackplane-federation-service`, declared it to both planes, and won a
  claim recorded against that branch. A concurrent session then removed that
  worktree with `git worktree remove`, the branch went with it, and the same
  path was recreated on the peer's own branch `feat/ackplane-node-protocol`.
  The first session's next edits therefore landed in the peer's checkout, and
  `git worktree add` for its own branch failed with `invalid reference`. —
  Cause: the one-writer marker in `scripts/worktree-owner.mjs` is only written
  when `ownershipVerdict` returns `record`, which happens on the *first commit*.
  A worktree that has been created and declared, but not yet committed in, has
  no marker and so reads as unclaimed to any guard that consults one. — Impact:
  silent cross-agent edit mixing, visible only by diffing the peer's worktree.
  Recovered here with `git checkout HEAD --` on the peer's files and a fresh
  worktree for the new crate; nothing was lost. — Left for later.
