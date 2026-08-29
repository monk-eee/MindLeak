- **Ackplane never reports its own view of a supervisor's position, so
  ADR-0116 decision 7's `IncompleteEvidence` case cannot be detected —
  MEASURED 2026-08-29, OPEN.** `service/handshake.rs` builds `HelloAccepted`
  with `accepted_position: last_accepted_position`, sourced straight from
  `hello.last_accepted_position` — the value the client itself just sent. It is
  an echo, not an independent statement.
  The supervisor daemon briefly wired `reconcile(positions,
  connection.accepted_position())` to that value, which compared a number
  against its own reflection and could only ever answer `UpToDate`. That call
  has been removed rather than left reading as a working guard, so nothing now
  claims to detect the case.
  What is lost is real: a supervisor whose durable state was restored from an
  older copy or truncated cannot be told apart from one that has never run,
  because both report position zero and only a peer that remembers can
  distinguish them. `reconcile()` itself is correct, unit-tested, and covered
  by slice 4's integration test — it is the decision function waiting for a
  truthful input, not the defect.
  Closing this means the server reporting its own supervisor position, which is
  a wire-protocol change (a new field or frame) with its own review, not
  something to retrofit onto the daemon. Note the sequence spaces differ:
  `accepted_position` is about the ledger's event stream, while the supervisor
  outbox has its own local frame sequence, so a fix has to say which one it is
  reporting rather than reusing a field that already means something else.
- **The review this asks for is now proposed as
  [ADR-0141](../docs/adr/0141-ackplane-reports-its-own-supervisor-position.md);
  the gap stays OPEN until it is accepted and implemented.** It takes the
  sequence-space warning above as its central constraint: a new
  `supervisor_frame_position` naming the outbox sequence rather than any reuse
  of `accepted_position`, optional so that "never seen this supervisor" stays
  distinguishable from "seen at zero", persisted per
  `(tenant_id, repository_id, supervisor_id)` beside the existing registration
  row, and degrading to today's undetected behaviour rather than to a false
  verdict when either side is older. `reconcile()` is unchanged by it.
- **ADR-0141 is accepted but NOT yet implementable as written — found while
  starting the implementation, 2026-08-30.** It says the server reports "the
  highest supervisor frame sequence it has durably accepted from this
  supervisor". The server is never told that number. The outbox sequence is
  allocated locally in `daemon::enqueue_receipt` as
  `positions().last_enqueued + 1` and appears on no wire field: `DirectiveReceipt`
  carries `directive_sequence`, which is the *server-issued directive's* number,
  not the supervisor's frame sequence, and `SupervisorHeartbeat`/
  `SupervisorSession`/`SupervisorLifecycleReceipt` carry no sequence at all. So
  the receipt has no truthful value to populate.
  Two further details make this worse than a missing field. A server-side
  *count* of accepted frames is not a substitute: it would only coincide with
  the supervisor's sequence through an unstated invariant — that the outbox
  contains exactly directive receipts, forever, and that none is ever accepted
  out of band — and it would diverge silently the moment ADR-0135's outbox
  carries any other frame type. Reporting it anyway would be the confident wrong
  answer ADR-0141 was written to prevent, arrived at by a different route. And
  the receipt cannot ride on `HelloAccepted` as the ADR's context implies, since
  `Hello` identifies only `producer_id`; `supervisor_id` first appears in the
  registration frame.
  **What is actually needed:** the supervisor must stamp its outbox sequence on
  the frames it sends, so the server has something real to record and echo back
  as its own accepted high-water mark. That is an additional outbound wire
  change ADR-0141 does not sanction, and ADRs here are immutable, so it needs a
  superseding or follow-up decision rather than being folded in quietly during
  implementation. The partial implementation (a `supervisor_frame_position`
  field on `SupervisorFrameReceipt`, and a nullable `frame_position` column on
  `supervisor_registrations`) was written, found to have no truthful source, and
  reverted rather than shipped as a field the server could only populate by
  guessing.
