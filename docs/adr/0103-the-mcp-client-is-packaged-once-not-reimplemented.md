# ADR-0103: The MCP client is packaged once, not reimplemented per consumer

- Status: Accepted
- Date: 2026-08-18
- Deciders: Pending human acceptance
- Accepted: 2026-08-18 by the repository owner, authorized directly in
  session — attributed human adoption after review.
- Related: [ADR-0059](0059-the-tool-surface-is-a-vocabulary.md) (the tool
  surface is a vocabulary), [ADR-0030](0030-discrete-per-agent-identity.md)
  (discrete per-agent identity — the handshake this wraps),
  [ADR-0016](0016-platform-packaging-and-registration.md) (platform packaging
  and registration — the adjacent, already-solved "ship a binary" concern),
  [ADR-0104](0104-a-reference-consumer-tests-the-tool-surfaces-stability.md)
  (a reference consumer tests the tool surface's stability)

## Context

Direct, first-hand evidence from this same day: bootstrapping CompLeak, an
external product depending on MindLeak, required hand-writing a roughly
150-line minimal JSON-RPC 2.0 stdio client (`lib/mindleak-client.mjs`) from
scratch, because no installable MindLeak client package exists. The only
prior art was `editors/vscode/src/mcpClient.ts` — extension-internal, never
published, not intended for reuse, and written against a VS Code extension
host rather than a standalone Node process.

The protocol itself is not the problem. It is simple and already stable:
newline-delimited JSON-RPC 2.0, documented in `docs/ARCHITECTURE.md`, and
covered by ADR-0059's tool-surface contract discipline. The problem is that
every consumer re-derives the same framing, handshake, and session-bootstrap
code independently, with no shared correctness guarantee between them —
exactly the kind of duplication this project's own "write it twice, extract
immediately" rule (AGENTS.md) exists to catch, here surfacing across repository
boundaries rather than within one codebase.

This repository's own README already claims MindLeak is "the layer coding
agents run on top of — not an agent and not a model." A layer nobody can run
on top of without reimplementing its wire protocol is a weaker version of that
claim than the one already published.

## Decision

**MindLeak publishes one reference client package per supported language,
versioned alongside the servers, so an external consumer depends on a library
rather than reimplementing the protocol.**

1. **A packaged client wraps the existing stdio JSON-RPC contract exactly as
   documented** — spawn, newline-delimited framing, the `initialize` ->
   `notifications/initialized` -> `open_session` handshake (ADR-0030), and
   `tools/call`/`tools/list`. It adds no new protocol surface; it is a typed,
   tested wrapper over what `mindleak-mcp`/`lodestar-mcp` already expose.
2. **The package ships from this repository, versioned with the servers it
   wraps**, so `serverInfo.version` compatibility is checkable at connect time
   rather than discovered at the first failing call. A minor server release
   that only adds tools stays backward compatible per ADR-0059; the client
   package's own version communicates that same guarantee to its consumers.
3. **The first target is TypeScript/Node**, matching the two consumers that
   already exist (the VS Code extension internally, CompLeak externally) and
   the ecosystem most current MCP consumers are written in. Additional
   language bindings are separate future decisions, not blocked by this one.
4. **The public surface groups typed methods per tool family** — graph reads,
   task lifecycle, conformance, evidence — rather than exposing only a raw
   `callTool(name, args)` escape hatch. It may still dispatch generically
   underneath, but a consumer's editor tooling and type-checker should catch a
   wrong argument shape before a round trip does.
5. **The extension's own `mcpClient.ts` migrates to depend on the published
   package** rather than keeping its own parallel implementation, per this
   project's own reuse discipline: if a near-miss exists, extend it rather
   than forking. Extension-specific concerns — output-channel logging,
   restart/reconnect UI state — stay in the extension; protocol framing does
   not. This migration is a follow-on task, not committed to inside this ADR.
6. **The package never bundles or spawns a specific server binary.** It
   accepts a command or path (or an already-running transport) the same way
   `resolveBinaryPath` already resolves one in the extension. Packaging a
   client must not quietly become packaging a redistribution of the server —
   that is the release installer's already-solved job (ADR-0016).

## Consequences

- One more artifact to version and publish alongside each release, in
  exchange for removing a whole class of "did I frame the JSON-RPC correctly"
  bugs from every future consumer, CompLeak included.
- The extension's `mcpClient.ts` gets a follow-on migration task once the
  package exists — deliberately not decided here, since replacing working
  code with a dependency is its own reviewed change, not a rider on
  introducing the dependency.
- A published package is a stronger compatibility promise than an internal
  file ever was; ADR-0104's reference-consumer compatibility gate is what
  keeps that promise honest in CI going forward.

## Rejected alternatives

- Leaving every consumer to hand-roll its own client, as today — rejected:
  this is the status quo the audit is naming as a gap, evidenced concretely by
  CompLeak's own bootstrap this same session.
- Wrapping the official `@modelcontextprotocol/sdk` transport instead of a
  bespoke implementation — a real option worth evaluating at implementation
  time rather than settling here; the decision that matters now is publishing
  *something* installable, not which transport library sits underneath it.
- Publishing the client from a separate repository — rejected: the client's
  correctness is a direct function of the servers' actual behaviour; keeping
  them in one repository with one CI run is what lets ADR-0104's
  compatibility gate run against every change automatically.
