# ADR-0141: Ackplane reports its own supervisor position, not the client's echo

- Status: Proposed
- Date: 2026-08-29
- Deciders: MindLeak maintainers (proposed in session; awaiting repository-owner
  review per this repo's adoption convention)
- Refines: [ADR-0116](0116-enrolled-supervisors-are-the-distributed-agent-runtime.md)
  decision 7 (on reconnect a supervisor reconciles positions and reports gaps)
- Related: [ADR-0135](0135-a-directive-receipt-survives-a-dropped-connection.md)
  (the durable outbox whose frame sequence this reports),
  [ADR-0083](0083-grpc-is-the-ackplane-node-protocol.md) (the wire this changes),
  [ADR-0084](0084-ackplane-evidence-has-explicit-trust.md) (why an unverifiable
  claim is not evidence)

## Context

`service/handshake.rs` builds `HelloAccepted { accepted_position:
last_accepted_position }`, sourced straight from `hello.last_accepted_position`
— the value the client itself just sent. It is an echo, not an independent
statement.

The supervisor daemon briefly compared `reconcile(positions,
connection.accepted_position())` against that value, which is a number compared
against its own reflection: it can only ever answer `UpToDate`, and its
`IncompleteEvidence` arm is unreachable in production. That call has since been
removed rather than left reading as a working guard, so today nothing claims to
detect the case — which is honest, and also means the case is undetected.

What is lost is real. A supervisor whose durable state was restored from an
older copy, or truncated, cannot be told apart from one that has never run:
both report position zero, and only a peer that remembers can distinguish them.
That is precisely the "frames it published and can no longer describe" case
`reconcile()` was written to refuse to paper over. `reconcile()` itself is
correct, unit-tested, and exercised by slice 4's integration test — it is a
decision function waiting for a truthful input, not the defect.

Two facts constrain the fix.

**The sequence spaces differ.** `accepted_position` on `Hello`/`HelloAccepted`
names the ledger's event stream, the same space as `EventEnvelope`'s
`producer_sequence`. The supervisor outbox (ADR-0135) has its own local frame
sequence, allocated by `SupervisorOutbox::enqueue` and entirely unrelated to
whether the supervisor ever published a ledger event. A supervisor that
receipts directives but publishes no events has a growing outbox sequence and a
ledger position of zero. Reporting one number where the other is expected would
produce a confident, wrong answer.

**The server already has somewhere to put it.** `supervisor_registrations`
(migration `0024_supervisor_session_projection.sql`) is keyed by
`(tenant_id, repository_id, supervisor_id)` and already carries mutable
per-supervisor state in `last_heartbeat_at`, so per-supervisor progress is an
established shape there rather than a new concept.

## Decision

**Ackplane reports its own durable view of a supervisor's outbox frame
position, in a field that names that sequence space, and distinguishes "I have
never seen this supervisor" from "I have seen it at zero".**

1. **A new, distinctly named field — never a reuse of `accepted_position`.**
   The server reports `supervisor_frame_position`: the highest supervisor frame
   sequence it has durably accepted from this supervisor. `accepted_position`
   keeps its existing meaning, the ledger event stream, unchanged. The field
   name states the space it belongs to, because the whole failure this ADR
   closes was a number read as if it meant something else.

2. **It is optional, and absence is not zero.** A server that has never
   accepted a frame from this supervisor omits the field. Zero means "I have
   seen this supervisor and accepted nothing"; omitted means "I have no
   independent statement to make". Collapsing the two would erase exactly the
   distinction — restored-from-an-older-copy versus never-ran — that motivates
   this ADR.

3. **The server persists it per `(tenant_id, repository_id, supervisor_id)`**,
   alongside the registration, advanced only when a supervisor frame is
   durably accepted. It is server-authored state, never written from a value
   the client supplied, or it becomes an echo again by a longer route.

4. **`reconcile()` is unchanged.** Its `IncompleteEvidence` arm becomes
   reachable because its input becomes truthful, not because its logic moves.
   The daemon supplies the server-reported value when present, and when absent
   does not reconcile at all rather than substituting a default.

5. **Degrade to today's behaviour, never to a false verdict.** An older server
   that omits the field, or an older node that ignores it, loses detection —
   which is where this repository already is — and must never produce
   `UpToDate` or `IncompleteEvidence` on the strength of a value nobody
   independently asserted. Silence is reported as silence (ADR-0084: an
   unverifiable claim is not evidence).

## Consequences

- ADR-0116 decision 7's gap reporting becomes real rather than nominal: a
  supervisor whose durable record is behind the server's is told so, and stops,
  instead of resuming and silently skipping frames it can no longer describe.
- The wire gains a field and the server gains a column and a write path; both
  need their own implementation slice, and this ADR deliberately implements
  neither.
- Two positions now travel on the same handshake. That is the cost of not
  conflating them, and the field names are the mitigation.
- A supervisor is now able to detect that its own state was rolled back, which
  is a case no local check can find, because locally the truncated record looks
  entirely self-consistent.

## Rejected alternatives

**Reuse `accepted_position` and report the outbox sequence in it.** Rejected:
it conflates the ledger event stream with the supervisor's local frame
sequence. A supervisor that receipts directives but publishes no ledger events
would compare a growing frame count against a ledger position of zero and
report a gap that does not exist — a confident wrong answer, which is worse
than today's undetected case.

**Derive the position from the ledger instead of storing it.** Rejected: the
outbox frame sequence is not in the ledger. Directive receipts are supervisor
frames, not `EventEnvelope`s, so there is nothing to derive it from.

**Treat a reported zero as "never seen this supervisor".** Rejected: it is
indistinguishable from a genuinely fresh supervisor that has legitimately
accepted nothing, which is the exact discrimination this ADR exists to make.

**Leave it, and have the supervisor trust its own durable state.** Rejected:
that is the status quo, and it fails precisely when the local state is the
thing that is wrong. A restored or truncated record is internally consistent;
only an independent witness can contradict it.
