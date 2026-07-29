# ADR-0064: The log is the ledger

- Status: Accepted
- Date: 2026-07-29
- Deciders: MindLeak maintainers
- Related: [ADR-0048](0048-a-lapsed-lease-holes-the-window-it-does-not-move-it.md) (evidence windows),
  [ADR-0063](0063-a-migration-may-tidy-the-past-never-the-present.md) (live claims are not rewritten),
  [ADR-0054](0054-identity-is-the-session-not-the-process.md) (identity collapse),
  [ADR-0009](0009-evidence-backed-conformance.md) (evidence-bound verdicts),
  [ADR-0020](0020-task-lifecycle-states.md) (task lifecycle states)

## Context

Lodestar's task state is a mutable row. `tasks.status`, `tasks.owner`,
`tasks.lease_expires_at` are overwritten in place on every transition, and the
transition itself is not recorded anywhere. What happened is inferred from what
the row currently says.

The schema shows we have already worked around this three separate times:

- **`claim_lapses` and `unleased_seconds`** are scalar aggregates of events that
  were never written down. Their own comment says so — *"a window survives a
  lapse so earlier work stays provable, but the holes are counted here"*. The
  holes are counted; they are not recorded.
- **`task_claim_transfers`** is an append-only log for exactly one verb, with
  hand-written `from_status`, `from_claim_started_at`, `from_lease_expires_at`
  and `from_parked_at` columns. Those are a before-image, written by hand because
  the row they describe is about to be overwritten.
- **`conformance`** is already append-only: autoincrementing, `checked_at`,
  never updated.

Three improvisations of the same missing primitive.

The cost is not theoretical. Measured on this repository on 2026-07-29, with 220
tasks on the board:

- Answering *"why does the board keep growing?"* required reconstructing inflow
  and outflow from `created_at` / `updated_at` arithmetic, and **the first answer
  was wrong**. Twenty-nine tasks with expired leases read as abandoned
  tombstones; every one of them was held by an agent working at that moment. The
  fact that settled it lived in a different table (`session_context`), because
  the claim history that would have shown continuous activity had been compressed
  into two integers. A sweep on that reading would have stripped 29 tasks from
  four working agents.
- ADR-0048 caps a verdict at `needs_human` whenever `claim_lapses > 0`. But two
  integers cannot distinguish *lapsed while idle* from *lapsed mid-`cargo test`
  with three commits landing inside the hole*. The question ADR-0048 actually
  wants to ask — **did evidence land during an unleased interval?** — is
  unanswerable, because the intervals were never recorded. With the default lease
  at 300 seconds and `cargo test --all` routinely exceeding it, the cap fires on
  healthy work: 21 of 107 completed tasks lapsed at least once and finished
  anyway.
- ADR-0063 documents `tasks.owner` being rewritten out from under a live holder.
  In a mutable row an ownership change is invisible — the field simply differs
  between two reads. The diagnosis cost a session.

Each of these is the same failure: a question about history, asked of a table
that only stores the present.

## Decision

1. **An append-only `task_events` table is the authoritative record of the task
   lifecycle.** One row per transition, typed, carrying the actor, the
   recorded-at timestamp, and the transition's payload. Rows are never updated
   and never deleted. Everything that today overwrites task state instead emits
   an event. The table is named for its scope rather than `events`, because
   decision 6 confines this to the task domain and a bare `events` would promise
   a generality it does not have.

2. **`tasks` becomes a projection of that log — but is never dropped and
   rebuilt.** ADR-0063 is explicit that a live claim is not ours to touch, and
   there are live claims on this board today. A destructive rebuild would
   transfer ownership as a side effect of a migration, which is the exact
   failure ADR-0063 exists to prevent. So: the existing rows stay where they are,
   and the projector writes through them going forward.

3. **Migration imports the present as a genesis event, and says that is what it
   is.** One `task.imported` event per existing task, capturing current state
   verbatim, run exactly once per database by name through `run_once` (ADR-0063
   decision 3). A genesis event declares that it carries **no prior history**. We
   do not synthesise the claims, lapses and transitions that would have produced
   the current row. Inventing plausible history inside an audit ledger is the
   hollow receipt this system exists to refuse.

4. **The projector is deterministic.** It reads no wall clock. Every timestamp is
   a field on the event that recorded it. Rebuilding the projection from the log
   into a scratch table and diffing it against the live table is a test, and that
   test is how decision 2 stays honest.

5. **`claim_lapses`, `unleased_seconds` and `task_claim_transfers` are deleted**
   once the log subsumes them. This ADR is a net removal of state, not an
   addition: three lossy summaries collapse into one record that answers the
   questions they could only approximate.

6. **Scope is the task lifecycle only.** Goals, designs, amendments, policy
   packs, knowledge and controls keep their current write path. They have not
   caused this class of failure, and widening the blast radius to 144 mutating
   statements to fix a problem concentrated in 36 of them would be paying for
   symmetry rather than for a fix.

## Consequences

A lapse becomes an interval with two endpoints rather than a counter, so
"did work land inside the hole?" is a query. ADR-0048's cap can then be
evidence-driven instead of pessimistic, which is a follow-on decision, not this
one.

An ownership change becomes a row with an actor and a timestamp. The ADR-0063
flip would have been visible in one read.

Board health — inflow, outflow, per-agent claim concurrency, time-in-status —
becomes a projection rather than forensics.

Three things this deliberately does **not** do:

- **It does not shrink the board.** The growth measured on 2026-07-29 was 69
  tasks created against 36 closed in one day, driven by four concurrent agents
  fanning observations out into tasks. An event log makes that legible; it does
  not make it smaller. Anyone selling this change as the cure for backlog growth
  is selling the wrong thing.
- **It does not make verdicts recomputable.** Deriving a verdict by replaying
  evidence against any constitution version — and therefore forking the ledger to
  ask what an amendment *would* have decided — is the reason to want this
  architecture, and it is explicitly out of scope here. This ADR builds the
  substrate; that capability is a separate decision on top of it.
- **It does not recover the history we never had.** Every task existing at
  migration time starts from a genesis event and nothing before it. The board's
  past is not reconstructable, and the log will not pretend otherwise.

The cost is real: the task-domain write path (36 mutating statements across
`store/coordination.rs`, `store/lifecycle.rs` and `store/claim_transfer.rs`)
moves behind an emitter, and every task-state test that asserts against a
directly-written row is rewritten to assert against a projected one.

## Alternatives considered

**Keep the rows authoritative and add an audit log alongside (dual-write).**
Cheaper, and it delivers the diagnostic wins immediately. Rejected because it
creates two sources of truth that can disagree, and the disagreement would be
discovered exactly when the ledger is being used to settle a dispute — the worst
possible moment. It is also the shape the prime directive names: a transitional
bridge that quietly preserves the legacy pattern and never gets removed.

**Better counters.** Record lapse intervals as a JSON column on `tasks` instead
of two integers. Rejected: it is the same lossiness with more syntax, and it is
the fourth improvisation of the primitive the first three already needed.

**Adopt an existing event-sourced agent runtime (e.g. ActiveGraph).** Its
inversion — append-only log as truth, graph as projection — is the argument this
ADR is built on, and its fork-and-diff capability is more valuable to a
governance plane than to an agent runtime. Rejected as an adoption because it is
a different layer: it executes the agent's behaviours, in Python, in its own
process. Lodestar governs agents it does not run, cannot instrument, and did not
write. The idea transfers; the runtime cannot.
