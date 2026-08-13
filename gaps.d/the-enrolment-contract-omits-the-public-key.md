- **The enrolment contract omits the public key it exists to establish, so no
  signature in the system can ever be verified — MEASURED 2026-08-13, left
  OPEN.** ADR-0085 decision 2 states that *"Only the public key and fingerprint
  enter an enrolment request"* — both — and decision 5 requires that
  *"Ackplane verifies the signature"* of the activation challenge. The shipped
  wire contract cannot support either.

  Every message in
  [`proto/mindleak/ackplane/v1/node_sync.proto`](../crates/ackplane-protocol/proto/mindleak/ackplane/v1/node_sync.proto)
  that should carry key material:

  | Message | Key material it carries |
  |---|---|
  | `EnrollmentRequest` | `public_key_fingerprint` only |
  | `EnrollmentChallenge` | `public_key_fingerprint`, `nonce` |
  | `EnrollmentActivationProof` | `public_key_fingerprint`, `nonce`, `signature` |
  | `KeyRotationRequest` | `successor_public_key` — **the only actual key bytes anywhere** |

  A fingerprint is a hash of a key, not a key. The one message that does carry
  key bytes is rotation, which presupposes an already-enrolled key to rotate
  *from*, so the chain has no origin.

  It compounds downstream. `EventEnvelope` carries `signing_key_id` (field 11)
  and `signature` (field 12), and `ledger_records` persists both — so envelopes
  arrive stamped with a key id that nothing can resolve to a key. The shape
  looks finished and verifies nothing.

  There is also nowhere to keep one. `migrations/0001_ledger.sql` and
  `0002_projection.sql` create `stream_heads`, `ledger_records`,
  `ledger_receipts`, `projected_nodes`, `projected_edges` and
  `projection_state`. There is no enrolments table and no keys table, so the
  state machine ADR-0085 decision 1 describes — `unenrolled -> pending ->
  approved -> activating -> active -> rotating|revoked`, every transition
  appended with actor, time, tenant, repository, node, fingerprint and reason —
  has no storage either.

  Impact is bounded today because nothing verifies anything yet: no enrolment
  service is implemented, and `ProvenanceClass` values arrive declared rather
  than proven. The cost is that trust work reads as nearly done when its
  foundation is absent, and the first person to implement verification will get
  as far as resolving a key before discovering it cannot. It already cost one:
  `task:49965c724a51` was created for envelope signature verification and
  blocked an hour later for exactly this.

  Left open because the repair is small but not mine to choose unilaterally:
  adding a public-key field is additive and backward-compatible in proto3 (take
  a fresh field number, never renumber), but *which* messages carry it, and
  whether the fingerprint stays as the thing an administrator approves per
  decision 4, is a contract decision. Tracked as step one of
  `task:c265276db1ba` (Serve NodeEnrollmentService).
