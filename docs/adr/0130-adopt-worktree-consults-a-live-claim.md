# ADR-0130: Adopting a worktree refuses a claim it would not be handed

- Status: Accepted
- Date: 2026-08-26
- Deciders: MindLeak maintainers
- Related: [ADR-0038](0038-isolated-worktrees-shared-repository-state.md) (isolated
  worktrees, shared repository state), [ADR-0024](0024-preflight-overlap-detection.md)
  (preflight overlap detection), [ADR-0048](0048-a-lapsed-lease-holes-the-window-it-does-not-move-it.md)
  (a lapsed lease holes the window, it does not move it),
  `gaps.d/adopt-worktree-takes-a-peers-uncommitted-work.md`,
  `gaps.d/rescuing-a-lapsed-lease-can-duplicate-a-published-pr.md`

## Context

`scripts/worktree-owner.mjs --adopt-worktree` records a deliberate handover of
a linked worktree from whatever session (if any) the ownership marker names to
the session invoking it. Until now its only pre-flight checks answered two
questions: does the branch already have a published pull request (a *remote*
signal, `gh pr list`), and is the working tree dirty (a *local* signal, `git
status --porcelain`, added in this same fragment's prior fix). Both are
deliberately advisory — printed as a warning, never a refusal — because a
worktree can be genuinely, legitimately handed over while dirty or while its
branch carries an abandoned PR, and refusing outright would break the
legitimate rescue case `gaps.d/rescuing-a-lapsed-lease-can-duplicate-a-published-pr.md`
exists to support.

Neither check asks Lodestar anything. That is the gap this ADR closes: nothing
in the adopt path consults whether a **live, unexpired claim** already names
this exact branch as its `owner_branch`, held by a session other than the one
adopting.

This is not the same shape of ambiguity as the other two signals, and that
difference is the whole argument below:

- **A dirty tree is ambiguous.** It could be a peer's live edit, or it could be
  build output, an editor swap file, or debris from an aborted experiment.
  Nothing in the tree itself says which.
- **A published PR is ambiguous.** It could be open and current, or closed,
  abandoned, or superseded. The branch having *a* PR says nothing about whether
  that PR still represents live intent.
- **A live Lodestar claim is not ambiguous.** `task_claim`'s own compare-and-swap
  already treats "a task is `claimed`, its lease has not expired, and the
  caller is not the owner" as an unconditional loss — that is the one fact this
  entire coordination system exists to make authoritative, not a heuristic
  about it. If that same fact is true of the branch a worktree sits on, and the
  worktree is adopted anyway, the ownership transfer that `--adopt-worktree`
  performs at the git layer has just handed over exactly the thing `task_claim`
  at the Lodestar layer would have refused to hand over a moment earlier
  through the front door. The adopt path is not ambiguous about this fact; it
  simply never asks.

Concretely: the same rescue this mechanism protects — an agent picking up
work whose lease has genuinely lapsed — is untouched by this decision, because
a lapsed lease is by definition not a live claim. This ADR narrows the case
where the lease has **not** lapsed and is held by someone else, which is not a
rescue by this system's own vocabulary; it is a collision `task_claim` would
already refuse.

## Decision

**`--adopt-worktree` queries Lodestar for a live claim on the branch being
adopted, and refuses by default when one is held by a session other than the
caller's.**

1. **The check.** Immediately before recording the handover, query
   `task_query view=board branch=<branch> include_terminal=false` against the
   repository's Lodestar server (the same `resolveServer`/`callTools` plumbing
   `scripts/canonical-push.mjs` already uses — no new wiring invented). A task
   is a blocking claim when `status === "claimed"`, `lease_expires_at` is a
   number greater than the current time, and `owner` is not the adopting
   session's resolved agent id.
2. **Refusal is the default, unlike the dirty-tree and PR checks.** Those two
   remain advisory for the reason given above — they are ambiguous signals.
   This one is not: it mirrors the exact condition `task_claim` itself already
   refuses, so a refusal here does not invent a new rule, it closes a path that
   silently bypassed an existing one. The refusal names the owning session and
   the task, matching `refusalMessage`'s existing style.
3. **An explicit override exists, and is one flag, not a dance.**
   `--adopt-worktree --override-active-claim` proceeds anyway. This is for the
   genuine, rare case where a human has independently confirmed the Lodestar
   record is wrong or the situation warrants overriding it — the same posture
   ADR-0034 already takes toward mechanical enforcement generally: refuse by
   default, let a deliberate override name itself, never require an
   uninstall-and-reinstall-shaped workaround.
4. **Unreachable Lodestar degrades to today's behaviour, exactly.** If no
   server binary resolves, the process cannot be reached, or the board cannot
   be parsed, the check reports unchecked and adoption proceeds with only the
   existing PR/dirty-tree warnings — printed, never silent, matching the
   existing `gh`-unavailable degradation in `existingPullRequestWarning`. A
   stale or unreachable ledger must not become a way to block a genuine
   rescue, the same principle the PR check was already built on.
5. **Only the `--adopt-worktree` path changes.** The default (first-commit)
   ownership path is untouched: it already only ever claims an *unclaimed*
   marker, and a worktree with a recorded owner already refuses a different
   session's commit outright (the existing `action: "refuse"` path). This
   decision is scoped to the one path that previously had no Lodestar-aware
   check at all.

## Consequences

- The concrete incident `gaps.d/adopt-worktree-takes-a-peers-uncommitted-work.md`
  records — a worktree adopted out from under a peer who was still actively
  claimed on that exact branch — becomes a refusal by default instead of a
  silent transfer, closing the fragment's second, previously-deferred signal.
- **A genuinely stranded worktree (owner gone, lease lapsed) is unaffected.**
  This is the majority case `worktree-reclaim.mjs`'s own remedy line points
  at, and the whole reason `--adopt-worktree` exists; nothing here makes that
  path slower or noisier, because a lapsed lease never satisfies this check's
  condition.
- **A worktree can now be refused for adoption even though its local state
  (clean tree, no PR) looks perfectly idle.** This is the point, not a cost:
  local state was never sufficient to answer the question Lodestar is
  authoritative for, and this decision is what makes that visible instead of
  silently wrong.
- **This adds a Lodestar round-trip to `--adopt-worktree` specifically**,
  where none existed before. Bounded to one additional `task_query` call, on a
  path that is already a deliberate, occasional, human-invoked action rather
  than a per-commit hook — the cost profile this repository has already
  accepted for `canonical-push.mjs`'s own several Lodestar calls at publish
  time, not the tighter budget `scoped-commit.mjs` holds itself to for every
  commit.
- **The override flag can be misused** to force through a transfer Lodestar
  correctly flagged. This is accepted for the same reason the PR and
  dirty-tree checks are advisory at all: the alternative is a hard refusal
  with no exit, which history in this repository (`rescuing-a-lapsed-lease-can-duplicate-a-published-pr.md`)
  shows produces its own workarounds when the genuine case cannot get through.
  Naming the override explicitly, rather than letting silence be the escape
  hatch, is what keeps the misuse visible in the command someone ran.
