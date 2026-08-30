# ADR-0146: A supervisor declares its own frame sequence; the server only echoes what it accepted

- Status: Proposed
- Date: 2026-08-30
- Deciders: MindLeak maintainers (proposed in session; awaiting repository-owner
  review per this repo's adoption convention)
- Refines: [ADR-0141](0141-ackplane-reports-its-own-supervisor-position.md)
  (supplies the inbound half ADR-0141 assumed but did not specify)
- Depends on: [ADR-0135](0135-a-directive-receipt-survives-a-dropped-connection.md)
  (the durable outbox whose sequence this puts on the wire),
  [ADR-0116](0116-enrolled-supervisors-are-the-distributed-agent-runtime.md)
  decision 7 (the reconciliation this makes reachable)
- Related: [ADR-0083](0083-grpc-is-the-ackplane-node-protocol.md) (the wire this
  changes), [ADR-0084](0084-ackplane-evidence-has-explicit-trust.md) (why a
  number nobody asserted is not evidence)

## Context

ADR-0141 decided that Ackplane must report **its own** view of a supervisor's
outbox frame position instead of echoing back the client's declared
`last_accepted_position`, so ADR-0116 decision 7's `IncompleteEvidence` arm
stops being unreachable. That decision stands.

Implementing it revealed that it specified only the **outbound** half. It says
the server reports "the highest supervisor frame sequence it has durably
accepted from this supervisor" — but nothing ever tells the server that
sequence, so there is no truthful value to report.

The outbox sequence is allocated purely locally, in
`daemon::enqueue_receipt`, as `positions().last_enqueued + 1`. It appears on no
wire field. `DirectiveReceipt` carries `directive_sequence`, which is the
*server-issued directive's* number and unrelated. `SupervisorHeartbeat`,
`SupervisorSession`, and `SupervisorLifecycleReceipt` carry no sequence at all.

A server-side **count** of accepted frames looks like a substitute and is not
one. It coincides with the supervisor's sequence only through an unstated
invariant — that the outbox contains exactly directive receipts, forever, and
that none is ever accepted out of band — and diverges silently the moment
ADR-0135's outbox carries another frame type, which ADR-0135's own module doc
anticipates. That divergence would produce a confident wrong answer, which is
the failure ADR-0141 exists to prevent, reached by a different route.

One smaller correction. ADR-0141's context implies the value rides on
`HelloAccepted`. It cannot: `Hello` identifies only `producer_id`, and
`supervisor_id` first appears in the registration frame. ADR-0141's decision
names a field rather than a message, so this refines rather than contradicts it.

## Decision

**The supervisor states its own outbox frame sequence on every frame that
sequence covers, and the server's report is an acknowledgement of what it
accepted — never a number the server invented.**

1. **Outbound supervisor frames carry `outbox_sequence`.** Every frame the
   supervisor enqueues in its durable outbox is stamped with the sequence that
   outbox assigned it. Today that is `DirectiveReceipt` alone; the field is
   defined on each frame the outbox may carry rather than on one of them, so
   widening the outbox later does not require a second wire decision.

2. **A frame sent outside the outbox carries no sequence, and that is
   meaningful.** Registration, session announcement, and heartbeat are
   idempotent and re-sent fresh on every `serve_once` (ADR-0135's reasoning for
   not making them durable). They are not outbox frames, so they have no
   sequence, and the server must not infer one for them. Absent means "this
   frame is not part of the resendable stream", not "sequence zero".

3. **The server records the maximum accepted, per
   `(tenant_id, repository_id, supervisor_id)`, and never derives it.** It
   advances only on durable acceptance of a frame that carried a sequence, and
   only upward. A frame with no sequence advances nothing. The server never
   counts, never interpolates a gap, and never writes a value the supervisor
   did not state — which is what keeps ADR-0141's "server-authored" property
   honest rather than turning it into a longer echo.

4. **The answer rides on `SupervisorFrameReceipt`, not `HelloAccepted`.** The
   receipt answering the registration frame is the first point at which the
   server knows which supervisor it is talking to. ADR-0141's optionality is
   preserved exactly: omitted means "no independent statement", `0` means "seen
   this supervisor, accepted nothing", and the two are never collapsed.

5. **Both directions degrade to silence, never to a verdict.** A supervisor
   that sends no sequence, or a server that reports none, leaves the
   reconciliation unrun — today's undetected behaviour — rather than
   substituting a default. `reconcile()` is still unchanged, and still receives
   two independently asserted numbers or none at all.

## Consequences

- ADR-0141 becomes implementable, and `IncompleteEvidence` becomes reachable
  for the case it was written for: a supervisor whose durable record was
  restored from an older copy or truncated is contradicted by a peer that
  remembers more than it does.
- The wire gains one field per outbox-carried frame plus the reply field
  ADR-0141 already specified. Both halves must land together; the outbound half
  alone changes nothing, and the reply half alone is what this ADR exists to
  stop.
- The outbox's sequence becomes part of the protocol contract rather than a
  private implementation detail, so changing how it is allocated is now a wire
  concern. That is the honest cost of making it verifiable by someone else.
- A supervisor can now be told its own state is behind reality. Nothing here
  decides what it should do about that beyond ADR-0116 decision 7's existing
  "report gaps rather than resume" — deliberately, because adopting the
  server's position is a recovery decision with its own consequences.

## Rejected alternatives

**Have the server count the supervisor frames it accepts.** Rejected: it
matches the supervisor's sequence only by an unstated coincidence, and breaks
silently when the outbox widens beyond directive receipts. It also re-creates
ADR-0141's original defect in reverse — a number the supervisor never asserted,
presented to it as independent evidence.

**Derive the position from the ledger's `producer_sequence`.** Rejected: the
outbox carries supervisor frames, not `EventEnvelope`s. A supervisor that
receipts directives and publishes no events has a growing outbox sequence and
no ledger presence at all, so there is nothing to derive from.

**Put `supervisor_id` on `Hello` so the answer can ride on `HelloAccepted`.**
Rejected: NodeSync serves producers that are not supervisors, and a node may
host more than one supervisor. Widening the handshake to carry a role-specific
identity to save one round trip trades a clean protocol boundary for a field
most connections would leave empty.

**Amend ADR-0141 in place.** Rejected by convention: ADRs here are immutable
and are superseded or refined, never edited. ADR-0141's decision is correct as
far as it goes; this supplies the half it assumed rather than replacing it.
