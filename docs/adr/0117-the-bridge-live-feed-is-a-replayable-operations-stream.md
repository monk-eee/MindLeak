# ADR-0117: The Bridge live feed is a replayable operations stream

- Status: Accepted
- Date: 2026-08-21
- Deciders: MindLeak maintainers
- Accepted: 2026-08-21 by the repository owner, authorized directly in session
   — attributed human adoption after review.
- Depends on: [ADR-0086](0086-postgresql-is-the-ackplane-ledger-and-arbiter.md)
  (authoritative ledger and projections),
  [ADR-0095](0095-the-bridge-uses-an-authenticated-projection-api.md)
  (authenticated Bridge API), [ADR-0105](0105-bridge-is-the-server-version-of-the-vsix.md)
  (Bridge as the human operations surface)
- Refines: [ADR-0083](0083-grpc-is-the-ackplane-node-protocol.md) decision 9
  (the browser transport is a separate decision)
- Related: [ADR-0106](0106-ackplane-closes-the-agentic-operating-loop.md)
  (the closed operating loop),
  [ADR-0107](0107-registered-agents-accept-authenticated-control-directives.md)
  (directive delivery and receipts),
  [ADR-0112](0112-bridge-list-reads-are-paginated-sortable-and-filterable.md)
  (bounded list snapshots)

## Context

The Bridge must let a person operate a large, distributed fleet. A static page
that periodically re-fetches tables cannot show whether a worker is still
connected, a lease is about to expire, a control directive was refused, a
projection is lagging, a context packet was superseded, or a human decision is
waiting. Aggressive background polling does not solve this: it creates
unbounded database pressure, wakes hidden tabs, races list pagination, and
still loses the explanation of what changed between snapshots.

Ackplane already has durable records and projection freshness semantics. The
browser needs a narrow, authenticated delivery projection over those facts. It
must be real-time enough for operations, resumable across normal browser
reconnects, bounded for a fleet with many viewers, and explicit whenever its
view is stale or requires a fresh snapshot. It must not become a second
authority, a raw database changefeed, or a browser-to-agent control channel.

## Decision

**Bridge uses an authenticated snapshot-plus-server-sent-events protocol for
live operations state. The stream is a bounded, replayable delivery projection
over authoritative Ackplane records, never a control or authority channel.**

1. **A view starts with an authenticated bounded snapshot.** Existing
   tenant-scoped REST reads provide the initial Fleet, Agents, Work, Context,
   Knowledge, Evidence, Governance, or Telemetry data, including resource
   versions, source ledger position, projection position/time, and freshness.
   The snapshot returns a live-stream cursor for the caller's authorized scope.
   A page renders a truthful snapshot before it opens a live connection; an
   absent stream never makes an older snapshot look current.

2. **Visible Bridge views use one authenticated Server-Sent Events stream.**
   `GET /api/v1/live` is a same-origin, principal-scoped, tenant-scoped SSE
   endpoint. It supports a resume cursor through `Last-Event-ID` and an
   explicit cursor query parameter for clients that cannot set the header. SSE
   provides server-to-browser delivery, standard reconnection behavior, and
   ordered event identifiers without adding a browser-to-server command socket.
   Browser commands remain separately authorized HTTP or gRPC-Web/API calls.

3. **Every event is an envelope with an opaque per-tenant cursor.** An event
   includes cursor, event kind/version, emitted time, resource type/id, compact
   changed-state summary, relevant authority ledger references, projection
   freshness, and a resource version or snapshot-reload hint. The cursor is
   allocated transactionally with the authoritative change or its published
   projection update. It is a durable delivery sequence, not a claim, task,
   or ledger position and never replaces the source record's own identity.

4. **The stream carries named operational facts, not arbitrary payloads.**
   Initial event families cover repository/node/session presence; task and lease
   transitions; waits and human decision requests; context-packet availability
   or invalidation; knowledge lifecycle changes; evidence/conformance state;
   directive delivery/receipts; projection freshness; and typed telemetry
   health. An event contains references, status, counters, timing, and bounded
   display metadata. It never carries source files, terminal output, secrets,
   credentials, raw prompt/context bodies, or a generic JSON tunnel.

5. **Resumption is bounded and gaps fail visibly.** Ackplane retains live
   delivery events for a policy-defined window. If a cursor is valid, the
   server replays later events in cursor order before returning to live
   delivery. If a cursor is unknown, outside retention, unauthorized under
   current policy, or crosses an unrecoverable projection reset, the server
   sends a typed `resync_required` event and closes the stream. The client
   fetches a new snapshot; it never guesses what it missed or reconstructs a
   current state from a partial event history.

6. **Authorization and tenancy are evaluated before connection and during
   delivery.** A stream sees only resources the authenticated principal may
   read in its tenant/repository/project scope. Permission changes, session
   expiry, tenant changes, or authorization failure terminate the stream with a
   typed reason. Event cursors are scoped server-side; a client cannot use a
   guessed cursor to replay another tenant's activity. The server performs no
   browser-specific trust shortcut based on a development tenant token outside
   the existing loopback profile.

7. **Backpressure is a server responsibility.** Each client has bounded
   buffered events and a bounded write deadline. A slow or hidden client is
   coalesced to resource invalidations where safe or receives
   `resync_required`; it never accumulates an unbounded per-client queue.
   Projection workers and PostgreSQL notifications may wake the delivery
   service, but the durable event sequence is the replay source. Losing a
   notification delays a wake-up, never loses an operations event.

8. **The Bridge treats freshness as first-class UI state.** The shell displays
   connected, reconnecting, stale, resyncing, and disconnected state along with
   the newest cursor and source/projection freshness. It opens a stream only
   while a relevant view is visible, reconnects with bounded exponential
   backoff, and stops hidden-page polling. Live updates preserve filters,
   pagination, focus, and accessible announcement semantics; a changed record
   may invalidate a page rather than silently reshuffle it under a reader.

9. **The feed is observably safe.** Ackplane records connection count,
   authorization refusals, replay lag, replay/resync rate, queue drops,
   backpressure, delivery latency, latest emitted cursor, and client-visible
   projection lag. Bridge can therefore show whether a view is fresh without
   claiming it received facts that the client has not actually applied.

10. **Control remains a separate receipted path.** A live event may tell a
    person that a decision needs attention or that a directive was applied. It
    cannot approve, pause, resume, terminate, create work, alter policy, or
    issue an agent command. Those actions use their own authorization,
    idempotency, confirmation, and receipt contracts; the feed merely carries
    their resulting state for observation.

## Consequences

- Bridge becomes a real-time operations surface without giving browser clients
  database access, raw gRPC node access, or an unbounded polling workload.
- The service needs a durable tenant-scoped event-delivery projection, SSE
  handler, cursor retention/replay, authorization checks, live-health metrics,
  and focused browser behavior tests.
- A human can see a fleet pulse, agents needing attention, lease risk,
  directives, context changes, and governance escalations as they occur, with
  a clear source/freshness chain behind every visible change.
- A missed event becomes a visible resynchronization state rather than a
  silently misleading dashboard. This is essential for a control room that a
  person may use to make consequential decisions.

## Rejected alternatives

**Poll every endpoint at a fixed short interval.** Rejected because it wastes
database and browser resources, wakes hidden pages, races mutable list views,
and cannot provide a precise replay/gap contract.

**Use a raw PostgreSQL changefeed in the browser.** Rejected because it exposes
storage schema and credentials, bypasses authorization/projection semantics,
and makes a database implementation detail the public UI contract.

**Use WebSockets for both live state and agent control.** Rejected because
browser-to-agent control is prohibited by ADR-0107, while SSE already provides
the browser's required one-way operations feed with simpler reconnect and
deployment semantics.

**Treat event delivery as authoritative command success.** Rejected because an
event can report that a directive was queued or a worker changed state; only a
typed command receipt establishes that the requested effect occurred.
