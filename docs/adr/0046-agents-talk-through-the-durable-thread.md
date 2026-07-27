# ADR-0046: Agents talk through the durable thread, never to each other

- Status: Accepted
- Date: 2026-07-27
- Deciders: MindLeak maintainers
- Refines: [ADR-0020](0020-task-lifecycle-states.md) (task lifecycle states —
  `needs_input` and `paused`)
- Related: [ADR-0015](0015-advisory-symbol-leases.md) (no symbol-lease
  primitive), [ADR-0009](0009-evidence-backed-conformance.md) (evidence-backed
  conformance), [ADR-0045](0045-a-fleet-is-a-distributed-system.md) (a fleet is a
  distributed system)

## Context

Concurrent agents could not exchange anything directed at one another. They
shared a blackboard — `record_knowledge`, `record_decision`, `working_set`,
`check_overlap` — and `check_overlap` even named the colliding agent and its
lease expiry. But nothing could be *addressed*, and two specific things were
missing rather than merely absent.

**A block carried no reason.** `block_task(id, blocked_by, now)` took a
predecessor task id and nothing else. Blocking clears a live claim, so an agent
could have work taken off it with no way to discover why — and `pause_task` was
the same. That is exactly the failure ADR-0045 names: a state change whose
message points nowhere near its cause. The system that exists to make verdicts
explicable was itself producing an unexplained one.

**`ask_question` could only reach a human.** It parks the task in `needs_input`
and waits for a person. When one agent needed something only a peer knew — did
you already rename this symbol, is your migration landing before mine — the
only route was to park for a human who would have to go and ask that peer.

The obvious fix is a message channel: a mailbox, a queue, an inbox per agent.
It is also the wrong one. Messages are ephemeral, unattributed, and invisible to
the evidence bundle, so a decision reached in one would never appear in the
conformance record — a second source of truth beside the ledger, which is the
thing this project exists to eliminate. It is also a new shared mutable resource
needing its own arbiter (ADR-0045 clause 2), and a queue's read-and-consume
semantics introduce a way to *lose* a question that the current design does not
have.

## Decision

**Agent-to-agent communication is addressed rows on a task's durable thread,
discovered by reading. There is no channel, and nothing is ever delivered.**

1. **`task_qa` is the task's dialogue, not just its Q&A.** It gains a third
   kind, `note`, and `block_task` and `pause_task` take an optional `reason`
   recorded there. A blank reason writes nothing: an empty note is worse than
   none, because a reader sees an entry and believes an explanation was given.
   A losing (non-owner, no-op) call writes nothing either — nothing happened, so
   nothing is explained.
2. **A question may be addressed at a peer.** `ask_question` takes an optional
   `audience` (an agent id); `NULL` keeps today's meaning, a human. This is the
   only addressing in the system and it routes nothing.
3. **The transition is identical whether the addressee is a human or an agent.**
   The owner cannot proceed until answered either way, so the task parks and the
   ADR-0020 parking grace still protects it from an addressee that never
   replies. Addressing changes who is expected to answer, never what the task
   does while it waits.
4. **Discovery is a query, never a delivery.** `pending_questions(agent)`
   returns unanswered questions addressed to that agent. Nothing is reserved or
   consumed, so two readers see the same rows and reading can never lose a
   question. It needs no arbiter, because it mutates nothing.
5. **Anyone may answer.** An addressed question is cleared by the next answer on
   that task whoever wrote it. A human must always be able to unstick a pair of
   agents waiting on each other; restricting the answer to the addressee would
   deadlock them until the parking grace elapsed a week later.
6. **An agent may not address a question to itself.** Refused, because it parks
   the task waiting on the only agent that cannot act while it is parked — a
   self-deadlock that reads as a legitimate wait.

## Consequences

- Every exchange is durable, attributed, timestamped, and already inside the
  evidence bundle. A human reviewing a conformance record can read why one agent
  waited on another, which a chat channel would have put somewhere else.
- No new shared mutable resource, and therefore no new arbiter. The only write
  is an append to a table that is already append-only.
- Latency is a poll. An agent learns it was asked something when it next calls
  `pending_questions`, so this suits questions that block a task, not chatter.
  That is the intended shape: if an exchange did not park work, it did not need
  to be addressed and belongs on the blackboard.
- `block_task` and `pause_task` gain parameters, and `TaskQa` gains an
  `audience` field. Every existing call site is migrated in this commit; the
  column is added by transactional migration and is `NULL` for existing rows,
  which reads correctly as "addressed to a human".
- Two agents can now wait on each other. Clause 5 keeps that recoverable by a
  human and clause 3 keeps the parking grace as the backstop, but the deadlock
  is reachable and `fleet_view` does not yet surface it.

## Rejected alternatives

- **A mailbox, queue, or pub/sub channel between agents.** The expedient answer.
  Rejected on three counts: it creates a second source of truth beside the
  evidence ledger; it is a new shared mutable resource requiring an arbiter
  (ADR-0045 clause 2); and read-and-consume semantics introduce a way to lose a
  question that reading a table does not have.
- **Push notification to the addressed agent.** Requires the server to reach an
  agent it does not own, holds no useful meaning for a stdio process that may
  not be running, and turns a stateless read into delivery state that must then
  be made reliable. Polling is honest about what it can promise.
- **Restrict answering to the addressee.** Symmetrical and tidy, and it
  deadlocks two agents that address each other with no human recovery short of
  the seven-day grace.
- **A `reason` column on `tasks` instead of a thread note.** Only holds the most
  recent reason, so a task blocked twice loses the first. The thread is already
  append-only and already read by `task_qa`; a second, lossy store beside it
  would be a fork of the same idea.
- **Let agents write to each other's tasks directly.** Breaks the owner guard
  that makes claims meaningful. A question is a request to its owner, not an
  edit of their state.
