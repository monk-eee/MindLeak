# ADR-0020: Task lifecycle state machine — `needs_input` and `paused`

- Status: Proposed
- Date: 2026-07-23
- Deciders: MindLeak maintainers
- Related: [ADR-0004](0004-intent-plane-spec-brain.md) (intent plane is durable,
  coordination does not decay), [ADR-0015](0015-advisory-symbol-leases.md)
  (progressive `blocked_by` handoffs), [ADR-0019](0019-task-retention-and-board-hygiene.md)
  (archive disposition), [SPEC-INTENT.md](../SPEC-INTENT.md) §4

## Context

The Executive task ledger has one live status enum:
`open · claimed · in_review · done · blocked · abandoned`, plus the orthogonal
`archived` disposition from [ADR-0019](0019-task-retention-and-board-hygiene.md).
Two coordination needs are unmet, and each was being filed as a plain feature
task that would have grown the state machine *inside* implementation code:

1. **Agent raises a question / blocker for a human.** A working agent hits an
   ambiguity it cannot resolve and needs a durable, pull-based answer — it must
   park the task in a way a human can see and respond to, without the agent
   abandoning ownership or holding a lease it cannot service.
2. **Owner parks work deliberately.** An owner wants to suspend a claimed task and
   resume it later, rather than release it back to the pool (losing the claim) or
   sit on a stale lease.
3. **A dependency-blocked chain strands when its predecessor never completes.**
   `block_task` clears the lease, so a `blocked_by`-gated successor has no lease to
   expire; it opens *only* when its predecessor completes to `done` (aligned), and
   `reopen_task` refuses to touch a dependency-gated task. If the predecessor is
   `abandoned` — or sits indefinitely in `in_review`/`needs_human` — the successor
   is stranded permanently, with no timeout and no escape.

The first two add a **new variant to the Task status enum** — a change to the
coordination state machine documented in [SPEC-INTENT.md](../SPEC-INTENT.md) §4.
That is a design decision that is hard to reverse once agents depend on it, so it
belongs in an ADR, not in feature execution. This ADR defines the whole extended
machine once so the two implementations (needs-input Q&A, pause/resume) build
against an agreed model instead of each inventing transitions, and closes the
blocked-chain stranding gap (3) with the same anti-stranding principle.

## Decision

Add two live states, **`needs_input`** and **`paused`**, both reachable *only*
from `claimed` by the current owner. Both **clear the live lease but preserve
ownership** (the `owner` and the `claim_started_at` evidence window survive); they
are deliberate parking, not release and not abandonment.

### The state machine

```
                 ┌───────── release ─────────┐
                 ▼                            │
  (predecessor done) ──► open ──claim──► claimed ──complete[aligned]──► done*
                 ▲          ▲   ▲   │  │   │  │
     reopen_task │  answer  │   │   │  │   │  └─complete[drift|needs_human]─► in_review
                 │          │   │   │  │   │                                     │
             in_review ◄────┼───┼───┘  │   └── block_task ──► blocked ──(pred done / reopen)─► open
                 ▲          │   │       │                        │
                 │      needs_input     └── pause ──► paused ──resume──► claimed
                 │      (owner asks)                    │
                 └──────────┘                     (owner resumes, fresh lease)

  * done and abandoned are terminal. archived (ADR-0019) is an orthogonal
    disposition reachable from any non-claimed state; it hides, never deletes.
```

### Transitions and guards

| From | To | Verb | Guard / effect |
|---|---|---|---|
| `claimed` | `needs_input` | `ask_question` | owner-only; records a durable question; **clears live lease, keeps owner + `claim_started_at`** |
| `needs_input` | `claimed` | human `answer` | records the durable answer; returns to the **same owner** with a **fresh lease** |
| `claimed` | `paused` | `pause` | owner-only; **clears live lease, keeps owner + `claim_started_at`** |
| `paused` | `claimed` | `resume` | **same owner**; fresh lease |

Unchanged transitions (`claim`, `release`, `complete`, `block_task`,
`reopen_task`, `archive`) keep their existing semantics. `needs_input` and
`paused` are **non-terminal** and **not** on the `blocked_by` handoff path: a task
must be `claimed` to enter them, and a `blocked_by`-gated task cannot be claimed,
so a handoff dependency can never be bypassed by parking. `reopen_task` continues
to act only on `in_review` / manually-`blocked` tasks; it does **not** touch
`paused` / `needs_input`, which resume through their own verbs.

### Ownership, leases, and anti-stranding

`needs_input` and `paused` differ from the three existing "not actively running"
shapes and must not be conflated with any of them:

- **vs `release`** — release clears `owner`; these keep it. The work is still
  *assigned*, just not *running*.
- **vs lease expiry** — an expired lease makes a `claimed` task reclaimable by
  anyone; a parked task is **owned**, so it is not silently reclaimed mid-park.
- **Anti-stranding (the real risk).** Because ownership is retained with no live
  lease, an owner that pauses/asks and never returns could strand the task
  forever. Resolve this explicitly: a parked task records `parked_at` and becomes
  **reclaimable after a bounded parking grace** (a distinct, longer horizon than
  the active lease, configurable), after which it returns to `open` for the pool.
  This preserves "does not abandon ownership" for the normal case while
  guaranteeing no task is permanently deadlocked by a vanished owner.

### Dependency-blocked chains: the same stranding risk

The parking grace above rescues *owned* parked tasks; the existing `blocked_by`
handoff has the mirror problem with **no** valve. Verified in
`crates/lodestar-core/src/store/coordination.rs`: `block_task` clears the lease (a
blocked task has none to expire), a dependency-gated successor opens *only* when
its predecessor completes to `done` (aligned), and `reopen_task` **refuses** a
task whose `blocked_by` is set ("it opens when that predecessor completes"). So an
`abandoned` predecessor — or one parked forever in `in_review`/`needs_human` —
strands its whole successor chain with no recovery.

Decision: **predecessor abandonment cascades.** When a predecessor reaches a
terminal non-`done` state (`abandoned`, or `archived` per
[ADR-0019](0019-task-retention-and-board-hygiene.md)), its blocked successor
transactionally returns to `open` — the reason it waited no longer exists — with
the `task_handoffs` lineage retained for audit. The gate is lifted, never silently
deadlocked. A predecessor merely *stuck* in `in_review`/`needs_human` is not
terminal, so the successor stays correctly gated; the escape there is to resolve
the predecessor (accept/complete it, or abandon it — which then cascades). This
repo has a live example: this ADR's own task sits in `in_review` (needs-human),
holding its successors `blocked` until it is completed aligned — correct gating
that must never become a permanent deadlock.

### Durability

Both states, the `needs_input` question, and the human answer are **durable,
persistent, and non-decaying** — coordination state per
[ADR-0004](0004-intent-plane-spec-brain.md), never memory. The Q&A is an
append-only thread (never deleted), consistent with the archive-not-delete stance
of [ADR-0019](0019-task-retention-and-board-hygiene.md).

### Rejected alternatives

- **Reuse `blocked` for `needs_input`.** `blocked` means "a predecessor task must
  complete first" and clears the claim; a question-for-a-human keeps the owner and
  is answered by a person, not a task completion. Overloading `blocked` would
  conflate task-dependency with human-input and corrupt the handoff invariant.
- **Release instead of pause.** Release drops the claim, so resuming means racing
  every other agent to re-claim and losing the evidence window. Parking must
  retain the owner.
- **Leave a parked task non-reclaimable forever.** Simpler, but a vanished owner
  deadlocks the task with no recovery verb (release is owner-guarded). The bounded
  parking grace is the right-shaped fix.
- **A live network callback for the human answer.** Agents are stateless between
  turns and the plane is stdio-only ([ADR-0004](0004-intent-plane-spec-brain.md));
  the answer must be durable and pulled, not pushed.
- **Give a `blocked_by` task a lease/TTL so it auto-expires to `open`.** A blocked
  task is waiting on a *dependency*, not running work; a timer would open it while
  the predecessor is still legitimately in flight, bypassing the handoff invariant
  ([ADR-0015](0015-advisory-symbol-leases.md)). The gate must lift on the
  predecessor's terminal state, not on a clock.

## Consequences

- Two downstream implementations build against this agreed machine:
  `task:4e85e6c993b2` (`needs_input` + durable Q&A thread + `ask_question` /
  `answer`) and `task:52536318bcd7` (`paused` + `pause` / `resume`), serialized as
  a `blocked_by` chain behind this ADR because both edit the same lifecycle enum
  in `crates/lodestar-core/src/store.rs`.
- New surface to implement and test: owner-guarded transitions, lease-clear-but-
  keep-owner, same-owner resume, the bounded parking-grace reclaim, and exhaustive
  `match` on the extended enum (no catch-all that could swallow a future variant).
- `board` / `next_task` must treat `needs_input` and `paused` as **not
  claimable** (owned, parked) while surfacing them distinctly so a human sees a
  question awaiting an answer.
- Abandoning (or archiving) a task with a blocked successor must transactionally
  return that successor to `open`, mirroring the aligned-completion unblock in
  `store/coordination.rs`, so no chain is stranded by a dead predecessor. Tests:
  abandon a predecessor and assert its blocked successor opens; a predecessor
  still `in_review` keeps the successor blocked.
- SPEC-INTENT §4 is updated to list the two states and their transitions; this ADR
  is the design-first predecessor and carries no behavioural code itself.
