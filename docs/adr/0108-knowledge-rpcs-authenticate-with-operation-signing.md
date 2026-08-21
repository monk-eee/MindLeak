# ADR-0108: Knowledge RPCs authenticate with an operation-signing scheme mirroring claims

- Status: Accepted
- Date: 2026-08-20
- Deciders: MindLeak maintainers
- Accepted: 2026-08-20 by the repository owner, authorized directly in
  session - attributed human adoption after review.
- Depends on: [ADR-0096](0096-ackplane-arbitrates-federated-claims-through-leased-delegation.md)
  (Ackplane arbitrates federated claims through leased delegation),
  [ADR-0100](0100-repository-node-owns-one-non-exporting-signer.md) (the
  repository node owns one non-exporting signer)
- Related: `gaps.d/ackplane-knowledge-service-rpcs-are-unauthenticated.md`

## Context

ADR-0106 added a PostgreSQL-backed `KnowledgeService` (`RecordKnowledge`,
`RecallKnowledge`, `RetireKnowledge`) to `ackplane-server`, the first slice of
the Industrial knowledge domain. It shipped deliberately unauthenticated: no
request is bound to an enrolled node's signing key, and no nonce or
timestamp guards a replay. This was a scope decision made explicit in the
gap fragment filed alongside it, not an oversight — reusing
`ClaimOperation`/`CLAIM_DOMAIN` (`crates/ackplane-protocol/src/claim_auth.rs`)
directly would have been a domain mismatch, since its fields (`branch`,
`lease_seconds`, `expected_owner`, ...) have no equivalent for a knowledge
statement.

ADR-0100 decision 10 already solved the general problem this RPC surface has:
identity proof alone (which key signed this) is not authorization of an exact
mutation. A signature covering only tenant/repository/node/key identity
verifies identically for two different requests that change different fields,
so ADR-0100 bound a distinct operation tag and every operation-specific field
into the signed bytes, added a bounded clock-skew window on `signed_at`, and
consumed a client-selected `(signing_key_id, nonce)` pair exactly once through
a durable table before the request's own effect could apply. That decision's
own text scopes itself to "claim authentication" — it does not, by its
wording, extend to any other Ackplane RPC surface, and no ADR currently makes
that extension explicit for `KnowledgeService`.

The gap is real and growing: any caller that can reach `ackplane-server`'s
gRPC port can record, recall, or retire knowledge for any `(tenant_id,
repository_id)` pair it names, impersonating any enrolled node with no proof
required. That is the same trust boundary the claim RPCs closed under
ADR-0100 decision 10 and `ClaimDelegationService`'s `authenticate()`; knowledge
is simply the one mutating domain that does not yet sit behind it.

## Decision

**`KnowledgeService`'s three mutating RPCs authenticate with the same
operation-signing mechanism ADR-0100 decision 10 established for claims,
mirrored into its own domain rather than sharing the claim domain's bytes,
keys, or nonce space.**

1. **A `KnowledgeOperation` enum, structurally parallel to `ClaimOperation`.**
   `crates/ackplane-protocol/src/knowledge_auth.rs` defines `Record { content:
   &str, half_life_hours: f64, embedding_model: Option<&str> }`, `Recall {
   query_embedding_present: bool, limit: u32 }`, and `Retire { knowledge_id:
   &str, reason: &str }` variants, each binding its own operation-specific
   fields the same length-delimited way `ClaimOperation::push_fields` does.
   `Recall` binds whether a query embedding was supplied and the limit, not
   the embedding vector itself — the vector is large, and a read RPC's
   authentication only needs to bind what the caller is asking to be shown
   under, not the ranking input verbatim; a later amendment may tighten this
   if replay-forged recall queries prove to be a real concern.

2. **A domain separator distinct from `CLAIM_DOMAIN`.** `KNOWLEDGE_DOMAIN =
   b"mindleak.ackplane.v1.knowledge\0"`. A signature produced for a claim can
   never verify as a knowledge authentication and vice versa, even if every
   other field happened to coincide, exactly as `CLAIM_DOMAIN` already
   separates claim signatures from any other Ackplane signing use.

3. **Its own nonce table, not the claim table.** A `knowledge_authentication_nonces`
   table (`signing_key_id`, `nonce`, `consumed_at`), consumed the same way
   `ClaimStore::consume_claim_nonce` does today (`INSERT ... ON CONFLICT
   (signing_key_id, nonce) DO NOTHING`, checking rows-affected). A knowledge
   nonce and a claim nonce are unrelated pairs; sharing one table would let a
   captured claim authentication's nonce silently pre-empt a legitimate
   knowledge request using the same nonce value under the same key, or vice
   versa — a coincidence a shared table could not distinguish from a real
   collision.

4. **The same identity resolution and verification shape.** `crates/
   ackplane-server/src/knowledge_signature.rs` mirrors `claim_signature.rs`:
   a pure `verify()` taking the resolved `KeyResolution` (the existing
   `signing_keys` lookup, unchanged — this is not a new key registry, only a
   new consumer of the one that already exists), the bounded clock-skew
   check on `signed_at`, and Ed25519 verification of the canonical bytes. The
   same `ClaimAuthRefusal`-shaped enum (renamed for this domain) reports
   `Unsigned`/`Unidentified`/`UnknownKey`/`BindingMismatch`/`KeyNotInForce`/
   `Revoked`/`BadSignature`/`MalformedTimestamp`/`StaleTimestamp`/`Replayed`.

5. **A `KnowledgeAuthentication` message on the wire, structured like
   `ClaimAuthentication`.** `signing_key_id`, `node_id`, `signed_at` (RFC3339),
   `nonce` (client-selected random bytes), `signature`. Added as a field on
   `RecordKnowledgeRequest`/`RecallKnowledgeRequest`/`RetireKnowledgeRequest`
   in `node_sync.proto`. This is additive to a domain that has not yet
   reached a tagged release — no existing caller depends on the current
   unauthenticated shape, so there is no backward-compatibility break to
   manage, unlike the claim RPCs which were authenticated after already
   shipping unauthenticated for a period.

6. **No dependency on the `ackplane-node` companion process.** ADR-0100
   decisions 1-9 describe a repository-side companion that owns the signer
   and constructs closed domain requests before signing them; that companion
   does not exist in the workspace yet. This ADR does not wait for it.
   `claim_signature.rs` already verifies claim authentications purely
   server-side against `ackplane-server`'s own resolved `signing_keys`
   registry, with no dependency on how the client assembled or signed the
   bytes; `knowledge_signature.rs` verifies the same way. Whatever
   eventually produces a `KnowledgeAuthentication` client-side — today's
   direct client, or a future `ackplane-node` companion — is a client-side
   concern this ADR does not have to resolve.

7. **Tests mirror `claim_service.rs`'s sabotage suite.** A sabotage test per
   refusal reason (change the operation tag, a bound field, `signed_at`, or
   the nonce, and confirm the specific `KnowledgeAuthRefusal` variant fires,
   not just "some error"), a nonce-replay test proving the identical
   `(signing_key_id, nonce)` pair is refused the second time, and a
   cross-operation test proving a `Record` authentication does not verify
   for a `Recall`/`Retire` naming the same identity fields.

## Consequences

- `KnowledgeService`'s three RPCs gain a required `KnowledgeAuthentication`
  field and reject any request that omits or fails one, closing the
  impersonation gap `gaps.d/ackplane-knowledge-service-rpcs-are-unauthenticated.md`
  named. Any existing direct caller of the unauthenticated RPCs (there are
  none outside this repository's own tests and examples, since the domain
  has not been released) must start signing requests.
- A second nonce table and a second domain-separated signing scheme are now
  the house pattern for authenticating a mutating Ackplane RPC surface. A
  future third domain (e.g. context packets, decisions, directives from
  ADR-0106 decision 3's remaining product work) is expected to add its own
  `<domain>_auth.rs` / `<domain>_signature.rs` / `<domain>_authentication_nonces`
  triple the same way, rather than generalizing into one shared abstraction
  now — two data points are not enough to safely extract the right
  generalization, and a premature one would need to guess which future
  domain's fields it must accommodate.
- Implementation (the proto field, `knowledge_auth.rs`, `knowledge_signature.rs`,
  the nonce table migration, and wiring into `knowledge_service.rs`'s three
  handlers) is separate, larger work gated on this ADR's acceptance — not
  included in this change.

## Rejected alternatives

**Reuse `ClaimOperation` and `CLAIM_DOMAIN` directly for knowledge requests.**
Rejected for the same reason the original `KnowledgeService` gap named:
`ClaimOperation`'s fields (`branch`, `lease_seconds`, `expected_owner`, paths,
symbols) have no equivalent for a knowledge statement, so binding a knowledge
request's real fields would mean inventing placeholder claim fields to fill,
which signs nothing meaningful and invites a reviewer to trust a signature
that does not actually cover what changed.

**Share one nonce table across every signed domain.** Rejected because a
`(signing_key_id, nonce)` pair is only unique within the space it is drawn
from; sharing the table couples an unrelated domain's nonce budget to
claims' and makes a coincidental cross-domain nonce collision (rare, but
possible if either side's nonce generation is not perfectly random) refuse a
legitimate request in one domain because of unrelated traffic in another.

**Wait for the `ackplane-node` companion (ADR-0100 decisions 1-9) before
authenticating knowledge requests.** Rejected because server-side
verification against `ackplane-server`'s own resolved signing-key registry
does not need that companion to exist — `claim_signature.rs` proves this
today. Gating knowledge authentication on an unrelated, unbuilt subsystem
would leave a known, real gap open for a dependency it does not actually have.

**Generalize immediately into one shared `OperationAuthentication` type
parameterized over the domain.** Rejected as premature abstraction with one
prior example (claims) and one new one (knowledge): the two operation enums
already differ in shape (list-valued fields for claims, a float and an
optional string for knowledge), and forcing a shared generic now risks
designing it around the wrong two data points. Mirroring the pattern by hand
a second time, and revisiting generalization once a third domain exists, is
the more honest sequencing.
