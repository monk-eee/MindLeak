- **A worktree whose untracked work is provably superseded is kept forever,
  because the rule that protects unfinished work cannot tell it from finished
  work — MEASURED 2026-08-13, left OPEN.** `MindLeak-ackplane-federation` sits
  on `feat/ackplane-node-protocol`, 169 commits behind `origin/main`, with no
  `lodestar-owner` marker (nobody ever committed in it) and a HEAD whose commits
  are already on main. It is dirty with `M Cargo.lock`, `M Cargo.toml` and an
  untracked `crates/ackplane-protocol/`.

  That crate has already shipped. Comparing all five files against
  `origin/main`:

  | | stranded | on main |
  |---|---|---|
  | `node_sync.proto` | 99 lines, 14 declarations | 236 lines, 27 declarations |
  | declarations found only here | **0** | 13 |

  The 13 are the whole `NodeEnrollmentService` and key rotation. The stranded
  copy is a strict earlier subset, so discarding it loses nothing.

  `classifyWorktree` in
  [`scripts/worktree-reclaim.mjs`](../scripts/worktree-reclaim.mjs) nevertheless
  keeps it, reporting "uncommitted or untracked changes". **That refusal is
  correct and must not be weakened.** The defect is not the refusal but that it
  has no exit: nothing revisits the decision, and untracked files never become
  clean on their own, so every audit from here to forever declines this worktree
  for the same reason and moves on.

  Why that matters rather than being untidy: this repository has already
  measured where the pattern ends — 88 worktrees, 149.18 GiB, one sampled
  `target/` holding 82,891 entries, which is what made the editor slow enough to
  be unusable. The accumulation is not the tool failing; it is the tool
  succeeding at a rule that only ever adds to the kept set.

  Left open deliberately. The obvious repair — sweep a worktree whose untracked
  content matches something already landed — asks a cleanup tool to decide that
  somebody's uncommitted work is worthless, from a content comparison it has no
  reliable basis to make, with an outcome nobody can undo. A near-miss there
  costs far more than the disk it reclaims. The safer direction, if this is
  taken up, is to *report* superseded-looking worktrees for a person to confirm,
  and to leave what the tool may delete exactly where it is.
