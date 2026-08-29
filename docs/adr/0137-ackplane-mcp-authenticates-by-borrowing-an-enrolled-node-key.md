# ADR-0137: `ackplane-mcp` authenticates by borrowing an enrolled node's key

- Status: Proposed
- Date: 2026-08-29
- Deciders: MindLeak maintainers (proposed in session; awaiting repository-owner
  review per this repo's adoption convention)
- Refines: [ADR-0136](0136-ackplane-gains-an-mcp-front-door-not-a-duplicated-storage-core.md)
  (decision 4 named this as an open, blocking decision; this ADR resolves it)
- Depends on: [ADR-0085](0085-node-enrolment-requires-proof-of-possession.md)
  (the Ed25519 possession-proof this reuses), [ADR-0098](0098-connection-trust-reuses-the-enrolled-key-oidc-waits.md)
  (connection trust today, and OIDC's explicit deferral), [ADR-0030](0030-discrete-per-agent-identity.md)
  (per-session, not per-process, identity — amended by ADR-0054),
  [ADR-0054](0054-identity-is-the-session-not-the-process.md) (identity is the
  session)
- Related: [ADR-0116](0116-enrolled-supervisors-are-the-distributed-agent-runtime.md)
  (the supervisor is the existing precedent for a node-colocated process
  making outbound authenticated connections)

## Context

ADR-0136 named the authenticated principal for an MCP client as an open,
blocking decision rather than resolving it. Investigated directly: today
there are exactly two authenticated principal types. An enrolled repository
node proves possession of its approved Ed25519 key over a per-connection
nonce (`enrollment::verify_connection_challenge`, ADR-0085/0098) — a real,
production-grade identity, but scoped to "this repository's node," not an
individual human or agent session. Bridge's loopback developer profile
(`BridgeConfig::resolve`, `ackplane-bridge/src/lib.rs`) derives one static
`development_tenant_token` — `hex(SHA-256(salt || tenant_name))` — per
installation; it refuses any non-loopback listen address, has no per-user or
per-session identity, no expiry, and no revocation. It exists explicitly as a
placeholder "while production authentication is wired" (its own module
comment) and cannot serve an off-machine or multi-session client at all.
Neither fits "an MCP client authenticating itself to Ackplane."

Ackplane's own data model, however, already composes a second layer of
identity *underneath* node-level connection trust: `agent_session_id` is a
first-class field threaded through `evidence_service.rs`, `directive_store`,
and `context_packet_store` — `session:v1:<hex>` strings, the exact format
ADR-0030/0054 already mint locally via `open_session(session_id)`. Today that
field is populated by *whatever the connected node reports*, trusted because
the node itself authenticated the connection. Nothing currently authenticates
an agent session independently of the node that reports it — but the shape
already fits: node-level transport trust, agent-session-level attribution
inside it, exactly mirroring the local planes' own model where stdio process
trust is implicit and `session_id` carries per-agent identity within it.

Building a new principal type (a revocable operator API key, or OIDC) is real,
security-sensitive server work — new issuance, storage, rotation, revocation,
and scoping — and ADR-0098 explicitly deferred OIDC until "a real second
tenant" justified it. `ackplane-mcp` is plausibly that tenant eventually, but
committing to the heaviest option first is not required to get a genuine pilot
working, and this repository's own reuse discipline (AGENTS.md's
before-you-write checklist) says to extend what exists before adding a new
primitive.

## Decision

**`ackplane-mcp` does not mint a new principal type. It runs colocated with an
already-enrolled repository node and authenticates its Ackplane connection
using that node's existing Ed25519 key over the existing NodeSync
challenge-response (ADR-0085/0098) — the same mechanism `ackplane-supervisor`
already uses (ADR-0116). Individual MCP client identity layers on top via the
existing `open_session(session_id)`/`agent_session_id` mechanism, unchanged
from how the local planes already work.**

1. **Node-level trust authenticates the connection; it does not authenticate
   an individual agent.** `ackplane-mcp` is deployed alongside (or embeds) the
   signer an enrolled node already holds under its approved-key material
   (ADR-0100's node-owned, non-exporting signer). It opens its Ackplane
   connection exactly as `ackplane-supervisor` does today: complete the
   connection challenge, prove possession, and nothing else authenticates the
   process. `ackplane-mcp` cannot run at all against a repository with no
   enrolled node — that is a real requirement, not an oversight, and is stated
   plainly rather than worked around.

2. **`open_session(session_id)` keeps meaning exactly what it means locally.**
   An MCP client calls `open_session` on `ackplane-mcp` precisely as it does on
   `mindleak-mcp`/`lodestar-mcp` today (ADR-0030, amended by ADR-0054):
   identity is the session, not the process, and one long-lived `ackplane-mcp`
   process serves multiple concurrent client sessions distinguished only by
   their registered `session_id`. `ackplane-mcp` derives an `agent_session_id`
   from it in the existing `session:v1:<hex>` form and threads it into every
   Ackplane call it makes on that session's behalf — the same field
   `evidence_service`, `directive_store`, and `context_packet_store` already
   carry, now populated by an independently-authenticated session instead of a
   bare value the node merely reported.

3. **Every call is attributed at two levels, and both are real.** The
   connection is provably "this enrolled node" (Ed25519). The call within it
   is "this declared session" (`agent_session_id`). Ackplane never conflates
   the two: a compromised or misbehaving `session_id` cannot forge the node's
   connection trust, and the node's connection trust never implies any one
   `session_id` acted honestly — that remains ordinary claim/evidence
   discipline, unchanged.

4. **This is an explicit, named limitation, not a silent one.** Every MCP
   session behind one `ackplane-mcp` process shares one connection identity —
   "this repository's enrolled node" — not a distinct human or account
   identity. Two different developers running `ackplane-mcp` against the same
   repository are indistinguishable from Ackplane's connection-trust
   perspective (though distinguishable by `agent_session_id`, which is
   self-declared, not independently verified, exactly as it is locally today).
   A deployment that needs to independently verify *which human* is behind a
   session — for audit, per-user revocation, or billing — needs a stronger
   principal than this decision provides, and must not be told this already
   solves that; it solves "an MCP client can reach Industrial coordination
   at all," not "Industrial coordination has verified multi-user identity."

5. **A stronger principal remains future work, sequenced, not blocked on.**
   A revocable operator API key (per-user, scoped, server-issued) or OIDC
   integration (ADR-0098's deferred decision) each remain available as a later
   ADR once real multi-user/off-machine demand exists. This decision does not
   preclude either; it does not require `ackplane-mcp` to wait for them.

6. **One open implementation question is deferred to the delivery slice, not
   this ADR:** whether Ackplane's NodeSync protocol already tolerates multiple
   concurrent connections signed by the same node key (`ackplane-mcp` running
   alongside an already-connected `ackplane-supervisor` daemon on the same
   node), or whether it needs an explicit multiplexing/sub-identity extension.
   No code inspected during this ADR's drafting asserts a single-connection
   limit, but the slice implementing `ackplane-mcp` must confirm this with a
   real concurrent-connection test before relying on it.

## Consequences

- `ackplane-mcp` can authenticate today, against already-enrolled
  repositories, with zero new server-side authentication code — unblocking a
  real pilot immediately rather than waiting on a new principal type.
- The pilot is explicitly scoped to already-enrolled repositories/machines.
  A developer with no enrolled node gets a clear, actionable refusal (enroll
  first), not a confusing auth failure.
- Per-human accountability inside one node's sessions rests entirely on
  self-declared `agent_session_id`, exactly as it already does for the local
  planes and for every node-reported evidence record today — this decision
  neither improves nor worsens that property, it only extends it to a new
  transport.
- A later, stronger principal (API key or OIDC) is still needed before
  `ackplane-mcp` fits a genuinely multi-tenant, off-machine, or
  per-user-billed deployment; this ADR does not claim otherwise.

## Rejected alternatives

**Build a new revocable operator API-key principal first.** Not rejected as a
future direction — deferred. It is real, security-sensitive work (issuance,
hashed storage, rotation, revocation, scope) that is not required to get a
first pilot working against the identity Ackplane already trusts, and
building it before a single real `ackplane-mcp` deployment exists risks
guessing at scopes nobody has needed yet.

**Adopt OIDC now.** Rejected for the same reason ADR-0098 rejected drafting it
speculatively: "an unvalidatable design nobody can validate until a real
second tenant shows up." `ackplane-mcp` running against real enrolled
repositories is a plausible path to that real second tenant, but reusing the
enrolled-node principal first is what gets there without a speculative,
unvalidated OIDC integration blocking every other part of ADR-0136.

**Extend Bridge's loopback developer-tenant token to cover `ackplane-mcp`.**
Rejected: that token is explicitly a single-installation, loopback-only
placeholder with no per-session identity, and ADR-0094/0098 already refuse to
let it bind non-loopback. Reusing it here would either silently weaken that
refusal or produce a second, incompatible meaning for the same token.
