# ADR-0019: Task retention and board hygiene (archive, never decay)

- Status: Proposed
- Date: 2026-07-23
- Deciders: MindLeak maintainers
- Related: [ADR-0004](0004-intent-plane-spec-brain.md) (intent plane is durable,
  not decaying), [ADR-0005](0005-signal-weighted-decay.md) (only memory/knowledge
  decays), [ADR-0009](0009-evidence-backed-conformance.md) (conformance audit),
  [SPEC-INTENT.md](../SPEC-INTENT.md)

## Context

Lodestar tasks are deliberately **durable** — the Intent Plane is the opposite of
MindLeak's decaying memory ([ADR-0004](0004-intent-plane-spec-brain.md)). Only
learned *knowledge* decays ([ADR-0005](0005-signal-weighted-decay.md)) and only a
*lease* expires (reclaimability). There is no task TTL, prune, archive, or
per-task delete; the only removal is a full `reset_database`. Two problems follow:

1. **The board grows unbounded.** `done` / `abandoned` / `in_review` tasks
   accumulate forever, and `board()` returns *every* task ordered by
   `created_at`, so the live coordination view clutters over time.
2. **Non-actionable tasks poison the queue.** Observed 2026-07-23: a `constraint`
   goal (`goal:principled-verified-delivery`) had been decomposed into four
   tasks that merely restate the constraint ("Implement Clean Design Principles",
   "Test New Behavior for Compliance", …). A `constraint`/`invariant` goal is
   checked by **Conformance**, never "done", so those tasks can never accrue
   completion evidence — yet `next_task` (oldest-first) surfaces one at the **top**
   of the queue, handing an agent a zombie it can never legitimately complete.
   There is currently no verb to retire them short of wiping the database.

The tempting fix — give tasks a TTL or decay them like memory — is exactly the
expedient hack [ADR-0004](0004-intent-plane-spec-brain.md) forbids: coordination
history and the conformance audit trail must not silently disappear.

## Decision

**Hide, never delete.** Add a terminal **`archived`** disposition, reached by an
explicit **`archive_task`** verb, and make the default views exclude it. Preserve
every row and every conformance record. Do **not** add a task TTL, decay, or a
destructive per-task purge.

- **`archived` marker.** A nullable `archived_at` timestamp on the task row (the
  live `status` enum is unchanged; `archived` is an orthogonal disposition so a
  `done` task stays auditable as `done` *and* archived). Archiving is reversible
  by clearing `archived_at` (an `unarchive`, symmetric with `reopen_task`), so it
  is a view decision, not data loss.
- **`archive_task(task_id)` verb.** Owner-safe: refuses a task with a **live
  claim** (release or let the lease expire first — same guard as `block_task`),
  so archiving never yanks work out from under an active agent. Allowed from any
  non-claimed state — terminal (`done`/`abandoned`) for hygiene, or `open`/
  `blocked` to retire a mis-filed task. It never deletes the row and never touches
  the append-only conformance audit.
- **Default views exclude archived.** `board()` and `next_task` return only
  non-archived tasks by default; `board(include_archived=true)` (and/or a
  `status` filter) exposes the full durable history for audit. This is the board
  *hygiene* fix, orthogonal to durability.
- **No purge, no TTL, no decay.** History stays complete and regenerable-free.
  Archiving reclaims the *view*, not the *storage*; the durable/consistent
  invariant of the Intent Plane is untouched.
- **Governance rule codified.** A `constraint` or `invariant` goal must **not** be
  decomposed into tasks — those are enforced by Conformance, never completed. Only
  `objective` goals decompose into claimable work. `decompose_goal` and manual
  `create_task` should reject (or at minimum refuse to auto-decompose) a
  non-`objective` goal, closing the door that produced the four zombie tasks.
- **First use of the verb.** Archive the four orphaned policy tasks
  (`task:601c38151cb1`, `task:e9c2b3d0f429`, `task:eb1bc5c7f946`,
  `task:f7b6ac02b859`) so they leave the queue while their history remains.

### Rejected alternatives

- **Task TTL / decay** — conflates the durable Intent Plane with decaying memory;
  violates [ADR-0004](0004-intent-plane-spec-brain.md) invariant "the Constitution
  and coordination ledger do not decay". Rejected outright.
- **`purge_task` / hard delete** — destroys coordination history and the
  conformance audit ([ADR-0009](0009-evidence-backed-conformance.md)). Archiving
  hides without losing anything; a destructive delete is never the default.
- **Board pagination / active-only filter alone (no state)** — trims the *tail*
  but cannot distinguish "a recent `done` task" from "a zombie that should never
  have existed", so the four policy tasks would still surface. A first-class
  `archived` marker is what lets an operator *retire* a specific task.
- **Leaving `abandoned` as the retirement path** — `abandoned` implies work was
  started and dropped; it carries the wrong meaning for a mis-filed task and there
  is no verb to reach it anyway. `archived` is disposition, not outcome.

## Consequences

- `next_task` and `board()` stop surfacing zombies and stale terminal tasks; an
  agent asking "what next?" gets only live, actionable work.
- The full task history and every conformance record remain queryable for audit
  via `include_archived` — durability intact.
- New surface to implement and test in `lodestar-core` (store: `archived_at`
  column + `archive_task`/`unarchive`, claim-guard, board/next_task filters) and
  `lodestar-mcp` (tool defs + branches), plus README tool-table rows,
  SPEC-INTENT §4 update, and a CHANGELOG entry. New behaviour gets tests: archive
  hides from `board`/`next_task` but preserves the row and audit; archive refuses
  a live-claimed task; `include_archived` still returns it; `reset_database`
  still clears everything; decomposing a non-`objective` goal is refused.
- Implementation touches `store.rs`, which is under an active concurrent split
  (`task:5154b48b45b8`); the code change must serialize **after** that refactor
  lands to avoid clobbering it. This ADR is the design-first deliverable; the
  implementation is its follow-on.
