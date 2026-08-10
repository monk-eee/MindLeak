# ADR-0086: PostgreSQL is the Ackplane ledger and arbiter

- Status: Accepted
- Date: 2026-08-10
- Deciders: MindLeak maintainers
- Accepted: 2026-08-10 by the repository owner — attributed human adoption after
  review.
- Depends on: [ADR-0082](0082-backplane-is-a-standalone-federation-service.md)
  (standalone federation and sole authority),
  [ADR-0083](0083-grpc-is-the-backplane-node-protocol.md) (idempotent node
  protocol), [ADR-0084](0084-backplane-evidence-has-explicit-trust.md)
  (immutable receipts)
- Refined by: [ADR-0087](0087-the-backplane-graph-is-a-projection-not-an-authority.md)
  (graph and vector projections),
  [ADR-0088](0088-the-backplane-runs-in-containers-the-planes-do-not.md)
  (container topology and durability profile)
- Related: [ADR-0045](0045-a-fleet-is-a-distributed-system.md) (one arbiter per
  shared resource), [ADR-0064](0064-the-log-is-the-ledger.md) (event authority
  and deterministic projections)

## Context

The Ackplane decisions require one arbiter for federated claims, an append-only
ledger, transactional duplicate detection, immutable acceptance receipts, and
read models for the Bridge. Saying "standalone service" does not identify
where that authority lives when several server instances handle connections.

Keeping authority in an application process would recreate the split registry
that ADR-0045 diagnosed. An embedded database would make horizontal service
instances unsafe or force one pet server to own every stream. Active-active
multi-primary storage would move conflict resolution into the most sensitive
records in the system: claim ownership and evidence acceptance.

A broker is useful for fan-out but not sufficient as the whole store. Claims
need compare-and-swap state, tenant-scoped queries, uniqueness constraints, and
auditable transactions spanning an accepted envelope, its stream position, and
its receipt. Building those semantics around a log broker would add a second
database for projections and then require a decision about which one wins.

## Decision

1. **PostgreSQL is Ackplane's sole durable write authority.** All accepted
   envelopes, Ackplane-originated domain events, receipts, enrolments, claims,
   leases, and projection checkpoints are committed there. An application
   instance has no authoritative local state and cannot accept work while the
   database is unavailable.

2. **Ackplane server instances are stateless and horizontally replaceable.**
   Any healthy instance may terminate a gRPC stream or web request. It performs
   each domain mutation in PostgreSQL and returns success only after commit.
   Process memory may cache immutable metadata, but a cache hit never grants a
   claim, authenticates a principal, or acknowledges an envelope.

3. **A deployment has exactly one logical writable PostgreSQL primary.** A
   single-node database is supported for development and small self-hosted
   installations. Production HA uses a primary with synchronously replicated
   standbys and a failover system that fences the old primary before promoting
   another. Active-active multi-primary, disconnected writes, and application-
   level conflict merging are unsupported. If write authority is uncertain,
   Ackplane fails closed and repository nodes retain their durable outboxes.

4. **The ledger is partitioned into ordered logical streams.** Repository
   records use a repository stream; tenant administration uses a tenant stream.
   A `stream_heads` row is locked in the append transaction to allocate the next
   positive 63-bit position. This serialises records within one stream without
   serialising unrelated repositories. A stream position is assigned only in
   the transaction that writes the record and its receipt.

5. **Idempotency is enforced by database constraints.** The node deduplication
   key from ADR-0083 is unique across the authoritative store. In one
   transaction, Ackplane locks the stream head, checks the producer sequence,
   verifies the digest, appends at most one record, creates the signed receipt,
   and advances the head. A same-key/same-digest retry reads the stored receipt;
   a same-key/different-digest retry cannot pass the uniqueness and digest
   checks. No application-instance memory participates in that decision.

6. **Accepted records are immutable.** Canonical Protobuf bytes and their digest
   are stored with indexed typed metadata, not expanded into a mutable JSON
   document that becomes a second wire format. Corrections append a superseding
   record. Database roles deny ordinary application code `UPDATE` and `DELETE`
   on ledger records and receipts; migrations cannot rewrite their semantic
   content.

7. **The event log is authoritative; current state is a deterministic
   projection.** Coordination projections needed by command handlers are updated
   synchronously in the same transaction as their events and carry the stream
   version they represent. Claims use row locks plus that version for compare-
   and-swap. A missing or inconsistent projection makes the command fail and
   triggers repair; handlers never guess state from a partial event scan or
   write around the projection. Rebuilding into a scratch projection and
   diffing it against current state is a required invariant test.

8. **The Bridge reads versioned projections with visible freshness.**
   Assurance queues, timelines, trends, and fleet summaries may be projected
   asynchronously. Every response carries the source stream position and
   projection time. Read replicas are permitted only for read-only queries and
   must expose their replay position; claim, approval, waiver, enrolment, and
   receipt operations always use the primary. A stale replica can display an
   older view, never authorise a mutation or label it current.

9. **No message broker is required in v1.** Projection workers read the durable
   ledger through checkpoints. PostgreSQL notifications may reduce wake-up
   latency but are hints only; losing one cannot lose a record. Kafka, NATS, or
   another broker may be added for scale after measurement, but it remains a
   delivery optimisation and never becomes a second authority.

10. **Tenant scope is structural and enforced twice.** Tenant and repository
    ids participate in durable keys, foreign keys, uniqueness constraints, and
    every query. PostgreSQL row-level security provides defence in depth behind
    application authorisation. Tests execute queries under two tenants and fail
    on any cross-tenant row, receipt, stream position, or timing-derived result.

11. **A receipt is stored before it is returned.** The receipt is constructed
    and signed after its stream position is allocated, inserted in the same
    transaction as the accepted record, and sent only after commit. If the
    process or connection fails after commit, another instance returns the
    identical stored receipt on retry. If signing fails, the transaction rolls
    back and no acceptance is reported.

12. **Durability claims match deployment reality.** PostgreSQL uses durable WAL
    commits. A production deployment may report `quorum_durable` only when an
    acknowledged commit is synchronously replicated to the configured failure
    domain; a single-node deployment reports `single_node`. Backups are
    encrypted and point-in-time recovery is tested. Ackplane never labels an
    asynchronously replicated acknowledgement as zero-loss assurance.

13. **Schema evolution preserves the ledger.** Migrations use expand, backfill,
    verify, then contract. New projection versions are built beside old ones and
    switch only after their checkpoints and deterministic diff pass. Protobuf
    compatibility follows ADR-0083. Retention, tenant export and deletion, and
    disaster migration between Ackplane deployments require separate decisions
    before production because they can end or move an append-only trust domain.

## Consequences

- Several Ackplane instances can serve clients without becoming several
  arbiters. PostgreSQL transactions settle ordering, uniqueness, leases, and
  claim ownership.
- The write path stays deliberately conservative. A database outage blocks new
  federated authority instead of accepting records into an instance-local queue
  whose place in the ledger is unknown.
- Ordering is per repository or tenant stream, not one global bottleneck. The UI
  can combine streams while preserving each record's source position.
- PostgreSQL becomes critical infrastructure. Capacity planning, connection
  pooling, WAL monitoring, backup restore drills, key management, migrations,
  and fenced failover are product responsibilities.
- The initial system avoids Kafka and a distributed projection architecture.
  That is a feature until measured throughput proves otherwise.
- A managed PostgreSQL service is compatible only when it exposes the required
  transaction, synchronous durability, backup, role, and row-security semantics.
  The product contract names capabilities rather than one cloud vendor.

## Rejected alternatives

**SQLite on the Ackplane host.** Rejected because it would bind authority to
one application host and make horizontal failover a filesystem problem. SQLite
remains the right repository-local store; the remote service has a different
concurrency and availability boundary.

**An in-memory leader with replicated followers.** Rejected because it would
require MindLeak to implement consensus, durable election, log replication, and
snapshot recovery before it could safely claim one arbiter.

**Active-active PostgreSQL or conflict-free multi-primary writes.** Rejected
because a claim cannot be merged after two owners both won. Coordination needs
one fenced writer, not eventual reconciliation.

**Kafka as the source of truth.** Rejected for v1 because claim CAS, tenant
queries, receipt lookup, and projections would still require a database. Two
authorities would then need a distributed transaction or an arbitrary winner.

**A database per tenant.** Rejected as the default because migration, pooling,
and fleet-wide operations would scale with tenant count before isolation needs
justify it. Structural tenant keys plus row-level security are the initial
boundary; dedicated deployments remain possible for customers requiring them.

**A generic event-store product.** Rejected initially because PostgreSQL already
provides the transactions, constraints, indexing, backup ecosystem, and
operational maturity this bounded domain needs. Adoption can be reconsidered
when measured stream volume exceeds that design, not for architectural fashion.
