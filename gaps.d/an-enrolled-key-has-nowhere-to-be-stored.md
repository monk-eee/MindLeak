- **An enrolled key has nowhere to be stored, so the enrolment state machine and
  every envelope signature still resolve to nothing — MEASURED 2026-08-13, left
  OPEN.** The wire contract now carries the public key (`EnrollmentRequest.public_key`
  in
  [`node_sync.proto`](../crates/ackplane-protocol/proto/mindleak/ackplane/v1/node_sync.proto)),
  which was the half of this that a contract change could fix. Receiving a key
  Ackplane cannot persist gets no further.

  [`migrations/0001_ledger.sql`](../crates/ackplane-server/migrations/0001_ledger.sql)
  and
  [`0002_projection.sql`](../crates/ackplane-server/migrations/0002_projection.sql)
  create exactly six tables — `stream_heads`, `ledger_records`,
  `ledger_receipts`, `projected_nodes`, `projected_edges`, `projection_state`.
  There is no enrolments table and no keys table. So:

  - The state machine of ADR-0085 decision 1 —
    `unenrolled -> pending -> approved -> activating -> active -> rotating|revoked`,
    every transition appended with actor, time, tenant, repository, node,
    fingerprint and reason — has no storage.
  - `EventEnvelope.signing_key_id` (field 11) is persisted by `ledger_records`
    alongside `signature` (field 12), but no table maps a key id to a key. An
    envelope arrives stamped with an identifier that resolves to nothing.
  - ADR-0084's requirement that a previously accepted envelope stay resolvable
    *with its key status at acceptance* needs the key history that would be in
    that missing table.

  Impact is still bounded because nothing verifies anything yet, and
  `ProvenanceClass` values arrive declared rather than proven. The cost is
  unchanged in kind: trust work reads as nearly done while its foundation is
  absent. `task:49965c724a51` (verify an envelope signature before the ledger
  appends it) remains blocked on this, not on the contract.

  Left open rather than fixed here because the schema is not a small additive
  choice the way a proto field was. It has to express approval attribution,
  fingerprint-at-approval, expiry, rotation overlap, and revocation with the
  status-at-acceptance history ADR-0084 depends on — and it is the storage half
  of `task:c265276db1ba` (Serve NodeEnrollmentService), which should decide it
  once rather than have it accreted field by field.

  Supersedes the narrower `the-enrolment-contract-omits-the-public-key`
  fragment, whose titled defect is now fixed.
