# ADR-0096: Ackplane arbitrates federated claims through leased delegation

- Status: Accepted
- Date: 2026-08-17
- Deciders: MindLeak maintainers
- Depends on: [ADR-0082](0082-ackplane-is-a-standalone-federation-service.md)
  (Ackplane is a standalone federation service), [ADR-0045](0045-a-fleet-is-a-distributed-system.md)
  (one arbiter per shared resource)
- Refines: [ADR-0024](0024-preflight-overlap-detection.md) (pre-flight overlap
  detection), [ADR-0048](0048-a-lapsed-lease-holes-the-window-it-does-not-move-it.md)
  (a lapsed lease holes the window, it does not move it)
- Related: [ADR-0020](0020-task-lifecycle-states.md) (task lifecycle states),
  [ADR-0064](0064-the-log-is-the-ledger.md) (the log is the ledger)

## Context

ADR-0082 decision 3 says a federated repository has one arbiter for its "shared
task namespace, cross-machine sessions, and claims" — Ackplane — and that the
mode is never selected per call according to reachability. It does not say what
arbitrates a claim actually *is*: whether a claim call is proxied to Ackplane
synchronously, mirrored to it for aggregation while a local store keeps
deciding, or delegated to it as a leased grant a node then exercises locally
for a bounded time.

That question was left open on purpose (ADR-0082's own words: "Cross-repository
objectives or blocking edges require a later decision about authority and
evidence"), and it has stayed open. `gaps.d/no-claim-is-arbitrated-through-ackplane.md`
names the consequence: `NodeSyncService.Synchronize` now accepts a `Hello`
(shipped under task:d4d683d3ee60) and node enrolment now has a real authority
contract (shipped under task:c265276db1ba), so a repository node can reach
Ackplane and prove who it is. Neither of those things routes a single
`task_claim` call anywhere. `crates/lodestar-core/src/store/coordination/claim.rs`
is the only place a claim is ever decided today: a guarded compare-and-swap on
one row of the repository-local `tasks` table. A client built against that gap
would have nothing to call — building one now would either silently keep
arbitrating locally (the false-authority failure ADR-0082 decision 3 exists to
refuse) or invent wire semantics ad hoc, which is exactly the trap the gap
fragment warns against: "settled inside a gap fragment by whoever noticed the
hole" rather than as a reviewed decision.

This ADR is that decision. It fixes the shape of the adapter — what moves to
Ackplane, what a repository node keeps doing locally, and what "disconnected"
means for a claim already in hand — without yet specifying the wire messages,
matching how ADR-0082 itself was split from ADR-0083 through ADR-0086 so each
irreversible choice could be accepted independently.

## Decision

**Ackplane becomes the sole compare-and-swap authority for claim/lease state on
a federated repository's tasks. It is delegated to, not proxied through or
mirrored to.**

1. **Only ownership of a task moves. Its content stays local.** ADR-0082
   decision 3 already draws this line for evidence and conformance:
   "Repository-local memory, structural reconciliation, and deterministic
   conformance remain local." This ADR extends the same line to claims. Goal
   definitions, acceptance text, the task thread, evidence bundles, and
   conformance verdicts are never sent to Ackplane by this design. Only the
   claim primitive — who holds a task, since when, until when, under what
   declared scope — becomes an Ackplane-arbitrated resource. Federating more
   than that is a different, larger decision this ADR does not make.

2. **A claim is a leased grant, not a synchronous proxy and not a mirrored
   write.** A **proxy** (forward every `task_claim`/`renew`/`release` call to
   Ackplane and treat its answer as the only truth) makes every board read and
   every renewal a network round trip, which contradicts ADR-0082 decision 6's
   requirement that local memory and advice keep working while Ackplane is
   unreachable. A **mirror** (keep deciding locally, echo the result to
   Ackplane for aggregation) makes the local store the real arbiter and
   Ackplane a dashboard — the exact split-authority shape ADR-0045 clause 2
   forbids and ADR-0082 decision 3 refuses by name. The remaining shape is
   **delegation**: Ackplane performs the actual compare-and-swap and hands back
   a lease with a fixed, Ackplane-recorded expiry (ADR-0082 decision 6 already
   uses this language); the node then acts on that lease locally, at zero
   further round-trip cost, until it renews or the lease runs out.

3. **The compare-and-swap Ackplane runs is the one already accepted, not a
   new one.** `claim_task_with_partial_scope` already encodes the rules this
   repository has hardened: open, self-reclaim, an expired lease, or a parked
   task past its grace may be claimed; a same-owner re-claim preserves
   `claim_started_at` and the declared branch rather than resetting them
   (ADR-0048); a losing claimant never writes scope. Ackplane's claim
   arbitration reimplements this same state machine as the authority for a
   federated repository — it does not get to define a second, looser one. A
   local claim table remains the reference implementation; Ackplane's is
   required to agree with it bit for bit on every case the local test suite
   already covers.

4. **The local store keeps a read-through cache, never a shadow authority.**
   For a federated repository, `lodestar-core`'s task row still holds `owner`,
   `lease_expires_at`, `claim_started_at`, `branch`, `claim_lapses`, and scope —
   but as a cached copy of what Ackplane most recently granted, refreshed on
   every claim/renew/release response, never written from a local decision.
   `board`, `next`, and `scope` keep reading it directly, so those stay instant
   and work offline (ADR-0082 decision 6). `claim`, `renew`, `release`,
   `recover`, `park`, and `answer` — every call that changes who owns
   something — go to Ackplane first and only update the local cache once
   Ackplane accepts them.

5. **Overlap detection follows ownership to where it now lives.** ADR-0024's
   `task_query(view="overlap")` answers from "active claims whose declared
   scope intersects the requested paths/symbols." For a federated repository
   those active claims are Ackplane's, not the local table's, so the overlap
   check queries Ackplane's claim registry. MindLeak's `check_overlap` (the
   decay-aware footprint half of the same pre-flight) is unaffected: it reads
   graph attribution, which this ADR does not touch.

6. **Disconnection holes the lease, it does not extend it.** This restates
   ADR-0082 decision 6 in claim terms, because an implementer building the
   adapter needs it stated as a rule about the CAS, not only as a property of
   the system: a lease already granted stays valid locally until its recorded
   expiry even if Ackplane becomes unreachable, but it cannot be renewed or
   re-acquired without reaching Ackplane. Work done after expiry, or performed
   while disconnected with no live lease, is `uncoordinated` — the same label
   and the same evidence-window exclusion ADR-0082 decision 6 already
   specifies. On reconnect, re-claiming is subject to rule 3 above: a
   same-owner re-claim reopens the existing window rather than resetting it,
   exactly as ADR-0048 requires locally, so a federated lapse is not treated
   more harshly than a local one.

7. **The wire contract is a follow-on decision, not this one.** This ADR fixes
   the pattern — delegated leases, identical CAS semantics, local cache, holed
   not extended. The concrete RPC (request/response shapes for claim, renew,
   release, recover, park, and the overlap query; how it reuses or extends
   `ackplane-protocol`; and how a repository-side adapter selects it only under
   `CoordinationMode::Federated`) is deliberately left to the implementation
   task this ADR unblocks, so that decision can be reviewed and revised on its
   own once someone is holding the actual message shapes.

## Consequences

- `task:727ae37b4f5a` (give the repository an Ackplane client) gains the
  design it was blocked on: a federated `task_claim` call has somewhere real to
  go, and what "resolves" has to mean is now written down rather than left to
  whoever ships the transport.
- Ackplane gains a second arbitrated resource type (claims) alongside the
  ledger and enrolment authority it already has, following the same shape:
  Ackplane holds the authoritative state, a node holds a cache and a lease.
- A federated repository's board reads stay local and instant; only ownership
  changes cost a round trip, and only when one is actually happening.
- Building the adapter now means reimplementing `claim_task_with_partial_scope`'s
  exact CAS rules against Ackplane's storage — a real, non-trivial piece of
  work, and one this ADR deliberately does not shrink by pretending a thinner
  rule would do.
- A federated repository's claim history becomes append-only on Ackplane's
  side by the same reasoning as ADR-0064: the current lease is a projection,
  and rebuilding it must never alter an already-issued grant.

## Rejected alternatives

**Proxy every claim-affecting call to Ackplane and treat local storage as a
pass-through.** Rejected because it makes `board`/`next`/`scope` — read paths
exercised on every tool call — depend on network reachability, directly
contradicting ADR-0082 decision 6's requirement that local memory and advice
keep working while Ackplane is unreachable.

**Mirror local claim decisions to Ackplane for aggregation and dashboards.**
Rejected because the local store would remain the actual arbiter and Ackplane
would only echo it — the split-authority shape ADR-0045 clause 2 forbids
("every shared mutable resource has exactly one arbiter, or it is not
shared") and the false-authority failure ADR-0082 decision 3 refuses by name.

**Replicate the `tasks` table (or all of `spec.db`) into Ackplane and let
either side write it.** Rejected for the same reason ADR-0082 itself rejected
replicating `graph.db`/`spec.db` wholesale: two writable copies of the same
row is the split authority ADR-0045 exists to prevent, and conflict resolution
between them would be accidental rather than a domain decision.

**Design the wire contract in this same ADR.** Rejected because the pattern
decision (delegate, don't proxy or mirror) and the message-shape decision are
separable, and separating them is exactly what let ADR-0082 spawn ADR-0083
through ADR-0086 as independently reviewable choices instead of one
unreviewable one.
