# ADR-0105: The Bridge is the server version of the VSIX

- Status: Accepted
- Date: 2026-08-19
- Deciders: MindLeak maintainers
- Accepted: 2026-08-19 by the repository owner, authorized directly in
  session - attributed human adoption after review.
- Supersedes: [ADR-0094](0094-the-bridge-preserves-standalone-operation.md)
  (the Bridge as a separate assurance-only product surface)
- Refines: [ADR-0095](0095-the-bridge-uses-an-authenticated-projection-api.md)
  (the first read-only API is a delivery slice, not the product ceiling)
- Depends on: [ADR-0082](0082-ackplane-is-a-standalone-federation-service.md)
  (federation authority), [ADR-0083](0083-grpc-is-the-ackplane-node-protocol.md)
  (typed node protocol), [ADR-0089](0089-mindleak-is-an-operating-system-for-agent-coordination.md)
  (product and capability vocabulary), [ADR-0103](0103-the-mcp-client-is-packaged-once-not-reimplemented.md)
  (one packaged client boundary)

## Context

ADR-0094 protected a real product property: one developer can install the VSIX
and use the local MindLeak and Lodestar planes without an account, network, or
server deployment. It drew the wrong conclusion from that property. It made the
Bridge a separate, read-only assurance product rather than the server form of
the experience the VSIX already provides.

That split leaves the product with two incompatible meanings. The VSIX can show
and operate Context, Work, Board Doctor, Design, Evidence, Knowledge, Telemetry,
Readiness, and local lifecycle workflows. The Bridge can only inspect a Fleet
projection. Calling the latter the human operations interface while excluding
the operations makes the product boundary depend on which UI a person opened.

The load-bearing boundary is storage authority, not feature category, developer
count, or fleet size. The VSIX runs the planes against repository-local SQLite.
Ackplane runs the server product against its own PostgreSQL store. SQLite is the
lightweight fit for a small solo project. PostgreSQL is the industrial fit for a
large codebase, higher coordination complexity, stronger durability, or shared
work across agents, developers, projects, repositories, or machines. One
developer may need the industrial profile; several developers do not redefine
the product.

The user chooses that deployment profile during setup. MindLeak does not infer
it from repository size, silently switch when a second agent appears, or fall
back from PostgreSQL authority to SQLite when Ackplane is unavailable.

The Industrial profile is a hosted backplane for both coordination and
learning. AgentD, Agency, VSIX-connected repository nodes, and other agent
runtimes can use the same service without adopting one editor or one agent
framework. Bridge is the human client of that backplane; it is not the only
client and it is not the place agents themselves run.

"Server version of the VSIX" therefore means the same capability model and
human workflows over a different storage implementation. It does not mean
exposing or replicating local SQLite, forwarding raw MCP, or putting a remote
shell in a browser.

## Decision

1. **The Local profile is the SQLite-backed VSIX.** It is the default fit for a
  small solo project and remains complete for repository-local use:
  `mindleak-mcp` owns the local `graph.db` and `lodestar-mcp` owns the local
  `spec.db`. It needs no Ackplane, account, PostgreSQL, Docker, or network
  reachability. The VSIX also owns VS Code-specific integration: editor and
  workspace context, terminal and Git sensors, opening source and ADR files,
  local binary registration, and activation lifecycle.

2. **The Industrial profile is the hosted, PostgreSQL-backed Ackplane and
  Bridge.**
  Ackplane's PostgreSQL database is the authoritative store for shared server
  state, durable records, arbitration, and projections; it is not a cache of a
  chosen developer's SQLite files. The Bridge is its human operations
  interface and the server counterpart of the VSIX. It is intended for large
  codebases and coordination complexity, whether that means one developer
  running many agents, multiple developers, one or many projects, one or many
  repositories, several machines, or any combination of those.

3. **Setup selects the storage profile explicitly.** A user chooses Local or
  Industrial when configuring a project or installation. The choice is
  persisted and visible. No runtime heuristic promotes a repository because
  it became large or gained another agent; no connectivity failure demotes an
  Industrial deployment to Local authority. Moving between profiles is an
  explicit, auditable migration workflow, not an automatic fallback.

4. **Ackplane is agent-runtime neutral and makes coordination and learning
  first-class server capabilities.** AgentD, Agency, enrolled repository
  nodes, and other agents use versioned protocols or packaged SDKs to claim
  work, report evidence, exchange durable task context, publish corroborated
  signals or lessons, and query what the fleet has learned. Shared knowledge
  remains scoped, attributed, provenance-bearing, revalidated, and decaying;
  central storage does not turn an observation into policy. Ackplane
  coordinates agent runtimes but does not host models, schedule their compute,
  edit source, or execute the agents themselves.

5. **The first Industrial workflow is coordinating agents from the Bridge.**
  Before broader feature parity, the Bridge becomes the human control room for
  active work. It shows enrolled agents and sessions, their project and
  repository, heartbeat, current task, lease, declared scope, waits, and
  completion state. An authorized operator can create and route work, assign
  or delegate it, release or recover leases, pause and resume work, answer
  durable questions, inspect overlap and stalls, and follow evidence through
  review and completion. These are typed coordination mutations against
  Ackplane authority, not commands to an operating-system process.

6. **Feature parity is the target; storage changes the implementation.** The Bridge must
   provide the server meaning of the VSIX's human workflows: Context, Fleet,
   Work, Board Doctor, Design, Evidence, Knowledge, Telemetry, Readiness,
   backup/export, and lifecycle operations. A workflow may aggregate several
   enrolled repositories or name one repository explicitly, but it does not
   disappear merely because its source of truth is remote. A missing Bridge
   counterpart is an implementation gap, not deliberate product separation.

7. **One capability model sits behind two storage adapters.** Human workflow
  modules depend on a typed capability interface rather than VS Code, SQLite,
  HTTP, or PostgreSQL directly. The local adapter composes the packaged MCP
  client with the VSIX host and the SQLite-backed planes. The server adapter
  calls the Bridge's versioned, authenticated API over Ackplane's
  PostgreSQL-backed authority and projections. Shared components and state
  models are reused where the hosts permit it; neither storage implementation
  invents different task, evidence, knowledge, or design semantics.

8. **The VSIX remains the local device driver, not the server's storage
  gateway.** Browser code cannot observe editor focus, terminal completion,
  Git events, or a developer's filesystem. Those sensors and host actions stay
  in the VSIX or an enrolled repository node. In Local they write the SQLite
  planes; in Industrial an enrolled node publishes typed domain records that
  Ackplane accepts into PostgreSQL. The Bridge never reads SQLite through an
  extension tunnel, and PostgreSQL never becomes a block-level replica of
  either local database.

9. **Server-owned actions use typed Ackplane APIs.** Claims, leases, shared
   coordination, enrolment, and future server-owned policy or review actions
  are evaluated against PostgreSQL by their one authoritative service and
  exposed through narrow, principal-scoped command resources. A successful
  HTTP response is not proof of completion: each accepted mutation records its
  actor, target, idempotency identity, authoritative result, and evidence or
  receipt reference.

10. **Actions requiring local state execute on an enrolled node through typed
   commands.** Reconcile, ingest, local backup/export, source navigation, and
   similar operations cannot execute inside the Bridge service. A remote form
   needs a reviewed command contract, explicit repository and capability scope,
   local authorization, bounded deadlines and retries, idempotency, and a
   durable request/result receipt. ADR-0083's prohibition remains load-bearing:
   no arbitrary shell, terminal input, source patch, untyped `execute` payload,
   or raw MCP tunnel becomes the implementation shortcut.

11. **Scope, freshness, and authority remain visible.** A local VSIX view says it
   is repository-local. A Bridge view identifies the principal, selected
   projects and repositories, ledger position, projection freshness, and any
   repository that is unenrolled, stale, disconnected, or outside the
   principal's scope. Aggregation never launders partial publication into a
   complete fleet statement.

12. **ADR-0095's read-only Fleet API remains the correct first slice, not a
   permanent limit.** Its Fleet, repository detail, and timeline resources are
   the initial projection-backed implementation of this architecture. New
   command APIs do not follow automatically from this ADR: each authority and
   trust boundary still needs its typed contract, authentication,
   authorization, evidence, and failure semantics reviewed before delivery.
   What changes now is the roadmap boundary - write and workflow surfaces are
   expected Bridge work rather than categorically excluded.

13. **Packaging preserves both entry points without forking the product.** The
    VSIX packages the local client, sensors, and binaries. A server deployment
    packages Ackplane and the Bridge. Shared client, capability, and UI modules
    are versioned once where feasible, and the reference-consumer gate verifies
    the protocol they share. Installing the VSIX never silently starts a server;
    running Ackplane never makes local planes depend on server availability.

## Consequences

- The current Bridge Fleet page is a real vertical slice but not a complete
  product boundary. The immediate next slice is the PostgreSQL-backed Agents
  and Work control room described above; Design, Evidence, Knowledge, Context,
  Telemetry, Readiness, and reviewed operations follow as explicit
  server-roadmap surfaces.
- The extension must progressively separate reusable workflow state and views
  from VS Code-only sensors and commands. ADR-0103's packaged client is the
  natural protocol boundary; duplicating its MCP implementation in each host is
  not accepted.
- Industrial capabilities need first-class PostgreSQL schemas and projections
  that implement the shared domain contracts. The server never queries or
  copies a local SQLite database wholesale; enrolled nodes publish typed
  records across the protocol boundary. Publishing a new class of local
  information remains an explicit data and trust decision.
- The current server ledger, claim authority, enrolment, and structural
  projection are foundations rather than the whole Industrial product. Shared
  task context and learned knowledge need PostgreSQL-backed domain stores and
  agent-facing contracts before the hosted backplane reaches feature parity.
- Bridge and external agents consume the same capability vocabulary through
  different clients: a human-facing web interface and machine-facing SDKs or
  protocols. Neither is privileged to invent a second meaning for a claim,
  lesson, evidence record, or design decision.
- Remote mutations cost more than local commands: identity, authorization,
  idempotency, receipts, and failure recovery are product requirements rather
  than infrastructure details.
- Setup makes the cost and capability choice explicit. Local minimizes moving
  parts for a small solo project. Industrial pays for PostgreSQL, identity, and
  service operation when codebase size, durability, or coordination complexity
  justifies them - even for one developer.
- Product copy must stop calling the Bridge assurance-only. It is the human
  operations interface for the server product, with assurance as one
  capability among the same MindLeak coordination workflows.

## Rejected alternatives

**Keep the Bridge as a separate assurance dashboard.** Rejected because it
makes the named human operations interface unable to perform or even represent
most product workflows, and forces users to switch products when coordination
scope grows.

**Remove the VSIX and require Ackplane for one developer.** Rejected because
local-first, offline operation is a real capability and the VSIX owns editor
integration a browser cannot reproduce.

**Fork the VSIX workflows into unrelated Bridge implementations.** Rejected
because task, evidence, knowledge, and design semantics would drift by host.
The adapters differ; the capability model does not.

**Make the Bridge a web tunnel into one developer's SQLite databases.**
Rejected because it gives an industrial server a workstation-scoped source of
truth, exposes local storage lifecycle as a network boundary, and cannot
arbitrate several processes, developers, or repositories consistently. The
server owns PostgreSQL state populated through typed domain contracts.

**Replicate `graph.db` or `spec.db` wholesale into PostgreSQL.** Rejected
because storage schemas are implementation details, not federation contracts.
Ackplane accepts domain records and builds authoritative server projections;
it does not synchronize database pages or create two writable copies of local
rows.

**Make Ackplane a private backend used only by Bridge or the VSIX.** Rejected
because the Industrial product is a coordination and learning substrate for
agent runtimes. AgentD, Agency, and other clients must be able to use the
published contracts without pretending to be an editor extension.

**Run agents or models inside Ackplane.** Rejected because Ackplane owns shared
coordination, knowledge, and proof, not agent compute. Hosting execution would
mix the arbiter with the actors it arbitrates and turn a backplane into another
agent platform.

**Forward MCP or expose a generic remote execution endpoint.** Rejected because
it erases the authority boundary, turns a cooperative coordination plane into a
remote shell, and cannot provide operation-specific authorization or durable
proof.

**Require multiple developers or an organisation before Ackplane is valid.**
Rejected because one developer coordinating several agents, projects,
repositories, or machines already needs shared authority. Fleet size changes
load, not semantics.

**Select a profile automatically from repository size or connected-agent
count.** Rejected because a heuristic cannot know a user's durability,
operational, or cost requirements, and changing storage authority while work is
live would be a migration disguised as configuration. Setup asks; the user
decides.
