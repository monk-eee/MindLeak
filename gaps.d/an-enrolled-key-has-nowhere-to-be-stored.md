- **An enrolled key has no registry, so `EventEnvelope.signing_key_id` still
  resolves to nothing — NARROWED 2026-08-14, still OPEN.** Enrolment storage has
  landed since this was filed; the remaining gap is smaller and more specific
  than the heading above it used to claim. Both halves matter: a reader who
  trusts the old text rebuilds work that exists, and a reader who hears
  "enrolment landed" claims a task that is still walled.

  **Landed, and no longer part of this gap.**
  [`migrations/0003_enrollment.sql`](../crates/ackplane-server/migrations/0003_enrollment.sql)
  (PR #451) creates `enrollment_requests`, `enrollment_transitions`,
  `activation_challenges` and `enrollment_receipts`. `enrollment_requests`
  persists `public_key BYTEA NOT NULL` beside `public_key_fingerprint` and
  `state`, and `enrollment_transitions` is the append-only actor/time/reason
  audit ADR-0085 decision 1 asked for. The state machine has storage, and an
  enrolling node's key is durable. The wire half landed earlier still
  (`EnrollmentRequest.public_key`, PR #446).

  **Still open: there is no key registry.** ADR-0084 decision 2 requires that
  "Keys have ids, activation and expiry times, rotation, and revocation", and
  that a previously accepted envelope stay resolvable _with its key status at
  acceptance_. What exists is a key attached to an enrolment _request_, which is
  a different object: no key id, no activation or expiry, no rotation overlap,
  and no status history to resolve an old envelope against.

  - `EventEnvelope.signing_key_id` (field 11) is persisted by `ledger_records`
    beside `signature` (field 12) and maps to nothing. `git grep signing_key_id`
    finds it only in the proto, the ledger row, `sync.rs`'s `translate`, and test
    fixtures — no code resolves it.
  - Nothing anywhere defines that a `signing_key_id` _is_ a
    `public_key_fingerprint`. Even scanning `enrollment_requests` would be
    guessing at the join, and the column carries no uniqueness constraint that
    would make one active key the answer.

  Impact is still bounded because nothing verifies anything yet and
  `ProvenanceClass` values arrive declared rather than proven. The cost is
  unchanged in kind: trust work reads as nearly done while the object it
  depends on is absent.

  `task:49965c724a51` (verify an envelope signature before the ledger appends
  it) remains blocked on exactly this. Its recorded reopen condition — "when an
  enrolled node's public key can be resolved from its `signing_key_id`" — is
  still unmet, and a partial clearance is not a clearance.

  Left open rather than fixed here because the registry is a schema decision
  ADR-0084 decision 2 constrains: key id, activation, expiry, rotation overlap,
  revocation, and status-at-acceptance history. It deserves one deliberate
  decision rather than being accreted a column at a time.

  Supersedes the narrower `the-enrolment-contract-omits-the-public-key`
  fragment, whose titled defect is now fixed.
