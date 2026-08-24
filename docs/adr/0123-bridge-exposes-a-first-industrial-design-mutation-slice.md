# ADR-0123: Bridge exposes a first bounded Industrial Design mutation slice

- Status: Accepted
- Date: 2026-08-24
- Deciders: MindLeak maintainers
- Accepted: 2026-08-24 by the repository owner, authorized directly in
  session — attributed human adoption after review.
- Refines: [ADR-0121](0121-industrial-design-preserves-immutable-history.md)
  decision 7 (extends the first Bridge Design Board with three
  safety-justified mutations; the broader legality-of-transition policy,
  per-operator identity, separation of duties, and any Local-repository-
  affecting directive remain deferred, unchanged from that decision)
- Depends on: [ADR-0111](0111-bridge-recovers-a-stranded-claim-as-a-tenant-scoped-administrative-action.md)
  (the precedent this ADR follows: a Bridge mutation is safe without a caller
  identity when the store's own check is the real safety net),
  [ADR-0098](0098-connection-trust-reuses-the-enrolled-key-oidc-waits.md)
  (Bridge's tenant-scoped, loopback-gated administrative action model; OIDC's
  authenticated principal remains deferred), ADR-0121 decisions 3 and 4 (the
  idempotent `create_design` and idempotency-keyed `record_materialization`
  guarantees this ADR relies on rather than re-invents)
- Related: [ADR-0105](0105-bridge-is-the-server-version-of-the-vsix.md)
  decision 5 (the Bridge becomes the human control room for active work)

## Context

ADR-0121 decision 7 deliberately scoped the first Bridge Design Board to
read-only, reasoning that "reading a design does not authorize a consequential
lifecycle change" and that write commands need "separately reviewed principal
authorization, version compare-and-swap, idempotency, separation of duties,
durable receipts, and any typed enrolled-node directive needed to affect a
Local repository" — properties Bridge did not have on 2026-08-23, the day that
decision was accepted.

One day later, the repository owner directed that the Design Board become
interactive rather than a passive viewer — a design point already implied by
ADR-0105 decision 5, which names the Bridge's first Industrial workflow as
"coordinating agents from the Bridge," not observing them.

Bridge today still has no per-operator identity: ADR-0098 decision 4's OIDC
principal remains deferred, and Bridge resolves only a single development
tenant from a loopback-only salt file (`ACKPLANE_BRIDGE_SALT_PATH`). Any
mutation exposed today inherits that same absence of identity — exactly the
gap ADR-0111 already solved once, for `recover`, by locating the safety
property in the *store's own comparison* rather than the caller's identity,
rather than by inventing a pseudo-node signing key Bridge has no legitimate
claim to.

That precedent does not transfer for free. `ClaimStore::recover`'s
compare-and-swap is a one-way, self-correcting temporal predicate — a lease
only ever becomes *more* expired between when an operator observes it and when
they act, never less, so a stale read is still a true read. A design's
`lifecycle_state` has no such direction: it can move to any other state by any
subsequent decision, so a bare, unconditional write is a genuine two-writer
race. `DesignStore::record_decision` had no defense against that race at all
before this ADR — its own doc comment said plainly that "legality of a
particular transition... is deliberately NOT enforced here." That specific gap
had to close before *any* caller, Bridge or otherwise, could safely expose
`record_decision` as a mutation; it is a precondition for this ADR, not a
consequence of it.

`create_design` and `record_materialization` already carry exactly the kind of
store-level safety net ADR-0111 relied on: `create_design`'s digest-checked
idempotent insert (ADR-0121 decision 3) and `record_materialization`'s
idempotency-key contract (ADR-0121 decision 4) both already refuse an unsafe
concurrent write on their own, unconditionally, regardless of who calls them.

## Decision

**`DesignStore::record_decision` gains a compare-and-swap parameter closing
its pre-existing race, and Bridge exposes exactly three Design mutations —
propose, record a lifecycle decision, and record a materialization revision —
each justified by its own store-level safety net rather than a caller identity
Bridge does not have. Legality-of-transition policy, per-operator identity,
separation of duties, and any Local-repository-affecting directive remain
deferred, exactly as ADR-0121 decision 7 left them.**

1. **`RecordDecisionRequest` gains `expected_lifecycle_state`.** Mirroring
   `ClaimStore::recover`'s compare-and-swap, the guarded
   `UPDATE industrial_designs SET lifecycle_state = $new ... WHERE
   lifecycle_state = $expected` only lands if the stored row still matches
   what the caller last observed. A mismatch returns
   `DesignStoreError::LifecycleStateConflict { design_id, expected, actual }`
   rather than silently applying; an unknown `design_id` returns
   `DesignStoreError::UnknownDesign` rather than surfacing a raw foreign-key
   error. The decision-history row is appended only after the guarded update
   actually lands, inside the same transaction, so a rejected compare-and-swap
   leaves no partial trace. This closes the store's own concurrency gap
   independently of who calls it — the same "safety lives in the store, not
   the caller" property ADR-0111 established for `recover`, applied to a
   two-directional race rather than a one-directional expiry check.
2. **Bridge holds `Arc<Mutex<DesignStore>>` and
   `Arc<Mutex<MaterializationStore>>`**, exactly as it already holds
   `ClaimStore` (ADR-0111 point 1) — a `Mutex` because these stores' mutating
   methods take `&mut self`, not a new authority: the store's own logic is the
   only thing that decides whether a mutation succeeds.
3. **`POST /api/v1/repositories/:repository_id/designs` (propose)** calls
   `create_design` directly. Its existing idempotent-insert guarantee — an
   identical retry is a no-op, a same-`design_id`-different-content
   resubmission is refused — is the entire safety net. A spurious proposal is
   exactly what the `Proposed` state already means: nothing has reviewed it
   yet, so there is nothing consequential to protect against a low-stakes
   creation.
4. **`POST .../designs/:design_id/decisions` (record a lifecycle decision —
   accept, reject, defer, retire, supersede, or materialize)** calls
   `record_decision`, now protected by point 1's compare-and-swap. The caller
   submits the `expected_lifecycle_state` it last observed — from the same
   detail view that renders the decision form — and a stale submission is
   refused with `409 Conflict`, matching `recover`'s own `CONFLICT` precedent;
   the operator reloads the design's current state and retries. An unknown
   design returns `404`.
5. **`POST .../designs/:design_id/materializations` (record a materialization
   revision)** calls `record_materialization` unchanged from ADR-0121 decision
   4. Its existing idempotency-key contract — an identical retry is a no-op,
   a changed resubmission under a reused key is refused with `409 Conflict` —
   is the entire safety net, exactly as it was before this ADR.
6. **Every mutation requires a free-text `actor`** (propose, decision) or is
   already actor-bearing (materialization), matching `recover`'s required
   `reason` (ADR-0111 point 4): an auditable attribution, not an authenticated
   identity. This does not claim to know *who* clicked the button — only what
   they said — and the store's own compare-and-swap or idempotency check
   refuses an unsafe concurrent write regardless of what any caller claims.
7. **What remains deferred is unchanged from ADR-0121 decision 7.**
   Legality-of-transition *policy* (for example, whether `Rejected` may return
   to `Proposed`) is still not enforced — this ADR closes the *race*, not the
   state machine. True principal authorization, separation of duties, and any
   typed enrolled-node directive needed to affect a Local repository's own
   Design Board all still wait on ADR-0098 decision 4's OIDC principal, the
   same way ADR-0111 left `delegate`, `release`, and `renew` deferred. This ADR
   does not reopen that deferral — it identifies three specific mutations
   whose safety does not depend on it, the same test ADR-0111 applied to
   `recover` alone among four claim mutations.

## Consequences

- The Design Board becomes what ADR-0105 decision 5 already asked every
  Bridge surface to be: an operator can propose, decide, and materialize from
  the browser, not only read.
- `DesignStore::record_decision`'s new `expected_lifecycle_state` field is a
  breaking change to its own request struct; every existing caller (the
  store's own tests) was updated in the same change, and two new tests cover
  the conflict and unknown-design paths directly.
- Bridge's Design mutation surface remains as narrow as its claim-recovery
  precedent: three specific actions, each independently justified, not a
  blanket "Design is now fully mutable" policy. A future change that wants to
  add more (for example, editing a design's title or summary after creation)
  needs its own equally specific justification, not an appeal to this ADR.
- `ackplane-bridge` gains a direct `tokio-postgres` dependency (previously
  reachable only transitively through `ackplane-server`) so its error mapping
  can distinguish a foreign-key violation (a bad caller-supplied
  Constitution/Work/Evidence reference — `400`) from a genuine server fault
  (`500`), reusing the same SQLSTATE-sniffing idiom already established in
  `ackplane-server`'s own `migration_lock`/`projection` modules.

## Rejected alternatives

**Wait for OIDC before exposing any Design mutation.** Rejected for the same
reason ADR-0111 rejected waiting on OIDC before exposing `recover`: these
three mutations are safe today because the safety comes from the store, not
the caller, and blocking on infrastructure not yet on any critical path would
leave the Design Board a read-only viewer indefinitely for no additional
protection.

**Expose `record_decision` unconditionally, matching its pre-existing store
contract.** Rejected: unlike `recover`'s one-way expiry check, a
`lifecycle_state` can move in either direction, so an unconditional write is a
genuine, silent two-writer race — exactly what ADR-0121 decision 7 meant by
"version compare-and-swap," and exactly the property `record_decision` lacked
until this ADR added it.

**Enforce full lifecycle-transition legality (a state machine) in the same
change.** Rejected as a separate, larger policy decision from the
race-safety property this ADR actually needs: closing the race does not
require deciding which transitions are meaningful, and conflating the two
would make this change larger and slower to land than the concurrency defect
it exists to fix.
