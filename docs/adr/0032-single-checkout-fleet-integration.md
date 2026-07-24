# ADR-0032: Single-checkout, single-publisher fleet integration

- Status: Accepted
- Date: 2026-07-24
- Deciders: MindLeak maintainers
- Supersedes: [ADR-0018](0018-conflict-safe-concurrent-editing.md) for repository
  validation and publication; scoped commits and overlap advice remain
- Related: [ADR-0015](0015-advisory-symbol-leases.md) (progressive handoffs),
  [ADR-0024](0024-preflight-overlap-detection.md) (overlap advice),
  [ADR-0025](0025-authoritative-checked-conformance.md) (commit evidence)

## Context

ADR-0018 correctly chose one shared checkout as the primary collaboration model,
but retained temporary worktrees as a validation and publication tactic. Under
load that exception became the workflow: agents published from side lineages,
cherry-picked equivalent changes onto advancing remote history, and moved local
branch refs beneath a dirty checkout to catch up.

The result was high patch throughput but poor convergence. The primary `main`
diverged substantially from `origin/main`, several logical changes acquired two
commit ids, and valid remote changes appeared as local edits after ref movement.
That is especially damaging here because MindLeak identifies commit intent and
conformance evidence by commit SHA: routine cherry-picking turns one logical
change into multiple graph identities and splits its provenance.

The repository needs one history that agents extend together, not independent
release trains that are reconciled after every task.

## Decision

1. **One physical checkout is the repository workflow.** Agents work together in
   the primary checkout. Repository scripts and hooks must not create or depend
   on Git worktrees. External worktree compatibility is not a recommended
   contributor workflow.
2. **One shared fleet branch per coordinated wave.** `main` remains a clean
   tracking branch. Agents claim tasks and make scoped commits on a shared
   `fleet/<goal>` branch; they do not switch branches independently.
3. **One publisher.** Exactly one designated integrator fetches, reconciles,
   validates, pushes the fleet branch, and updates its pull request. Other agents
   edit and commit only. Publication runs from the primary checkout through
   `scripts/canonical-push.mjs`.
4. **Publication preserves identity.** The publisher pushes the current branch's
   exact `HEAD` to the same remote branch. It refuses linked worktrees, detached
   heads, `main`/`master`, staged index state, and a remote branch that is not an
   ancestor of `HEAD`. It has no alternate commit or destination-branch option.
5. **Validate committed bytes without another checkout.** Cargo hooks materialize
   the staged or committed tree through a temporary Git index and
   `checkout-index`. The snapshot owns no branch or ref and cannot move `main`.
6. **Cherry-pick is exceptional.** It is reserved for an explicit backport or
   human-approved disaster recovery, with rationale recorded in the resulting
   commit or review. It is never the normal way to publish concurrent work.
7. **Divergence is a stop signal.** When the remote advances incompatibly, agents
   stop taking new work, finish or release current claims, reach a scoped clean
   checkpoint, and reconcile once in the primary checkout. No agent moves a
   branch ref underneath dirty files or repairs divergence from a side checkout.
8. **`main` changes through review.** The fleet branch is merged through the
   repository's pull-request and branch-policy path after integrated validation.

## Consequences

- Commit identity, MindLeak intent nodes, and Lodestar evidence remain aligned
  with the history reviewers actually merge.
- Individual agents may wait at a convergence barrier instead of landing patches
  independently. That is intentional: integrated throughput is the metric.
- Scoped commits and advisory overlap checks remain necessary because agents
  still share files and one index.
- A temporary-index snapshot is more plumbing than a worktree, but it is bounded,
  branchless, platform-agnostic, and regression-tested.
- Any pre-existing detached checkout is migration debt: preserve its WIP, recover
  it into the shared fleet branch through ordinary commits or a reviewed merge,
  then remove it. Do not cherry-pick it into `main` merely to make it disappear.
