# ADR-0083: gRPC is the Ackplane node protocol

- Status: Accepted
- Date: 2026-08-10
- Deciders: MindLeak maintainers
- Accepted: 2026-08-10 by the repository owner — attributed human adoption after
  review.
- Depends on: [ADR-0082](0082-ackplane-is-a-standalone-federation-service.md)
  (standalone federation boundary)
- Related: [ADR-0010](0010-observability-and-resilience.md) (bounded network
  behaviour), [ADR-0030](0030-discrete-per-agent-identity.md) and
  [ADR-0054](0054-identity-is-the-session-not-the-process.md) (logical session
  identity), [ADR-0059](0059-the-tool-surface-is-a-vocabulary.md) (MCP tool
  vocabulary)

## Context

The Ackplane boundary needs a protocol between repository nodes and a remote,
multi-tenant service. The traffic is not ordinary CRUD. A node publishes an
ordered stream of evidence and lifecycle records, receives an acknowledgement
for durable progress, reconnects from that position after interruption, and
needs live notices without polling every repository.

HTTP with hand-written JSON endpoints could carry those messages, but it would
leave compatibility, streaming, error shapes, and client generation as local
conventions. A WebSocket would provide duplex transport while leaving all of
those contracts to us. Direct database or broker access would couple every node
to server infrastructure and make the storage implementation part of the
product protocol.

MCP already serves a different boundary well: an agent asks a local tool to
perform a repository operation. Treating remote federation as another MCP
server would expose an inventory of commands where the protocol needs an
ordered, resumable stream. Conversely, replacing local MCP with a network RPC
API would add Ackplane availability to the deterministic local path.

## Decision

1. **Repository nodes speak versioned Protobuf over gRPC to Ackplane.** The
   first package is `mindleak.ackplane.v1`. The Rust implementation uses
   `tonic` and `prost`; generated types are the wire contract and contain no
   storage-layer structs.

2. **The core synchronization API is one bidirectional stream.** Conceptually:

   ```protobuf
   service NodeSyncService {
     rpc Synchronize(stream NodeFrame) returns (stream AckplaneFrame);
   }
   ```

   A node opens with repository identity, capabilities, and its last accepted
   position. It then sends bounded event batches and heartbeats. Ackplane
   returns per-batch receipts, explicit rejections, flow-control hints, and
   versioned notices. Reconnection resumes from the last receipt; it does not
   infer progress from connection lifetime.

3. **The stream is not a remote shell.** An `AckplaneFrame` has a closed set of
   coordination messages. It cannot carry arbitrary commands, source patches,
   terminal input, or an untyped "execute" payload. A future server-initiated
   mutation needs its own reviewed command contract and local authorisation.

4. **MCP remains the agent-facing local protocol.** Existing MindLeak and
   Lodestar tools keep their semantics and stdio transport. A repository node
   translates committed domain events and receipts, not MCP requests, into the
   Ackplane protocol. There is no one-for-one remote copy of every MCP tool.

5. **Compatibility is additive within `v1`.** Field numbers are never reused;
   removed fields remain reserved; enums include an unspecified value; readers
   tolerate unknown fields; and every connection exchanges capabilities before
   optional messages are sent. A breaking semantic change creates a new
   package version rather than a flag that changes the meaning of an old field.

6. **Domain failure is typed separately from transport failure.** gRPC status
   reports authentication, availability, deadline, and malformed-message
   failures. Accepted or rejected domain records return a stable reason code,
   offending record identity, and retryability. Human-readable text is
   diagnostic and is never the branch condition for a client.

7. **Retries are bounded and idempotent.** The durable deduplication key is
  `(tenant_id, repository_id, producer_id, producer_sequence)`. Repeating that
  key with the same envelope digest returns the original ledger position and a
  `duplicate` receipt without appending another record. Repeating it with a
  different digest is a non-retryable conflict. Producer sequences are positive
  63-bit values and never wrap; an exhausted sequence requires a new producer
  identity. Uniqueness is enforced transactionally in ADR-0086's authoritative
  store, not in one server instance's memory. Clients retry only retryable
  operations with exponential backoff and jitter. Deadlines, keepalive,
  maximum message size, and in-flight batch limits are configured explicitly;
  an interrupted stream cannot grow an unbounded local queue in memory.

8. **TLS is mandatory outside loopback.** Transport encryption is part of this
   protocol decision. Workload authentication, tenant binding, signatures, and
   evidence trust are intentionally delegated to the following trust-boundary
   decision; TLS alone must not be presented as proof of who produced a record.

9. **The browser is not forced through native gRPC.** The Bridge consumes
  a same-origin web API or a reviewed gRPC-Web gateway over Ackplane read
  models. Either route uses ADR-0084's principal and tenant authorisation,
  denies cross-origin access by default, and cannot add authority unavailable
  to the underlying service principal. Browser authentication, caching, and
  live updates have different constraints from node synchronization and do not
  justify weakening the node protocol. The exact browser transport requires a
  separate decision before implementation.

10. **The protocol is observable without logging evidence bodies.** Both ends
    record method, outcome, reason code, latency, batch count, byte count,
    retry count, and accepted position. Payloads, credentials, source content,
    and terminal output are excluded from transport logs.

## Consequences

- Live fleet activity and acknowledgement use one typed duplex connection
  instead of separate polling, upload, and notification mechanisms.
- Protobuf gives cross-language clients and compatibility checks before another
  implementation has to reverse-engineer Rust JSON.
- gRPC adds build tooling, generated code, HTTP/2 proxy requirements, and more
  operational complexity than a small REST API. The streaming and compatibility
  requirements are the reason to pay that cost.
- The Bridge can evolve its web-facing query contract without changing the
  repository synchronization protocol.
- The local MCP servers remain network-independent. Losing Ackplane
  connectivity does not turn local tool calls into gRPC failures.
- Protocol conformance needs fixture messages from every supported package
  version plus reconnect, duplicate, out-of-order, deadline, and backpressure
  tests.

## Rejected alternatives

**JSON REST plus polling.** Rejected for node synchronization because polling
multiplies stale reads across a fleet and hand-maintained JSON contracts provide
weaker compatibility guarantees. HTTP/JSON remains reasonable for the browser
query surface.

**WebSockets with custom messages.** Rejected because duplex bytes are the easy
part. We would still need to invent schemas, generated clients, status details,
deadlines, compatibility rules, and flow control.

**Remote MCP as the federation protocol.** Rejected because MCP exposes tools
to an agent; it does not define a durable ordered replication stream. Keeping
the vocabularies separate also prevents a remote service from entering local
ingestion by accident.

**Let nodes publish directly to Kafka, NATS, or a database.** Rejected because
it exposes Ackplane infrastructure as the product API and distributes tenant,
compatibility, and validation logic into every node. Ackplane may use a broker
internally without making it part of the node contract.

**Use gRPC for every surface, including the Bridge.** Rejected as a goal in
itself. The node stream earns gRPC; the browser should choose its transport from
browser constraints rather than architectural symmetry.
