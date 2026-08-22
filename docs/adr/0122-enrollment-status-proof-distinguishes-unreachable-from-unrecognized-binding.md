# ADR-0122: Enrollment-status proof distinguishes unreachable from unrecognized binding

- Status: Proposed
- Date: 2026-08-22
- Deciders: Pending human acceptance
- Depends on: [ADR-0085](0085-node-enrolment-requires-proof-of-possession.md)
  (node enrolment requires proof of possession),
  [ADR-0098](0098-connection-trust-reuses-the-enrolled-key-oidc-waits.md)
  (connection trust reuses the enrolled node key),
  [ADR-0100](0100-repository-node-owns-one-non-exporting-signer.md) decision
  10 (operation-specific claim signing), [ADR-0108](0108-knowledge-rpcs-authenticate-with-operation-signing.md)
  (the mirrored-domain operation-signing house pattern this ADR follows a
  second time)
- Related: `gaps.d/ackplane-client-cannot-detect-unenrolled-repositories.md`,
  [ADR-0082](0082-ackplane-is-a-standalone-federation-service.md) (Ackplane is
  a standalone federation service)

## Context

`ackplane-core::compiled_federation_readiness` resolves a `federated`
repository's usability to one of `Ready`, `NoClient`, `ArbiterUnreachable`, or
`NotEnrolled`. Only the first three are reachable today.
`ackplane_client::probe_reachable` opens a bare transport connection and
nothing else; `ackplane-protocol`'s wire contract has no RPC that answers "is
this exact repository enrolled with this deployment." A repository whose
Ackplane deployment is healthy but has simply never enrolled it resolves
identically to one whose deployment is down: `ArbiterUnreachable`, with a
remedy text — "check the deployment, or declare local" — that is actively
misleading when the real answer is "the deployment is fine, nobody enrolled
this repository yet."

Closing that gap is not as simple as adding a bare `IsEnrolled(tenant_id,
repository_id)` query. `NodeEnrollmentService` already treats enrollment as
identity-scoped, not repository-scoped: ADR-0085 decision 3 binds a pending
request to an exact `(tenant, repository, node, key)` tuple, because a
repository can have more than one node and a node's identity is its key, not
its label. A query that only asks about a repository would tell any caller
who can merely reach the port whether *any* node has ever enrolled that
tenant's repository, and — worse — a per-node variant that answered based on
node id and tenant/repository alone would let a caller enumerate which node
ids exist, which are pending versus revoked, and which repositories a tenant
has, all without proving it holds anything. ADR-0085's whole ceremony exists
because knowing a name is not holding a key; a status query that can be asked
by name alone would reopen exactly the trust-on-first-use problem decision 1
was written to close, just moved from the write side to the read side.

The node asking this question, though, is never a stranger to the identity it
is asking about. ADR-0085 decision 2 has it generate its Ed25519 signing key
*before* any enrollment request exists, and decision 3 has it propose its own
candidate node id in that request. A node checking its own status always
already holds the exact key whose binding it wants to know about — including
a node that has never contacted Ackplane at all, which still holds a freshly
generated local key with nothing enrolled yet. That is the proof this ADR
requires before any binding-specific fact leaves the server: not identity by
assertion, but the same signed proof of possession ADR-0085's activation
ceremony and ADR-0100 decision 10's claim authentication already require
before they act on a claimed identity.

## Decision

**`NodeEnrollmentService` gains one read-only RPC, `CheckEnrollmentStatus`,
gated by a proof-of-possession signature scoped to the exact binding being
asked about. Every failure to verify that proof — a binding that does not
exist, a candidate key that does not match the one on file, or a signature
that does not verify — returns the identical generic result. Only a
successfully verified caller learns its binding's real lifecycle state.**

1. **The request names an exact candidate binding, not a repository.**
   `EnrollmentStatusRequest` carries `tenant_id`, `repository_id`,
   `candidate_node_id`, and `candidate_key_fingerprint` — the same
   self-computed fingerprint ADR-0085 decision 3 already puts in a pending
   enrollment request. A node uses its *own* proposed or already-enrolled
   identity here; nothing in this RPC accepts or needs a node id it does not
   itself hold the key for.

2. **The fingerprint, not `signing_key_id`, is the binding key — because a
   fresh node does not have the latter yet.** `signing_key_id` is assigned by
   Ackplane at successful activation (the same field ADR-0085's activation
   result already returns). A node that has never submitted an enrollment
   request has no `signing_key_id` to present, only the fingerprint of the
   key it generated locally. `EnrollmentStatusAuthentication` is therefore
   `{ node_id, key_fingerprint, signed_at, nonce, signature }` — structurally
   parallel to `ClaimAuthentication`/`KnowledgeAuthentication` (ADR-0100
   decision 10, ADR-0108 decision 5), but keyed on the fingerprint the caller
   can always compute rather than a server-assigned id it may not have yet.

3. **A domain-separated `EnrollmentStatusOperation`, mirrored rather than
   shared, following ADR-0108's own precedent for a second domain.**
   `ENROLLMENT_STATUS_DOMAIN = b"mindleak.ackplane.v1.enrollment_status\0"`
   is distinct from `CLAIM_DOMAIN` and `KNOWLEDGE_DOMAIN`; a signature
   produced for one can never verify as another. The canonical signed bytes
   bind the operation tag plus `tenant_id`, `repository_id`,
   `candidate_node_id`, and `candidate_key_fingerprint` — every field the
   result depends on, so a captured signature cannot be replayed to ask about
   a different binding, exactly as ADR-0100 decision 10 requires for claims.

4. **Its own nonce table, consumed exactly like the claim and knowledge
   domains'.** `enrollment_status_authentication_nonces (signing_key_id_or_fingerprint,
   nonce, consumed_at)`, atomically consumed with `INSERT ... ON CONFLICT DO
   NOTHING` before any lookup runs, and a bounded clock-skew window on
   `signed_at`. This RPC is read-only, so replaying a captured request cannot
   forge a mutation — but an unbounded replay would still let a party who
   once captured a valid signed request poll that binding's lifecycle state
   indefinitely without ever holding the key again, which is a real, if
   modest, freshness leak this house pattern already knows how to close.
   Sharing the claim or knowledge nonce table is rejected for the same
   reason ADR-0108 gave: an unrelated domain's nonce traffic could
   coincidentally pre-empt a legitimate request in this one.

5. **Verification precedes disclosure, and every failure looks identical.**
   The server: (a) looks up whether `(tenant_id, repository_id,
   candidate_node_id)` has a binding at all; (b) if it does, compares its
   on-file key fingerprint against `candidate_key_fingerprint`; (c) if that
   matches, verifies the signature against the on-file public key within the
   clock-skew window and consumes the nonce. Steps (a), (b), and (c) each
   fail for a different reason, but every one of them returns the exact same
   `EnrollmentBindingState::NOT_ENROLLED` result with no distinguishing
   detail — an absent binding, a wrong fingerprint, an expired timestamp, a
   replayed nonce, and a forged signature are one outcome on the wire, not
   five. A caller who is not this exact node cannot learn that a binding
   merely *exists* under a different lifecycle state; it learns nothing more
   than it would about a binding that was never created.

6. **A verified caller learns its own real state, in full.** Once (a)-(c)
   all succeed, the caller has proven it holds the key bound to that exact
   identity — it is not a stranger asking about someone else's binding
   anymore, it is the binding. `EnrollmentBindingState` reports the ADR-0085
   decision 1 state machine directly: `PENDING`, `APPROVED`, `ACTIVATING`,
   `ACTIVE`, `ROTATING`, or `REVOKED`. A revoked node's key still lets it
   learn it is revoked (ADR-0085 decision 8: revocation ends new authority,
   it does not confiscate the key or forbid reading old evidence); this RPC
   grants no new authority, so surfacing that diagnosis to a still-key-holding
   but revoked caller is safe and operationally useful, not a re-grant.

7. **Only `ACTIVE`, confirmed by a live connection, maps client readiness to
   `Ready`.** `CheckEnrollmentStatus` answers the binding question; ADR-0098
   decision 1 already answers the connection question. A client that receives
   `ACTIVE` still opens the authenticated `Synchronize` connection before
   calling itself `Ready`, exactly as ADR-0085 decision 6 already requires —
   this ADR does not shortcut that confirmation. `PENDING`/`APPROVED`/
   `ACTIVATING`/`ROTATING`/`REVOKED`, and the uniform `NOT_ENROLLED`, all map
   to `FederationReadiness::NotEnrolled`; the *remedy text* the client shows
   may differ per state for its own operator (the node already knows which
   one it received), but the coarse three-way client enum this ADR is closing
   the gap for does not need a fourth member to carry that distinction.

8. **A transport failure on this RPC is `ArbiterUnreachable`, never
   `NotEnrolled`.** If the call itself cannot complete — connection refused,
   timeout, TLS failure — that is unchanged from today's `probe_reachable`
   failure and must not be reinterpreted as a verified "not enrolled" answer.
   Only a *completed* RPC exchange, successful or refused, resolves to an
   enrollment-status outcome. Conflating a network blip with "this repository
   was never enrolled" would send an operator chasing the wrong remedy.

9. **No Bridge or browser shortcut.** This RPC is reachable only by
   presenting a valid proof of possession for the exact binding asked about;
   there is no administrative bypass that answers it on a caller's behalf.
   An authenticated Bridge administrative view may separately show enrollment
   records for support and audit (already an authorized, tenant-scoped
   administrative capability against the enrollment store, not this RPC);
   that is a different, already-gated capability, not a substitute for a
   node proving its own binding.

10. **Tests mirror `claim_service.rs`'s and `knowledge_service.rs`'s sabotage
    suites.** One sabotage test per collapsed failure (absent binding, wrong
    fingerprint, stale timestamp, replayed nonce, forged signature) proving
    all five produce the byte-identical `NOT_ENROLLED` result; a same-caller
    positive test proving a verified query against each lifecycle state
    returns that exact state; a cross-domain test proving an
    `EnrollmentStatusAuthentication` signed for one candidate binding does not
    verify against a different `(tenant_id, repository_id,
    candidate_node_id)` tuple; and a cross-operation test proving a claim or
    knowledge authentication does not verify here and vice versa.

## Consequences

- `FederationReadiness::NotEnrolled` becomes reachable from a real signal
  instead of only existing as an enum variant nothing produces, and the
  remedy an operator sees can finally say "this repository was never
  enrolled with this deployment" instead of the misleading "check the
  deployment, or declare local" for a deployment that was fine all along.
- A caller that is not the exact node it is asking about learns nothing —
  not repository existence, not which other nodes exist, not their states —
  matching decision 3's binding scoping and closing the enumeration surface
  a bare `IsEnrolled` query would have opened.
- A third domain-separated nonce table joins the claim and knowledge ones,
  confirming the mirrored-domain shape ADR-0108 named as the expected house
  pattern rather than a one-off.
- Implementation — the proto messages and RPC, `enrollment_status_auth.rs`,
  `enrollment_status_signature.rs`, the nonce table migration, wiring into
  `NodeEnrollmentService`, and `ackplane-client`/`compiled_federation_readiness`
  calling it after a successful transport probe — is separate, larger work
  gated on this ADR's acceptance, matching how ADR-0108 itself deferred its
  own implementation.
- `ackplane-client-cannot-detect-unenrolled-repositories.md` can close once
  that implementation ships and is verified against a real, non-enrolled
  repository and a real revoked one, not merely against the wire contract.

## Rejected alternatives

**A bare `IsEnrolled(tenant_id, repository_id)` query with no per-node
binding or proof.** Rejected: it would tell any caller that can merely reach
the port whether a tenant's repository has ever been enrolled by anyone,
without the caller proving it is that repository's node. This is exactly the
enumeration surface decision 1 exists to close, only moved to the read side
of the ceremony ADR-0085 already protects on the write side.

**Return a distinguishable reason for each verification failure (binding
absent vs. wrong key vs. bad signature vs. replay).** Rejected as the precise
side channel this ADR exists to close. Telling a caller *which* way its guess
failed lets it narrow a search across candidate node ids or keys one bit at a
time; a uniform result leaks nothing beyond "you did not prove this."

**Reuse `ClaimOperation`/`CLAIM_DOMAIN` or `KnowledgeOperation`/
`KNOWLEDGE_DOMAIN` directly.** Rejected for the same reason ADR-0108 gave for
not reusing claims for knowledge: neither operation's fields correspond to a
binding-status check's real fields (a repository/node/fingerprint tuple, not
a branch and lease, not recorded content), so forcing a fit would sign
placeholder fields that mean nothing and invite a reviewer to trust a
signature that does not cover what is actually being asked.

**Key this operation on `signing_key_id` like `ClaimAuthentication` and
`KnowledgeAuthentication` do.** Rejected: both of those operations only ever
happen after activation, when a `signing_key_id` is guaranteed to exist. This
operation must also work for a node that has never enrolled at all, which
has generated a key but has no server-assigned id for it yet — only the
fingerprint it computed locally is available at every point in the
lifecycle this check needs to answer for, including before the first
enrollment attempt.

**Skip nonce/replay protection because the RPC is read-only.** Rejected for
consistency with the established house pattern and because it is not free of
cost: an unbounded replay of a once-captured signed request would let a party
who is no longer in possession of the key continue polling a binding's
lifecycle state indefinitely. A dedicated nonce table closes that the same
way the mutating domains already do.

**A server-issued challenge round trip (mirroring `GetActivationChallenge`)
instead of a client-selected nonce and timestamp.** Considered, and left as
the documented fallback ADR-0100 decision 10 itself names for claims: a
client-selected nonce preserves a one-round-trip status check, which matters
more here than for activation, since a repository may poll this on every
startup. If durable nonce consumption ever proves impractical in practice, a
later amendment may adopt a server challenge and accept the extra round trip,
exactly as ADR-0100 already reserves the right to do for claims.

**Let an authenticated Bridge session answer "is this repository enrolled"
on the node's behalf.** Rejected: that would let anyone who can reach a
Bridge login answer a question this ADR requires the actual key-holder to
prove. A Bridge administrative view over the enrollment store is a different,
already-authorized capability (support and audit, gated by its own
administrative authentication) — not a channel through which a node's own
self-check can be shortcut.
