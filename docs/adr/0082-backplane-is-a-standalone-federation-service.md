# ADR-0082: Ackplane is a standalone federation service

- Status: Accepted
- Date: 2026-08-10
- Deciders: MindLeak maintainers
- Accepted: 2026-08-10 by the repository owner — attributed human adoption after
  review.
- Related: [ADR-0002](0002-sqlite-decay-over-vector-llm.md) (local SQLite
  memory), [ADR-0004](0004-intent-plane-spec-brain.md) (separate memory and
  intent planes), [ADR-0026](0026-constitutional-policy-over-mechanistic-ratchets.md)
  (local constitutional authority), [ADR-0045](0045-a-fleet-is-a-distributed-system.md)
  (one arbiter per shared resource), [ADR-0064](0064-the-log-is-the-ledger.md)
  (append-only lifecycle history)

## Context

MindLeak and Lodestar are repository-local services. They speak MCP over stdio,
store state under one repository identity, and remain useful without a network.
That is the right boundary for one repository and one machine. It is not a
credible boundary for the Bridge across many repositories, machines, and
teams.

An organisation-wide UI needs a durable answer to questions no individual
clone can settle: which agents are active, who owns a cross-machine claim,
which constitution version governed a result, which waivers are in force, and
how fresh the evidence is. Pointing a web UI at local SQLite files would expose
implementation schemas as an API, create one partial view per clone, and make
the browser responsible for reconciliation. Exposing the existing unauthenticated
stdio servers on a network would cross the security boundary that their local
design deliberately avoids.

Replacing the local planes with a mandatory cloud service would solve the
aggregation problem by discarding MindLeak's offline and repository-local
properties. Replicating their databases would be worse: local graph decay and
repository governance have different ownership and retention rules, and two
writable replicas would create the split authority ADR-0045 forbids.

## Decision

1. **Ackplane is a separately deployable service.** It is not a mode
   of `mindleak-mcp`, `lodestar-mcp`, the VS Code extension, or the Bridge.
   Those remain clients or repository nodes. An Ackplane deployment owns an
   organisation boundary and may contain many repository identities.

2. **Ackplane federates domain records; it does not mount or replicate local
   databases.** Repository nodes publish versioned events, evidence envelopes,
   constitution identities, and conformance receipts through a supported API.
   SQLite schemas, graph rows, and local filesystem paths are never the wire
   contract. The Bridge reads Ackplane projections and never queries a
   plane's database directly.

3. **Authority is partitioned, never dual-written.** A repository has one
  explicit coordination mode: `local` or `federated`. In local mode, the
  current repository stores remain the arbiters they are today. In federated
  mode, Ackplane is the sole arbiter for the enrolled repository's shared task
  namespace, cross-machine sessions, and claims. The mode is not selected per
  call and never falls back according to reachability. Repository-local memory,
  structural reconciliation, and deterministic conformance remain local.
  Ackplane records their exported receipts; it does not silently recompute or
  overwrite them.

4. **Policy distribution cannot become policy activation.** Ackplane may
  distribute immutable policy packs and amendment proposals. A repository
  reviews every clause through the existing pack-disposition path; adoption
  into an active constitution then follows ADR-0043's attributed amendment
  flow. Ackplane has no shorter activation path. An organisation administrator
  cannot turn a proposal into active repository law merely by changing an
  Ackplane setting.

5. **Repository connections are outbound and interruption-tolerant.** A node
   maintains a durable local outbox, publishes idempotently, and resumes from
   the last acknowledged record. Ackplane assigns an ingestion position and
   projects accepted records for query. It never reaches through the connection
   to inspect source code or arbitrary local files.

6. **Disconnection is visible state, not silent success.** Local memory,
   ingestion, advice, and repository conformance continue when Ackplane is
   unavailable. An existing federated claim remains authoritative only until
   its fixed Ackplane-recorded lease expiry; a disconnected node cannot renew
   it or acquire another. Work performed without a live federated claim is
  labelled `uncoordinated` because it carries no valid claim-and-lease receipt,
  and falls outside a claim evidence window. On reconnect, the node publishes
  those records as context, runs the normal overlap check, and acquires a fresh
  claim before continuing. It then produces fresh validation and publication
  evidence inside the new window; offline evidence is never moved into it. If
  another owner now holds the scope, the work stops for review rather than
  taking or merging automatically. The Bridge reports the last accepted
  position, age, lease horizon, and uncoordinated records for every repository.

7. **Ackplane stores an append-only organisation ledger and derived read
   models.** Assurance queues, timelines, fleet status, and repository summaries
   are projections. Rebuilding a projection cannot alter the accepted records
   or the historical verdict under which a receipt was produced.

8. **The Bridge is an unprivileged Ackplane client.** The server may host
  its static assets for simple deployment, but the UI has no direct database
  access and no authority unavailable through the same authenticated service
  APIs used by other clients.

9. **Ackplane v1 aggregates repositories but does not coordinate across their
  task graphs.** A claim, goal, constitution, waiver, and conformance receipt
  belongs to one repository identity. Cross-repository objectives or blocking
  edges require a later decision about authority and evidence; a dashboard
  grouping is not such a decision.

This ADR, ADR-0083 (node transport), ADR-0084 (remote identity and evidence
trust), ADR-0085 (node enrolment), and ADR-0086 (authoritative storage and HA)
form one federation design. They are separated so each irreversible choice can
be accepted or replaced independently. Browser transport and tenant
retention/export/deletion still require their own decisions before production.

## Consequences

- The open-source local product remains useful with no account, network, or
  Ackplane deployment.
- A connected organisation gains one arbiter for cross-machine coordination
  and one query surface for compliance operations.
- Enrolment is a real mode transition. Claim calls in an enrolled repository
  cannot sometimes write locally and sometimes write remotely according to
  reachability; that would recreate split authority.
- A partition can interrupt coordination and eventually expire a lease, but it
  cannot create a second arbiter. The claim window already models that boundary;
  offline work after expiry is honest context with no authorising claim. The
  recovery path reuses overlap, claim, validation, and publication rather than
  inventing a retroactive-claim tool.
- Ackplane can aggregate exact receipts without pretending every uploaded
  observation has the same trust or that a control pass is a conformance
  verdict.
- The service adds operations that local MindLeak deliberately avoids:
  tenancy, authentication, authorisation, schema compatibility, backups,
  retention, availability, and migration.
- Raw source, terminal output, and the full decay graph stay local by default.
  A later feature that uploads any of them needs an explicit data contract and
  retention policy.

## Rejected alternatives

**Expose both MCP servers remotely.** Rejected because MCP is the agent-facing
tool vocabulary, not an authenticated multi-tenant replication protocol. It
would also turn a stdio-only local trust assumption into an internet boundary.

**Use a shared SQLite database on a network filesystem.** Rejected because it
couples availability to a filesystem, exposes storage schemas as protocol, and
does not provide a sound multi-tenant or cross-region concurrency model.

**Replicate `graph.db` and `spec.db` into the service.** Rejected because those
databases have different semantics and contain local implementation state.
Database replication would make conflict resolution accidental rather than a
domain decision.

**Make the VS Code extension the fleet coordinator.** Rejected because an
editor process is neither durable nor universal. Headless agents, CI, and other
editors must observe the same organisation state.

**Replace local mode with a hosted control plane.** Rejected because network
availability must not enter deterministic ingestion or repository conformance,
and because local-first operation is part of the product contract rather than
a temporary bootstrap mode.
