# ADR-0019: Task retention and board hygiene - hide, never delete

- Status: Accepted
- Date: 2026-07-24
- Deciders: MindLeak maintainers
- Related: [ADR-0004](0004-intent-plane-spec-brain.md) (intent plane is durable,
  not decaying), [ADR-0005](0005-signal-weighted-decay.md) (only memory/knowledge
  decays), [ADR-0009](0009-evidence-backed-conformance.md) (conformance audit),
  [SPEC-INTENT.md](../SPEC-INTENT.md)

## Context

Lodestar tasks are deliberately **durable** — the Intent Plane is the opposite of
MindLeak's decaying memory ([ADR-0004](0004-intent-plane-spec-brain.md)). Only
learned *knowledge* decays ([ADR-0005](0005-signal-weighted-decay.md)) and only a
*lease* expires (reclaimability). Two operational problems followed:

1. **The board grows unbounded.** `done` and `abandoned` outcomes accumulated in
  the same operational view as work that still needed attention.
2. **Non-actionable tasks poison the queue.** Observed 2026-07-23: a `constraint`
   goal (`goal:principled-verified-delivery`) had been decomposed into four
   tasks that merely restate the constraint ("Implement Clean Design Principles",
   "Test New Behavior for Compliance", …). A `constraint`/`invariant` goal is
  checked by **Conformance**, never "done", so those tasks can never accrue
  completion evidence. Mis-filed or obsolete tasks had no honest retirement
  path, while expired claims remained visible as if an agent still owned them.

The tempting fix — give tasks a TTL or decay them like memory — is exactly the
expedient hack [ADR-0004](0004-intent-plane-spec-brain.md) forbids: coordination
history and the conformance audit trail must not silently disappear.

## Decision

**Hide, never delete.** Use the existing terminal outcomes and make operational
versus audit views explicit. Preserve every row and every conformance record. Do
**not** add a task TTL, decay, archive subsystem, or destructive per-task purge.

- **Operational and audit views.** `board(include_terminal=false)` excludes
  `done` and `abandoned`; `board(include_terminal=true)` returns the full durable
  ledger. The VS Code Intent Board uses the operational view and defensively
  filters terminal rows.
- **`abandoned` is deliberate retirement.** `abandon_task` moves open,
  in-review, blocked, or lease-expired claimed work to terminal `abandoned`. The
  task row and every conformance record remain intact.
- **Live ownership is protected.** A live claim cannot be retired; release it
  first. `needs_input` and `paused` tasks retain an owner and must be resolved or
  released first. An expired lease is not live ownership and may be retired.
- **Retirement is available in the Intent Board.** Eligible rows expose a
  confirmed **Retire Task** action whose copy states that history is preserved.
- **Dependencies do not strand.** Retiring a predecessor transactionally opens
  its direct successor because the reason for waiting no longer exists.
- **Governance rule codified.** A `constraint` or `invariant` goal must **not** be
  decomposed into tasks — those are enforced by Conformance, never completed. Only
  `objective` goals decompose into claimable work. `decompose_goal` and manual
  `create_task` should reject (or at minimum refuse to auto-decompose) a
  non-`objective` goal, closing the door that produced the four zombie tasks.
- **First use of the verb.** Retire the four orphaned policy tasks
  (`task:601c38151cb1`, `task:e9c2b3d0f429`, `task:eb1bc5c7f946`,
  `task:f7b6ac02b859`) so they leave the queue while their history remains.

### Rejected alternatives

- **Task TTL / decay** — conflates the durable Intent Plane with decaying memory;
  violates [ADR-0004](0004-intent-plane-spec-brain.md) invariant "the Constitution
  and coordination ledger do not decay". Rejected outright.
- **`purge_task` / hard delete** — destroys coordination history and the
  conformance audit ([ADR-0009](0009-evidence-backed-conformance.md)).
- **Active-only filtering without retirement** — hides terminal history but
  leaves obsolete open/review/blocked and expired-claim work on the live board.
- **`archived_at` plus archive/unarchive tools** — adds a second lifecycle axis
  and ambiguous combinations such as "done and archived" to solve a view problem
  already handled by terminal outcomes and an explicit board filter.
- **Retiring live or parked ownership** — can discard work an agent is actively
  performing or awaiting input on.

## Consequences

- The operational board stays focused as terminal history grows.
- Operators can remove obsolete live work without falsifying its history.
- Crashed agents do not make tasks permanently unretireable after lease expiry.
- Full task history and conformance records remain queryable with
  `include_terminal=true`.
- Storage still grows with durable history. Audit-view pagination can be added
  later without changing lifecycle semantics.
