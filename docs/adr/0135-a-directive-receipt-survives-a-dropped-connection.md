# ADR-0135: A directive receipt survives a dropped connection

- Status: Accepted
- Date: 2026-08-29
- Deciders: MindLeak maintainers
- Accepted: 2026-08-29 by the repository owner, authorized directly in session
  — attributed human adoption after review.
- Depends on: [ADR-0107](0107-registered-agents-accept-authenticated-control-directives.md)
  (registered agents accept authenticated control directives),
  [ADR-0116](0116-enrolled-supervisors-are-the-distributed-agent-runtime.md)
  (enrolled supervisors are the distributed agent runtime, decision 6: "every
  lifecycle transition has a durable receipt", decision 7: reconnect
  reconciliation)
- Related: [ADR-0125](0125-bridge-work-commands-are-principal-scoped-and-receipted.md)
  (Assign/Steer/Pause/Resume/Drain issue the directives this receipt answers)

## Context

ADR-0116 slice 5 shipped a runnable supervisor daemon that opens
`SupervisorOutbox` on every `serve_once` call and reads its `positions()` at
startup, but nothing in the daemon called `enqueue()` or `pending()` outside
its own tests. Every outbound frame — registration, session, heartbeat, and a
directive receipt computed by `SupervisorInbox::receive` — went straight over
the live connection. A receipt lost to a connection drop between computing it
and the server acknowledging it therefore depended entirely on Ackplane
redelivering its directive: a guarantee held by the other side of the exact
connection that had just failed, not by anything this supervisor could rely
on independently.

A second, unrecorded defect made this worse. `serve_once` passed the outbox's
own `positions.acknowledged` into `NodeSyncConnection::open` as
`last_accepted_position`, and `service/handshake.rs` builds `HelloAccepted`
with `accepted_position: last_accepted_position` — sourced straight from the
value the client itself just sent. `reconcile(positions,
connection.accepted_position())` was therefore comparing a number against its
own reflection and could only ever answer `UpToDate`; `IncompleteEvidence` was
unreachable, so the honest reconnect-gap reporting slice 4 built and slice 5
claimed to run was not operating at all. `reconcile()` itself is correct,
unit-tested, and exercised by slice 4's own integration test — it is the
decision function waiting for a truthful input, not the defect.

Detecting a server genuinely ahead of this supervisor's own durable state
needs Ackplane to report its own independent view of that supervisor's
position. The wire protocol carries no such field today, and adding one is a
protocol change with its own review — not something to retrofit onto the
daemon incidentally while fixing the outbox. `accepted_position` in
`HelloAccepted` is about the ledger's event stream; the supervisor outbox has
its own local frame sequence, so a future fix has to say which one it reports
rather than reusing a field that already means something else. This is
recorded as an open gap
(`gaps.d/ackplane-never-reports-its-own-supervisor-position.md`), not solved
here.

## Decision

**A directive receipt is durable before it is transmitted. Registration,
session, and heartbeat frames are not, because nothing depends on their
retransmission surviving a restart. Reconnect reconciliation is not claimed
to detect a case it cannot detect.**

1. **A directive receipt is enqueued into the durable outbox before
   submission, and acknowledged only once Ackplane's own frame receipt
   confirms it.** `enqueue_receipt` assigns the next local sequence and
   writes the encoded `NodeFrame` before `submit_directive_receipt` is ever
   called; `outbox.acknowledge_through` runs only after that call succeeds.
   A receipt lost between being written and being confirmed survives in the
   outbox across a dropped connection or a process restart.

2. **Registration, session, and heartbeat frames deliberately do not route
   through the outbox.** Losing one of these to a dropped connection loses
   nothing durable: the daemon reconnects and sends a fresh registration,
   session announcement, or heartbeat with current information, which is
   exactly as correct as the one that was lost. Queuing them would gain
   nothing and would grow the outbox with frames whose content is already
   stale by the time a resend could matter.

3. **On connect, every frame still owed from a prior connection is resent
   before any new directive is served.** `resend_pending` walks the outbox's
   pending frames in sequence order and resends each one. A frame the server
   accepts is acknowledged and removed from the pending set. A frame the
   server refuses with a non-retryable refusal is dropped from the queue
   rather than resent on every future reconnect — retrying it forever would
   wedge the daemon behind a frame Ackplane will never accept, and whether a
   refusal is retryable is the server's own judgement to make, not this
   code's guess. This resend is entirely self-contained: it needs no
   server-reported position, because the outbox's own enqueued-versus-
   acknowledged pair is a truthful, local comparison regardless of what the
   server reports.

4. **The reconnect call that compared a position to its own echo is removed,
   not left reading as a working guard.** `serve_once` no longer calls
   `reconcile` against `connection.accepted_position()`. A future
   implementation that gives Ackplane its own reported supervisor position
   may reintroduce that call meaningfully; until then, nothing in the daemon
   claims to detect a server holding evidence this supervisor cannot
   account for. `IncompleteEvidence` remains a defined `DaemonExit` variant
   that `run` still handles correctly, and `reconcile` remains correct and
   tested for the day a truthful input exists — neither is deleted, because
   both are right; only the false claim that they were already wired
   together is.

5. **A connection dropped while registering, announcing a session, or
   submitting a receipt is an ordinary reconnect, not a fatal error.**
   `disconnected_on_error` treats any transport failure from those three
   calls the same way the heartbeat already treated one: as `Disconnected`,
   which `run` retries after its configured delay. An earlier revision
   propagated exactly this failure with `.map_err(Box::new)?` instead, which
   ended the daemon permanently for a condition with nothing distinguishing
   it from a heartbeat drop.

## Consequences

A directive receipt now has the same durability guarantee ADR-0116 decision 6
already promised for every other lifecycle transition: computed once,
recorded once, and delivered exactly once even across a connection that drops
mid-send. This records the decision behind the fix that already closed
`gaps.d/supervisor-outbox-is-never-wired-into-the-daemon.md`.

The reconnect-gap detection ADR-0116 decision 7 and slice 4 built remains
real, tested, and currently unreachable in the running daemon. That is
recorded honestly as an open gap rather than papered over with a call that
looked like it worked. A supervisor whose durable state is restored from an
older backup or truncated cannot currently be told apart from one that has
never run, because both report position zero and nothing today gives the
daemon an independent server-reported position to compare against.

A non-retryable server refusal of a queued frame is now dropped rather than
retried forever, which is strictly safer than the wedge it replaces, but it
means a permanently refused receipt is logged and discarded rather than
retried — an operator who needs to know a specific receipt was dropped must
read the daemon's logs; there is no separate durable record of a dropped
frame beyond that log line today.

## Alternatives considered

**Leave redelivery to the server.** Ackplane may well redeliver a directive
whose receipt was lost, and the inbox already replays an identical receipt
for a repeated directive idempotently either way. But that guarantee belongs
to the side of the connection that just failed, not to this supervisor's own
durable state, and ADR-0116 decision 6 asks for a receipt that is durable
independently of what the peer does next.

**Retry a non-retryable refusal anyway, on the theory that conditions might
change.** Rejected because a refusal the server has already judged permanent
will not become acceptable on the next attempt, and retrying it forever wedges
every later frame behind one that can never clear.

**Detect `IncompleteEvidence` today by inferring a position from indirect
signals (batch acknowledgements, directive counts).** Rejected for the same
reason ADR-0132 rejected inferring confinement instead of declaring it: a
wrong answer here fails in the dangerous direction — silently proceeding on
durable state that is actually behind reality — and an inferred signal that
is confidently wrong is worse than an open, recorded gap.
