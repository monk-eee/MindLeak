# ADR-0136: Ackplane gains an MCP front door; it does not duplicate the local planes' storage core

- Status: Proposed
- Date: 2026-08-29
- Deciders: MindLeak maintainers (proposed in session; awaiting repository-owner
  review per this repo's adoption convention)
- Refines: [ADR-0105](0105-bridge-is-the-server-version-of-the-vsix.md) (this
  is the agent/MCP-facing half of feature parity; ADR-0105 defined the
  human/browser-facing half)
- Depends on: [ADR-0082](0082-ackplane-is-a-standalone-federation-service.md)
  (Ackplane is the standalone service being fronted),
  [ADR-0086](0086-postgresql-is-the-ackplane-ledger-and-arbiter.md)
  (PostgreSQL is Ackplane's own authority, not a mirror of local rules),
  [ADR-0087](0087-the-ackplane-graph-is-a-projection-not-an-authority.md)
  (the projected graph's existing decay contract), [ADR-0059](0059-the-tool-surface-is-a-vocabulary.md)
  (the tool vocabulary this front door mirrors), [ADR-0103](0103-the-mcp-client-is-packaged-once-not-reimplemented.md)
  (packaged MCP client — the consumer side of this same protocol)
- Related: [ADR-0113](0113-the-industrial-knowledge-plane-is-evidence-backed-and-human-governed.md)
  (why Industrial knowledge is not a drop-in recall backend),
  [ADR-0120](0120-industrial-work-domain-is-an-authoritative-task-projection.md)
  (why Industrial Work's task semantics are narrower than Lodestar's board
  today), [ADR-0098](0098-connection-trust-reuses-the-enrolled-key-oidc-waits.md)
  (the authenticated-principal question this ADR depends on but does not
  resolve), [ADR-0045](0045-a-fleet-is-a-distributed-system.md) (split-authority
  risk), [ADR-0089](0089-mindleak-is-an-operating-system-for-agent-coordination.md)
  (product and capability vocabulary)

## Context

A user asked, directly: can the VSIX and local SQLite-backed `mindleak-mcp`/
`lodestar-mcp` be retired in favor of the PostgreSQL-backed Ackplane/Bridge
stack, for an MCP-speaking client — the same kind of client this very session
is? Investigated directly rather than from memory: Ackplane speaks gRPC only
and Bridge speaks HTTP only; neither has ever spoken MCP. `mindleak-mcp` has no
Postgres path at all — its `federation-client` feature only makes a bare
reachability probe real, changing zero data-path behavior. `lodestar-mcp`
federates only claim/renew/release/recover plus overlap preflight through
Ackplane; goals, constitution, the task board, conformance, and knowledge stay
local SQLite regardless of federation configuration. Today, no MCP client can
reach the Industrial profile at all.

[ADR-0105](0105-bridge-is-the-server-version-of-the-vsix.md) already answers
a closely related question: it defines Local (VSIX + SQLite) and Industrial
(Ackplane + Bridge + PostgreSQL) as the product's two profiles, states
"one capability model sits behind two storage adapters" (clause 7), and holds
Bridge to feature parity with the VSIX (clause 6). But its parity model is
browser/human-workflow parity through Bridge's HTTP API and gRPC SDKs for
"AgentD, Agency, ... and other agent runtimes" (clause 4). It does not cover
an MCP-native client — the specific shape almost every current AI coding
assistant, including this one, actually speaks — reaching Industrial directly
without adopting a bespoke HTTP/gRPC integration.

Two active constitution entries bound this decision precisely.
`goal:ackplane-federation-service` requires Ackplane to remain "a separately
deployable federation service ... never becoming a mode of either local
plane" — ruling out mounting PostgreSQL underneath `mindleak-core`/
`lodestar-core` as a literal swap-in backend for those crates.
`local-only-security-boundary` binds the *Local* plane to stdio-only, no
network listener — it says nothing about Ackplane, which is already an
intentionally networked, authenticated service (ADR-0084/0085/0098). This ADR
does not touch the Local profile's no-network property at all.

File-level evidence changes what "give me a Postgres-backed mindleak-mcp"
should mean. `mindleak-core`'s ~74 files split cleanly: only `db.rs` and the
`graph/`/`telemetry/` modules (~17 files) touch `rusqlite`; `decay.rs`,
`model.rs`, and the entire 25-file `ingest/` tree (git/AST/execution/structure
parsing — the whole deterministic ingestion engine) already have zero SQL
coupling. `lodestar-core`'s 100 files split the same way: `rusqlite` lives only
in `store/` (12 files) and `db/` (4 files); the 30-file `facade/` layer — the
actual claim/lease/conformance/constitution business rules — and `model/`
touch no SQL directly. Meanwhile Ackplane's own PostgreSQL schema already has
partial analogs for each local domain: `projected_nodes`/`projected_edges`
(0002_projection.sql) use the identical decay contract as `mindleak-core`
(base weight + half-life stored, effective weight always derived at query
time); `knowledge`/`knowledge_embeddings` (0007_knowledge.sql) use `pgvector`
at the same 768-dimension embedding model; `work_tasks` and friends
(0028_work.sql, ADR-0120) are a structurally close but semantically narrower
task-board analog.

Put together, those two facts point away from the naive reading of the ask.
Standing up new `mindleak-core-postgres`/`lodestar-core-postgres` crates and
`mindleak-server-mcp`/`lodestar-server-mcp` binaries would re-derive business
rules (task lifecycle, conformance, decay, constitution) that Ackplane's own
server already owns for its domain — a second, independently-written
implementation of the same concepts, which is exactly the split-authority
outcome ADR-0045 and ADR-0082 already reject, and exactly what ADR-0105 clause
7's "one capability model" was written to prevent. The narrower gap that is
actually open, and that this ADR closes, is: **no protocol adapter lets an MCP
client reach Ackplane's already-existing typed services.** ADR-0105 clause 10
forbids a raw MCP tunnel as a shortcut around typed command contracts for
node-local actions; it does not forbid a new, typed, *translating* MCP surface
over RPCs that are already typed and already authorized.

## Decision

**Ackplane gains a thin MCP protocol adapter that translates MCP tool calls
into calls against Ackplane's existing (or, where a real domain gap exists,
newly added) gRPC services. It is a protocol front door, not a second storage
engine, and it does not fork `mindleak-core`/`lodestar-core` onto Postgres.**

1. Add a new binary, `ackplane-mcp`, speaking the same newline-delimited
   JSON-RPC 2.0 MCP protocol as `mindleak-mcp`/`lodestar-mcp` (ADR-0059's
   tool-surface contract). Every tool handler is a translation to an Ackplane
   gRPC call; none re-implements decay math, claim/lease rules, or conformance
   logic locally. Where a handler needs a capability Ackplane's server does not
   yet expose over gRPC, that capability is added to `ackplane-server`/
   `ackplane-core` first, under its own review, and `ackplane-mcp` calls it —
   the same discipline this repository already applies to the VSIX and Bridge.

2. Tool names and argument/response shapes track the local tool vocabulary
   (ADR-0059) wherever Ackplane's domain already covers the same concept, so
   switching a connection target from `mindleak-mcp`/`lodestar-mcp` to
   `ackplane-mcp` does not require learning a new vocabulary for the same
   operation. Where Industrial semantics genuinely differ — Industrial Work's
   task projection (ADR-0120) lacks local claim-transfer and
   goal-decomposition semantics; Industrial knowledge (ADR-0113) is curated and
   human-governed, not full decaying recall — the tool is named distinctly or
   documents the difference explicitly, refusing with a typed reason rather
   than silently approximating. This mirrors `worker_adapter.rs`'s
   `AdapterError::Unsupported` discipline: an honest refusal, not a fake
   success.

3. Closing today's domain gaps against the local tool contract is Ackplane's
   own, separately reviewed work, sequenced as follow-up tasks under
   `goal:ackplane-federation-service` rather than designed in full here:
   - **Graph/recall parity.** Add a `pgvector` embeddings table scoped to
     `projected_nodes` itself (parallel to, but distinct from, the curated
     `knowledge_embeddings` table), so `ackplane-mcp`'s `recall` has a real
     graph-wide backing store instead of reusing a domain scoped for a
     different purpose.
   - **Task/claim parity.** Decide whether `work_tasks` (ADR-0120) grows the
     missing claim-transfer/renew/recover/goal-decomposition semantics, or
     whether `ackplane-mcp`'s task tools deliberately compose Ackplane's
     already-federated claim arbitration with Industrial Work rather than
     pretending Industrial Work already equals Lodestar's full task board.
   - **Ingestion authority.** Deterministic git/AST/execution ingestion
     (`mindleak-core`'s `ingest/` tree) is already pure, portable Rust with no
     SQL coupling; it needs a repository-side publisher — a thin sensor
     reusing the existing NodeSync/enrollment path (ADR-0082/0085) — rather
     than being reimplemented server-side or dropped. An `ackplane-mcp`
     ingestion tool, if offered at all, means "accept a fact this sensor
     already extracted," never "parse source server-side."

4. **Authentication for an interactive MCP session against Ackplane is an open
   decision this ADR depends on, not one it resolves.** Today's two principal
   types — an enrolled repository node's Ed25519 possession proof
   (ADR-0085/0098) and Bridge's loopback-only developer-tenant token
   (ADR-0098 decision 3, explicitly not multi-tenant auth) — do not fit "an
   arbitrary MCP client, potentially off the enrolled machine, authenticating
   as itself." `ackplane-mcp` does not ship past a local-loopback pilot until a
   third principal type is decided, or an enrolled-node principal is explicitly
   and narrowly reused (for example: "`ackplane-mcp` runs colocated with an
   already-enrolled node and borrows its key"). ADR-0098 deferred OIDC
   pending "a real second tenant"; `ackplane-mcp` is plausibly that tenant, but
   the authentication decision itself belongs to its own follow-up ADR.

5. **Nothing about the Local profile changes.** `mindleak-mcp`/`lodestar-mcp`
   keep their stdio-only, no-network-listener contract — the active
   `local-only-security-boundary` constraint — exactly as-is. A developer who
   wants zero network dependency is unaffected. `ackplane-mcp` is an
   additional, opt-in front door onto the already-networked Industrial profile
   (ADR-0082, ADR-0088); it is not a mode either local plane can be switched
   into, and no runtime heuristic ever promotes a Local repository into it.

6. **`mindleak-core`/`lodestar-core` are not forked onto Postgres.** No
   `mindleak-core-postgres`/`lodestar-core-postgres` adapter crate is created,
   and no storage trait is imposed on either crate to serve a backend it was
   never asked to serve. Where genuinely shared, storage-agnostic logic
   already exists on both sides of the schema boundary — specifically
   effective-weight decay derivation, which `mindleak-core::decay` and
   Ackplane's `projected_nodes`/`projected_edges` already both implement as
   "store base weight and half-life, derive effective weight at query time,
   never persist it" — that narrow, pure logic may be factored into one small
   shared crate both servers depend on. This is an optional simplification
   available because the contract already matches, not a mandate to unify the
   two servers' storage layers generally.

## Consequences

- An MCP-speaking agent gains a real path to the Postgres-backed Industrial
  profile instead of the VSIX/local SQLite stack — the exact gap raised — but
  only once the authentication decision (clause 4) and the domain-parity
  slices (clause 3) land. This ADR alone does not make the switch usable
  end-to-end; it fixes the shape of the solution and sequences the remaining
  work.
- Ackplane's own domain stores (projection, knowledge, work, constitution)
  gain a third consumer beyond Bridge and NodeSync, which will surface gaps in
  their current scope faster than leaving them Bridge-only.
- No duplicate implementation of decay, claim/lease, or conformance rules is
  created. The two profiles' behavior can only drift by omission (a
  not-yet-closed gap), never by two independently written, silently divergent
  rule engines answering the same question differently.
- `mindleak-core`/`lodestar-core`'s existing `rusqlite`-only footprint is left
  exactly as it is today — there is no urgency to abstract either crate behind
  a storage trait it was never asked to serve.
- Follow-up ADRs are needed for: (a) the authenticated principal for a
  non-enrolled MCP client, (b) task/claim domain parity between Industrial
  Work and Lodestar's full task board, (c) a `pgvector`-backed recall store
  for the projected graph.

## Rejected alternatives

**Fork `mindleak-core`/`lodestar-core` into new `-postgres` sibling crates
behind a storage trait, with new `mindleak-server-mcp`/`lodestar-server-mcp`
binaries built on them.** Rejected: this duplicates business logic (task
lifecycle, conformance, decay, constitution rules) that Ackplane's own server
already owns for its domain, directly working against
`goal:ackplane-federation-service` ("never becoming a mode of either local
plane") and ADR-0105 clause 7 ("one capability model," not two independently
maintained rule engines). It produces two authorities for the same concept —
a claim, a decayed weight, a conformance verdict — with no arbitration between
them, the exact split-authority risk ADR-0045/0082 already reject.

**Treat Bridge's HTTP API and gRPC SDKs as the sole agent-facing surface;
MCP-native clients adopt a bespoke integration instead.** Considered, not
rejected outright — this is roughly ADR-0105 clause 4's existing stance for
"AgentD, Agency, ... and other agents." It does not fit MCP-native clients
(this session's own tool-calling shape, and most current AI coding assistants)
that only bind to newline-delimited JSON-RPC MCP servers; requiring every such
client to instead hand-roll an HTTP/gRPC integration reproduces the exact
"every consumer re-derives the same...code" problem ADR-0103 diagnosed for the
local planes, aimed at Ackplane instead.

**Have `ackplane-mcp` implement each tool's logic directly against
PostgreSQL, bypassing Ackplane's own gRPC service layer.** Rejected: this still
creates two independent rule implementations against the same tables — one
inside `ackplane-server`'s gRPC handlers, one inside `ackplane-mcp` — the same
split-authority failure as the first rejected alternative, without even the
excuse of a different crate boundary.
