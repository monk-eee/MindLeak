- **A fleet hygiene script run from a stale checkout enforces stale rules.**
  [`scripts/worktree-reclaim.mjs`](../scripts/worktree-reclaim.mjs) acts on
  *shared* state — every worktree of the repository — but is executed from a
  *per-checkout* copy of itself. An agent whose checkout predates a fix silently
  runs the pre-fix version against everyone else's work, and nothing in the
  output says which version is speaking. Measured 2026-08-01, the same command
  from two checkouts of the same repository, seconds apart: from a primary
  checkout 96 commits behind `origin/main` it listed
  `docs/revalidate-gap-catalogue` as **reclaimable** while `task:4f2ac532e301`
  held a live claim naming that branch; from a current checkout it reported
  `keep docs/revalidate-gap-catalogue — held by a live Lodestar claim`. Four
  reclaimable versus two, on identical inputs. — Impact: this is the mechanism
  behind the live-claim worktree deletion recorded earlier, and it exonerates
  the fix — the guard added by that work is correct and does exactly what it
  says, but it only protects worktrees when the *runner's own copy* is current.
  A fleet-wide hygiene fix therefore protects nobody until every checkout
  updates, which for long-lived primary checkouts may be never. The class is
  wider than this script: any tool that reads shared state, is executed from a
  per-checkout copy, and takes destructive action has the same shape. — Recorded,
  not fixed. Two directions, neither chosen here because they trade different
  costs: have the script compare its own file against `origin/main` and refuse
  or warn when it is behind, which makes staleness self-reporting at the cost of
  a fetch on every run; or run fleet-hygiene tools only from a designated
  current checkout, which is cheaper but relies on a convention nothing enforces.
