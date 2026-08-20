# ADR-0098: Connection trust reuses the enrolled node key; OIDC waits for a real second tenant

- Status: Accepted
- Date: 2026-08-17
- Deciders: MindLeak maintainers
- Accepted: 2026-08-18 by the repository owner, authorized directly in
  session — attributed human adoption after review.
- Depends on: [ADR-0084](0084-ackplane-evidence-has-explicit-trust.md) (Ackplane
  evidence has explicit trust), [ADR-0085](0085-node-enrolment-requires-proof-of-possession.md)
  (node enrolment requires proof of possession)
- Refines: ADR-0084 decisions 1, 3, 9; ADR-0085's activation ceremony extended
  to cover the live connection, not only the envelope
- Related: [ADR-0094](0094-the-bridge-preserves-standalone-operation.md) (the
  Bridge preserves standalone operation)

## Context

`task:ee95827610cd` (the coarse "Implement: ADR-0084" promotion seed) has been
reopened and re-blocked three times. Its own thread is precise about why:
signed evidence envelopes, node enrolment, key rotation, and revocation all
shipped — `crates/ackplane-server/src/{signing_keys,envelope_signature,
enrollment,enrollment_service}.rs`, Ed25519, domain-separated, tenant/repo/node
binding checked at verification time. What remains unstarted is OIDC (decision
1), mTLS client-certificate binding for the *live connection* as opposed to the
envelope payload (decision 3), and tenant/repository-scoped authorization for
Bridge/administrative actions (decision 9) — each flagged as "itself
design-sized," which is why the coarse task stayed blocked rather than being
cut into a task nobody could honestly scope.

That framing carried an assumption worth correcting: it treated Ackplane as a
multi-tenant SaaS serving independent customer organisations, each needing its
own federated identity provider. That is not the shape being built right now.
The actual near-term deployment is one operator's fleet of *repositories* —
plural repositories, one trust domain. ADR-0084 decision 9's tenant scoping is
real (a repository id is still a required predicate on every durable key and
query — that does not go away), but the OIDC-grade *human* identity federation
in decision 1 was designed for a boundary — independent organisations, each
bringing their own IdP — that does not exist yet. Drafting OIDC integration
speculatively, before a second real tenant exists to design against, is the
kind of premature infrastructure this repository's own philosophy warns
against: it would ship a large, untestable surface with no real second party
to prove it against.

Node identity does not have the same gap. ADR-0085's enrolment ceremony
already gives every node a durable, non-exportable Ed25519 identity, proven by
signing a domain-separated challenge, and already used to sign every evidence
envelope. Decision 3's mTLS requirement was written as if connection
authentication were a separate problem needing its own certificate authority.
It does not need to be: the node already holds a key Ackplane already trusts.
Standing up a second, parallel PKI to authenticate the *transport* when one
already exists to authenticate the *content* would be exactly the kind of
duplicate machinery this repository's reuse discipline exists to catch.

## Decision

**Reuse the enrolled node key for connection trust. Keep OIDC and full
tenant-organisation federation off the critical path until a second real
tenant exists to design them against.**

1. **A `Synchronize` connection authenticates with the same key that signs its
   envelopes, not a second certificate.** On stream open, Ackplane issues a
   short-lived, single-use, domain-separated nonce (parallel to
   `enrollment::activation_challenge_bytes`, with its own domain string so a
   connection challenge can never be replayed as an activation or an envelope
   signature). The node signs it with its active enrolled Ed25519 key. Ackplane
   verifies the signature via the *same* `signing_keys` resolution envelope
   verification already uses, then binds the live stream to that node id,
   tenant id, and repository id for the stream's lifetime. A node with a
   revoked or expired key cannot open a connection at all — the existing
   `KeyResolution::Revoked`/`Expired`/`Retired` refusals apply here before any
   frame is accepted, not only at envelope-verification time.
2. **Transport encryption stays server-side TLS; no client-certificate
   authority is introduced.** ADR-0083's TLS enforcement (`task:f5a6e3a9c2ec`)
   already protects bytes in transit outside loopback. This decision satisfies
   ADR-0084 decision 3's actual requirement — a live connection provably bound
   to an enrolled identity — without a second issuance/rotation/revocation
   ceremony parallel to the one ADR-0085 already built for envelope keys. If a
   future ADR ever needs certificate-based mutual TLS specifically (for a
   client that cannot hold a long-lived application key, for instance), that
   is new scope, decided then, not assumed now.
3. **Human and administrative access stays on the loopback developer profile
   until it needs to be more.** ADR-0094's `BridgeConfig` already refuses any
   non-loopback bind without a production authentication verifier. That
   refusal is correct and does not change. What changes: the developer
   profile's tenant identifier (`ACKPLANE_BRIDGE_DEVELOPMENT_TENANT`) moves
   from a bare comparable string to a value derived from a locally generated
   per-installation salt — `development_tenant_token = hex(SHA-256(salt ||
   tenant_name))`, salt generated once and stored beside the other local
   secrets — so a guessed or logged tenant name alone cannot impersonate the
   operator. This is explicitly a small hardening of a single-operator
   loopback path, not authentication infrastructure, and is named as such so
   nobody mistakes it for decision 1's OIDC requirement being satisfied.
4. **OpenID Connect integration is deferred, not cancelled.** ADR-0084 decision
   1 stands as written for the future: when Ackplane actually serves a second,
   independent organisational tenant, that work gets its own ADR, written
   against that real tenant's actual identity provider, requirements, and
   threat model — not drafted speculatively now against a scenario this
   deployment does not yet have. Until then, `authenticated_principal`
   provenance (ADR-0084 decision 6) is simply unavailable, and any evidence
   contract requiring it correctly reports `provenance requirement unmet`
   (ADR-0084 decision 6) rather than silently downgrading to it.
5. **Tenant/repository authorization for administrative actions is a
   repository-id predicate, not an RBAC system, until decision 4 lands.**
   ADR-0084 decision 9's "every command and query is evaluated against...
   explicit tenant and repository scope" is satisfied today by requiring every
   Bridge query and administrative mutation to carry an explicit
   `repository_id` (already true of `signing_keys` and the ledger tables) and
   refusing any handler that omits it — a lint/test-level guard, not a
   permissions engine. Distinct permissions for policy activation, waiver
   grant, evidence review, fleet operation, and audit read (also decision 9)
   wait for decision 4's authenticated principal, because without one there is
   no subject to grant a permission to — only "this repository's already-
   enrolled operator," which decision 3's loopback profile already gates.
6. **What this ADR does not decide.** `provider_attested` corroboration from
   an external CI/Git source (ADR-0084 decision 6) and the Bridge's
   freshness/completeness view (decision 11) remain their own future work,
   each cut as its own narrow task once this ADR unblocks the coarse umbrella.
   They do not need a design decision the size of connection trust or OIDC —
   they were only stuck behind the umbrella task's blocked status, not behind
   an undrafted design.

## Consequences

- `task:ee95827610cd` can be split into narrow, honestly-scoped tasks: the
  connection-challenge handshake (decision 1), the salted developer-tenant
  token (decision 3), and the repository-id-predicate guard (decision 5) are
  all buildable against already-shipped code, without waiting on OIDC.
- No second PKI, certificate authority, issuance ceremony, or rotation
  schedule is introduced. The one enrolment ceremony ADR-0085 already built
  now authenticates two things (the envelope and the connection) instead of
  one, which is less code than two independent systems would have been.
- Real multi-tenant OIDC federation is explicitly still coming — this ADR
  does not claim decision 1 is satisfied, only that it is correctly deferred
  rather than blocking everything behind it.
- A compromised node key now also grants connection access, not only envelope
  forgery capability. This is not a new exposure: ADR-0085 decision 8 already
  requires revocation to end live authority immediately
  (`task:734246339dd6` terminates live streams on revocation), and that
  termination now covers the connection this decision authenticates with the
  same key.
- The developer-tenant salt (decision 3) is a usability improvement over a
  bare string, not a security boundary; ADR-0094's loopback-only refusal
  remains the actual control until decision 4 delivers a real principal.

## Rejected alternatives

**Build a parallel mTLS certificate authority for connections.** Rejected:
the node already holds a durable, enrolled, revocable Ed25519 identity built
for exactly this purpose. A second certificate-issuance system authenticating
the same node would duplicate ADR-0085's ceremony for no additional trust
property — it would need its own bootstrap, rotation, and revocation path
tracking the *same* lifecycle `signing_keys` already tracks.

**Draft OIDC now, against a hypothetical second tenant.** Rejected: an
identity-federation design is only as good as the real requirements it is
checked against. Writing it against an imagined organisation risks the
opposite failure AGENTS.md warns about — a large, accepted, untested surface
nobody can validate until a real second tenant shows up, at which point the
speculative design likely needs rework anyway.

**Treat the salted developer-tenant token as sufficient authentication.**
Rejected: it is an improvement over a bare string for a single-operator
loopback profile, nothing more. ADR-0094's refusal to bind non-loopback
without a production verifier stays exactly as strict as it already is.
