# ADR-0084: Ackplane evidence has explicit trust

- Status: Accepted
- Date: 2026-08-10
- Deciders: MindLeak maintainers
- Accepted: 2026-08-10 by the repository owner — attributed human adoption after
  review.
- Depends on: [ADR-0082](0082-ackplane-is-a-standalone-federation-service.md)
  (standalone federation boundary),
  [ADR-0083](0083-grpc-is-the-ackplane-node-protocol.md) (node protocol)
- Refined by: [ADR-0085](0085-node-enrolment-requires-proof-of-possession.md)
  (key binding, rotation, and revocation ceremony),
  [ADR-0098](0098-connection-trust-reuses-the-enrolled-key-oidc-waits.md)
  (decisions 1, 3, 9 narrowed to the actual single-operator, multi-repository
  deployment; full OIDC federation deferred)
- Related: [ADR-0009](0009-evidence-backed-conformance.md) (evidence-backed
  verdicts), [ADR-0031](0031-exportable-conformance-evidence.md) (portable
  receipts), [ADR-0054](0054-identity-is-the-session-not-the-process.md)
  (logical local identity),
  [ADR-0071](0071-task-resolution-records-an-unverified-reviewer-label.md)
  (attribution is not authentication)

## Context

The local planes are unauthenticated by design. A registered session token
separates concurrent clients and preserves continuity, but it is not a remote
credential. A reviewer label records what a caller declared; it does not prove
that the named person supplied it. ADR-0071 makes that limitation explicit.

A multi-tenant Ackplane crosses a different boundary. It receives records over
a network, aggregates repositories belonging to different organisations, and
presents evidence to security reviewers and auditors. TLS protects bytes in
transit but does not make a local label an authenticated person, prove that an
agent reported the truth, or preserve origin after the connection closes.

Calling every received record "verified" would make the remote product less
honest than the local one. Treating all records as equally untrusted would also
throw away useful distinctions: an imported historical receipt, an envelope
signed by an enrolled repository node, a CI result fetched from its provider,
and an approval made by an authenticated person carry different provenance.

## Decision

1. **Ackplane authenticates principals; it does not trust display labels.**
   Human access uses an organisation-configured OpenID Connect provider. Service
   access uses short-lived workload credentials. Stable issuer and subject
   identifiers are the principal keys; display names and email addresses are
   mutable presentation fields.

2. **A repository node is explicitly enrolled.** Enrolment binds a tenant id,
   Ackplane repository id, node id, and node-held public signing key through an
   authorised administrative action. The corresponding private key never
   leaves the node and is stored through an operating-system credential facility
   where available. Keys have ids, activation and expiry times, rotation, and
  revocation; revocation never rewrites records signed before it. ADR-0085
  defines the proof-of-possession ceremony and is the only activation path.

3. **Node connections use mutually authenticated TLS.** A short-lived client
   certificate or equivalent workload credential binds the live gRPC connection
   to the enrolled tenant, repository, and node. Connection authentication and
   record signing are separate: mTLS protects and identifies the session, while
   an application signature preserves record origin after transport terminates.

4. **Every publishable record is carried in a signed evidence envelope.** The
   signature covers a domain-separated canonical form containing at least:

   ```text
   schema_version
   tenant_id
   repository_id
   producer_id
   producer_sequence
   occurred_at
   payload_type
   payload_digest
   previous_envelope_digest
   signing_key_id
   ```

   Payload size is bounded before hashing and sending. The producer sequence
   and previous digest make deletion, insertion, replay, and reordering visible
   within one producer stream. A sequence gap is recorded as a gap; Ackplane
   never invents the missing event or closes the chain on the producer's behalf.

5. **Ackplane validates first and then issues an immutable receipt.** A receipt
   binds the envelope digest to its tenant, repository, authenticated connection
   principal, accepted ledger position, and server receive time. Duplicate
   envelopes return the original receipt. A conflicting digest for an existing
   producer sequence is rejected and audited. Accepted envelopes and receipts
   are append-only.

6. **Provenance is categorical, not a trust score.** Each assertion exposes its
   origin class and verification facts. The initial classes are:

   - `unverified_attribution`: imported local history or a free-form label;
   - `enrolled_node`: origin and integrity verified against a node key;
   - `authenticated_principal`: an action made in an authenticated user or
     workload session;
   - `provider_attested`: Ackplane independently validated the result against
     an external source such as a Git or CI provider.

  These are not averaged into a percentage and are not silently ordered from
  bad to good. A versioned clause evidence contract names the accepted
  provenance classes for each required evidence role. An omitted provenance
  requirement preserves local semantics and never implies authentication. A
  local receipt keeps its recorded verdict; if an organisation assurance view
  requires stronger provenance, it reports `provenance requirement unmet`
  beside that receipt rather than rewriting the historical verdict.

7. **A signature proves origin and integrity, not truth.** An enrolled node can
   sign an incorrect observation. Conformance still resolves a clause against
   evidence and control observations; repetition, corroboration, or a valid
   signature cannot turn an observation directly into a verdict or policy.

8. **Authenticated approval is a new record, not a reinterpretation.** A
   Ackplane approval records the OIDC principal, tenant, role, target, reason,
   and timestamp. Existing `resolved_by` and other local reviewer labels remain
   `unverified_attribution` forever. Synchronising them does not upgrade them to
  authenticated human actions. A clause that requires authenticated approval
  names `authenticated_principal` for that evidence role; a distinct local
  label cannot satisfy it.

9. **Authorisation is tenant- and repository-scoped.** Every command and query
   is evaluated against the authenticated principal and explicit tenant and
   repository scope. Policy activation, waiver grant, evidence review, fleet
   operation, and read-only audit are distinct permissions. Tenant identity is
   part of every durable key and query predicate; a missing tenant predicate is
   a test failure, not an administrator convention.

10. **Evidence minimisation precedes upload.** Protobuf payloads are an allowlist
  of typed, bounded fields; no generic environment, terminal-input, source,
  graph, or arbitrary metadata map is accepted. Structural violations are
  rejected with a non-retryable reason and are never silently scrubbed. A
  schema cannot prove an allowed string contains no secret, so nodes still
  redact before serialization and Ackplane treats permitted text as
  sensitive. Raw terminal output, source text, and full graph content remain
  local unless a separately accepted ADR adds a bounded payload type and
  retention contract. Hashes and provider references are preferred when they
  let an auditor resolve the source.

11. **Freshness and completeness remain visible.** The Bridge shows the
    last accepted producer sequence and time, chain gaps, key status, and
    provenance class. A cryptographically valid but stale or incomplete stream
    cannot render as current assurance.

12. **Compromise is appended, never erased.** Key revocation and a later
  compromise finding are durable security events. Existing receipts retain
  the key status known at acceptance and gain a visible later-compromise
    annotation; they are not deleted or silently downgraded. The Bridge
  raises an assurance finding, and affected policy claims require explicit
  re-evaluation under their evidence contracts.

## Consequences

- Ackplane can make a stronger identity claim than the local stdio services
  without falsifying the history produced before authentication existed.
- Auditors can distinguish who delivered a record, whether it changed in
  transit or storage, whether an independent provider corroborated it, and
  whether the evidence contract accepts that provenance.
- The system needs OIDC integration, node enrolment, certificate and signing-key
  rotation, revocation, receipt signing, tenant isolation tests, and an incident
  path for compromised keys.
- Offline enrolled nodes can sign records at creation and upload them later.
  Records created before enrolment are historical imports and remain explicitly
  unverified.
- A compromised but still-valid node key can produce authentic falsehoods.
  Provider attestation, control execution, constitutional consequence, and
  human review remain separate defences.
- Local mode keeps its current trust boundary. It does not need an identity
  provider merely because Ackplane supports one.

## Rejected alternatives

**Treat TLS as evidence attestation.** Rejected because TLS authenticates a live
connection and protects transport. It does not preserve application-level
origin in an exported receipt or prove the observation is correct.

**Use the local `session_id` as a remote credential.** Rejected under ADR-0030
and ADR-0054 because it was designed as an opaque continuity token, is not
issued by a trusted identity provider, and has no tenant, expiry, role, or
revocation semantics.

**Assign every record one numeric confidence score.** Rejected because origin,
integrity, independent corroboration, freshness, and policy sufficiency are
different facts. One score hides which property is absent and invites a
threshold to become policy without constitutional authority.

**Upgrade legacy reviewer labels during import.** Rejected because a matching
directory name does not prove who performed the historical action. Later
authentication cannot travel backwards in time.

**Have Ackplane sign all uploads and call them verified.** Rejected because a
server receipt proves what Ackplane accepted, not who originally produced it
or whether its contents were true. Producer signatures and server receipts
answer different questions.

**Upload all local data so an auditor can decide later.** Rejected because
maximal collection expands the security and privacy boundary without improving
the semantics of a receipt. Evidence contracts should collect the minimum facts
needed to resolve the governed claim.
