# ADR-0095: The Bridge uses an authenticated projection API

- Status: Accepted
- Date: 2026-08-17
- Deciders: MindLeak maintainers
- Depends on: [ADR-0082](0082-ackplane-is-a-standalone-federation-service.md)
  (federation boundary), [ADR-0083](0083-grpc-is-the-ackplane-node-protocol.md)
  (node protocol and browser boundary), [ADR-0084](0084-ackplane-evidence-has-explicit-trust.md)
  (remote trust), [ADR-0086](0086-postgresql-is-the-ackplane-ledger-and-arbiter.md)
  (ledger and read models), [ADR-0087](0087-the-ackplane-graph-is-a-projection-not-an-authority.md)
  (projection authority), [ADR-0094](0094-the-bridge-preserves-standalone-operation.md)
  (product modes)

## Context

Ackplane's gRPC protocol is for enrolled repository nodes. A browser has
different authentication, caching, same-origin, and deployment constraints.
Letting it call PostgreSQL, local SQLite, or the existing unauthenticated MCP
servers would cross the boundaries those components deliberately maintain.

The Bridge needs a concrete implementation boundary before a UI is built. It
must answer operational questions with data a browser is entitled to see:
which repositories are enrolled, how current their accepted records are, what
their projection freshness is, whether coordination is healthy, and how a
qualified certification status resolves to evidence. A generic dashboard over
raw tables would make these answers ambiguous and lock the UI to storage.

## Decision

1. **The full-server browser surface is a separate `ackplane-bridge` service.**
   It is an unprivileged Ackplane client and is packaged with the full-server
   deployment, never with the standalone VSIX. It may serve its static assets
   itself or run beside a separately hosted static bundle, but both are
   same-origin from the browser's perspective.

2. **The browser calls only a versioned, authenticated read API.** The initial
   API prefix is `/api/v1`. It is implemented by `ackplane-bridge`; no browser
   receives database credentials, connects to PostgreSQL, invokes MCP, or
   calls native gRPC node services. The Bridge service obtains projection data
   through a narrow read-store interface, not through raw SQL in route handlers.

3. **Every data request is tenant-scoped and principal-scoped.** The service
   validates an operator-configured OpenID Connect identity before serving
   tenant data, resolves the principal's tenant and repository permissions, and
   rejects cross-tenant access by default. A developer profile may bind only to
   loopback and use an explicit development principal; a non-loopback listener
   refuses to start without a configured authentication verifier. This does not
   grant browser users any write or coordination authority.

4. **The first API is read-only and task-focused.** Its first resources are:
   `GET /api/v1/fleet` for enrolled repository summaries; `GET
   /api/v1/repositories/{repository_id}` for one repository's coordination,
   freshness, and qualified status; and `GET
   /api/v1/repositories/{repository_id}/timeline` for accepted positions and
   evidence/receipt events. Each response includes the tenant-scoped
   repository identity, ledger position, projection timestamp, and freshness
   state. Write routes, enrolment mutations, claim mutations, and policy
   activation are excluded.

5. **The first Bridge screen is Fleet, not a generic dashboard.** It supports
   scanning repository health, filtering by freshness and coordination state,
   opening a repository detail, and tracing a visible status to its accepted
   receipt or evidence reference. Empty, stale, uncoordinated, and
   not-enrolled states remain visible rather than disappearing from counts.

6. **Read models remain derived and replaceable.** The API contracts name
   operational concepts, not projection table names. Rebuilding a projection
   cannot change ledger truth, and a rebuilding or lagging projection is
   presented as such. Live updates may begin as bounded polling; WebSockets or
   server-sent events require a measured need and a separate protocol decision.

7. **The service is additive to Ackplane's node protocol.** Node sync remains
   gRPC. The browser API can evolve without changing protobuf contracts or
   forcing browser clients to implement native gRPC.

## Consequences

- The initial implementation has a clear vertical slice: projection read
  store, authenticated `/api/v1/fleet`, repository detail/timeline endpoints,
  and a Fleet browser screen.
- The API can be contract-tested independently of the UI and can evolve without
  coupling browser releases to schema migrations or node protocol releases.
- Authentication and tenant authorisation are delivery prerequisites for any
  remotely reachable Bridge instance, rather than an afterthought after the UI
  is visible.
- The Bridge starts read-only. Administrative workflows remain in their
  existing reviewed paths until a later decision defines their authority and
  evidence requirements.

## Rejected alternatives

**Serve a single-page app directly from `ackplane-server` with SQL in handlers.**
Rejected because it couples node synchronization, browser deployment,
authentication, and projection queries into one deployable surface.

**Expose a gRPC-Web gateway over every Ackplane method.** Rejected because the
browser needs a narrow operational vocabulary, not the node protocol or future
write authority.

**Build a UI first against seeded JSON.** Rejected because it postpones the
hard parts - tenant scope, freshness, authority, and evidence traceability -
until the visual model has already become a contract.
