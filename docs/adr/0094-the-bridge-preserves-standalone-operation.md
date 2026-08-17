# ADR-0094: The Bridge preserves standalone operation

- Status: Proposed
- Date: 2026-08-17
- Deciders: MindLeak maintainers
- Depends on: [ADR-0016](0016-platform-packaging-and-registration.md)
  (platform packaging), [ADR-0082](0082-ackplane-is-a-standalone-federation-service.md)
  (federation boundary), [ADR-0088](0088-the-ackplane-runs-in-containers-the-planes-do-not.md)
  (container boundary), [ADR-0089](0089-mindleak-is-an-operating-system-for-agent-coordination.md)
  (product category)

## Context

The Bridge is the most compelling human-facing part of the full MindLeak
product: one place to inspect fleet health, evidence freshness, claims, and
qualified certification status across repositories. It must not turn that
ambition into a tax on the local product.

MindLeak is already useful as a standalone VSIX plus two local stdio servers.
Its graph and intent stores are SQLite databases on the developer's machine;
they work without a network listener, account, container runtime, or Ackplane
deployment. Requiring a service for the local graph, Work surface, Evidence
Board, or repository-local coordination would replace a product property with
a deployment preference.

Conversely, presenting an organisation-wide browser view as though it were a
local view would be misleading. Only Ackplane can answer tenant-wide questions
from accepted federation records. A browser must neither inspect local SQLite
files nor silently make a federated repository locally authoritative when the
server is unavailable.

## Decision

1. **MindLeak has two explicit product modes.** `standalone` is the default
   local product: the VSIX, `mindleak-mcp`, and `lodestar-mcp` run against their
   repository-local SQLite stores. `full-server` is an optional deployment:
   Ackplane and the Bridge add an organisation assurance surface over accepted
   federation records. The modes are capabilities a user chooses, not build
   tiers that remove features from the local product.

2. **Standalone remains complete for one repository.** The bundled VSIX and
   local MCP servers retain the Context Graph, Work, Design Board, Evidence
   Board, telemetry, readiness, backup, export, reset, and local coordination
   workflows. They require no browser service, login, PostgreSQL, Docker, or
   network reachability. The browser Bridge is absent in this mode by design;
   the VSIX is its local human interface.

3. **Full-server adds a distinct assurance view; it does not replace local
   planes.** The Bridge shows tenant-wide fleet, repository, evidence, claim,
   and certification information from Ackplane read models. Repository-local
   memory, deterministic ingestion, conformance, and their SQLite lifecycle
   remain local. The Bridge cannot browse a developer's local graph, source,
   terminal output, or filesystem.

4. **Scope and authority are visible in every human surface.** A VSIX view
   identifies repository-local data. A Bridge view identifies its tenant,
   repository selection, ledger position, and projection freshness. A tenant
   aggregate is never presented as a complete statement about repositories
   that have not enrolled or have not published recent records.

5. **Coordination authority never falls back silently.** A repository in
   `local` coordination mode continues to use its local arbiter. A repository
   in `federated` mode follows ADR-0082: an unavailable Ackplane lease may
   expire, and the client reports the resulting uncoordinated work rather than
   acquiring a local replacement claim. The Bridge reports that state; it does
   not repair it by changing mode.

6. **Packaging keeps the modes independently usable.** A platform-targeted
   VSIX continues to bundle and register the local binaries. A full-server
   deployment packages Ackplane and the Bridge using ADR-0088's container
   boundary. Installing one must not implicitly install, start, or configure
   the other.

## Consequences

- A developer can install the VSIX and immediately use MindLeak offline, as
  they can today.
- Organisations can deploy a browser assurance interface without publishing
  local SQLite data or exposing unauthenticated stdio MCP servers.
- Product copy and UI states have to be precise about local versus federated
  scope, freshness, enrolment, and authority.
- The Bridge cannot be treated as a cosmetic rewrite of the VSIX. It is a
  separate tenant-aware client with a different source of truth.
- Full-server configuration and authentication add operational work, but that
  work remains optional for repository-local users.

## Rejected alternatives

**Make the Bridge the only UI.** Rejected because it makes the local product
depend on a server, account, and network while duplicating a VSIX experience
that is already appropriate for one repository.

**Have the browser read local SQLite through an extension tunnel.** Rejected
because it exposes storage schema as API, cannot form a tenant-wide view, and
turns a developer workstation into a service boundary.

**Let a disconnected federated client temporarily arbitrate locally.** Rejected
because it creates the dual authority ADR-0082 was written to prevent.
