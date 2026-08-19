- **`ActivateEnrollment` never returns the server-assigned `signing_key_id` to
  the node that just activated — OBSERVED 2026-08-19, left OPEN.**
  `EnrollmentActivationResult` (`crates/ackplane-protocol/proto/mindleak/ackplane/v1/node_sync.proto`)
  carries `request_id`, `state`, `enrolment_receipt_id`, `rejection_reason`,
  and `diagnostic` — no `signing_key_id`, even though `activate()`
  (`crates/ackplane-server/src/enrollment_store.rs`) generates one and stores
  it (`new_signing_key_id()` in `enrollment_service.rs`) as part of the same
  activation. The node that just proved possession of its own key has no
  contract-level way to learn the id the server just minted for it, even
  though every later RPC that uses that key (`Hello.signing_key_id`,
  `EventEnvelope.signing_key_id`) requires it.

  `crates/ackplane-client/examples/enroll_and_sync.rs` (this session) works
  around it the only way currently possible: after activation, query
  `signing_keys` directly by `public_key_fingerprint`, ordered by
  `activated_at DESC LIMIT 1` — a database read a real external node could
  never perform, and a race if the same fingerprint were ever activated
  twice in quick succession (unlikely today, but the query has no
  activation-attempt correlation beyond recency).

  Impact: every genuine client of `NodeSyncService.Synchronize` needs a
  `signing_key_id` to send `Hello` at all, so this gap forces every real
  integration onto the same server-side-only workaround this example uses,
  or onto a manual copy-paste from an operator's own database access. It is
  the reason this example still needs `ackplane-server` as a dependency for
  more than just the (deliberately server-side) approval step.

  Left OPEN: no fix attempted this run. The right-sized fix is adding
  `signing_key_id` to `EnrollmentActivationResult` and returning it from
  `activate()`'s existing `EnrollmentActivation` result -- the value already
  exists at the point the response is built; this is a wire-contract
  addition (new field, additive per ADR-0059 decision 5), not new logic.
