# ADR-0149: Commit attribution repair is verified and audited

- Status: Proposed
- Date: 2026-09-10
- Review: implementation proposed in PR #922; no human adoption asserted.
- Related: [ADR-0009](0009-evidence-backed-conformance.md),
  [ADR-0010](0010-observability-and-resilience.md),
  [ADR-0038](0038-isolated-worktrees-shared-repository-state.md)

## Context

The publication recorder supplied a cumulative branch diff as the changes of
`0b6860b7b5950d2dd289e108fbb1731ae5511a5e`, a nine-file commit. Its stored
`refactored` edges consequently claimed earlier branch work. Correcting the
publisher prevents new pollution, but append-and-reinforce ingestion cannot
retract the false edges. Filtering a completion bundle hides the defect;
deleting artifacts also destroys unrelated history.

A full hash existence check is insufficient authority for deletion: it proves
that an object exists, not that caller-supplied replacement facts describe it.
A shallow clone also makes a boundary commit appear to introduce its whole
tree, so successful Git output alone is not proof of a complete delta.

## Decision

1. Provide one explicit, registered-session repair operation accepting only
   a full commit hash and reason. Read metadata and changed paths from the
   configured repository's immutable Git object, never from client metadata.
   Reject unavailable or shallow history; disable replacement refs, external
   diff programs and lazy fetching. Retain the existing combined-diff rule:
   clean merge integration is not authored work.
2. Keep Git execution in `mindleak-storage`, injected into the core facade.
   The graph engine remains deterministic and never spawns Git or calls a model.
   Share the existing commit-node and path construction with ordinary ingestion.
3. In one immediate write transaction, correct only the named intent's commit
   metadata and `refactored` edge set. Preserve unrelated facts, observations,
   retained reinforcement and weights; correct edge clocks to commit time.
   Identical replay is a no-op, not evidence reinforcement.
4. Append complete before/after facts, acting agent, reason and repair time to
   the existing durable telemetry store in that same transaction. A missing
   audit is a failed repair and rolls back every mutation. Expose exact audit
   event lookup through `telemetry_snapshot(event_id)` rather than creating
   another ledger. Bound input and audit size; never truncate correction proof.
5. Do not attribute the original commit to the repairer and do not rewrite
   conformance history. Generate fresh evidence for the original claim window
   and run conformance separately. The historical record of a bad decision,
   if one exists, remains historical fact even after its source is corrected.

## Consequences

- Existing polluted attribution becomes recoverable without deleting unrelated
  graph history, fabricating a replacement commit, or silently editing SQLite.
- Operators must have complete local Git history and an existing commit intent.
  A repair cannot resolve fabricated hashes or recover objects absent from Git.
- Ordinary ingestion still accepts reported facts and is not a replacement for
  audited correction. A stale writer can report bad facts again; a subsequent
  repair records a new correction rather than concealing that event.
- The authority remains the local stdio threat model: registered attribution
  is not network authentication, and this adds no remote listener or sandbox.

## Verification

Tests cover historical exact-SHA lookup, original timestamps and rationale,
clean/conflicted merges, replacement refs, shallow history, invalid sources,
unrelated history, audit rollback, idempotency, session binding, refusal of
client replacement facts, and durable audit retrieval beyond recent events.
