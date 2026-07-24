# ADR-0023: Design items, the Design Board, and reviewed materialization

- Status: Accepted
- Date: 2026-07-23
- Deciders: MindLeak maintainers
- Related: [ADR-0004](0004-intent-plane-spec-brain.md) (intent plane),
  [ADR-0009](0009-evidence-backed-conformance.md) (code-evidence conformance),
  [ADR-0019](0019-task-retention-and-board-hygiene.md) (archive, never delete),
  [ADR-0020](0020-task-lifecycle-states.md) (task lifecycle), [SPEC-INTENT.md](../SPEC-INTENT.md)

## Context

An accepted ADR is currently **inert** to Lodestar: nothing turns a landed design
decision into scheduled executive work. The design→implementation handoff is
entirely hand-authored (manual `blocked_by` chains), and this session proved that
pattern is not just tedious but **broken**:

- **Design tasks cannot complete.** A design/ADR task produces a docs commit.
  `complete_task` runs [ADR-0009](0009-evidence-backed-conformance.md) conformance,
  which scores evidence against the goal's **code** bindings; a docs commit
  "does not touch code bound to the task goal", so the verdict is `needs_human`
  and the task lands permanently in `in_review`. There is no code for a design
  decision to conform to.
- **Their successors strand.** An implementation task chained `blocked_by` a
  design task only opens when that predecessor reaches `done` — which a design
  task can never do. So `task:4e85e6`, `task:52536318`, `task:69100f` were left
  stranded behind `in_review` design tasks, and (with `reopen_task`/`archive` not
  exposed in the running server) could not be recovered at all.

The missing concept is a first-class **design item** with a **human acceptance**
step that completes design work the way code-evidence completes implementation
work — and, after acceptance, maps the design to reviewed executive work. The
building block already exists: `decompose_goal` breaks a goal into tasks (local
model with a deterministic single-task fallback); it must be generalized to an
accepted design item.

The first implementation proved that decomposition alone is unsafe. ADR-0028
already had an authoritative pilot task (`task:7f5ae1198134`) blocked on the
v0.1.1 release. Blind deterministic fallback nevertheless created an unblocked
duplicate (`task:735e36892ffa`) under the wrong objective and marked the design
fully materialized. Promotion must therefore review whether to create, link, or
create no work; task creation is not the default meaning of acceptance.

## Decision

Introduce **design items** as first-class Intent-Plane objects, a **Design Board**
distinct from the executive board, and an **accept→review→materialize bridge**.

### Design item = an ADR under review

A design item references its ADR file (`docs/adr/NNNN.md`) by opaque path/id and
carries the ADR's own lifecycle: the **taint is the ADR `Proposed` status**;
acceptance is `Accepted`. While tainted it is **not claimable** and **must not**
appear in `next_task` or the executive board — it lives on the Design Board.

### The human acceptance gate

A tainted design item stays tainted until an explicit, attributed **human**
`accept_design` or `reject_design`. **No agent may accept its own design**
(human-in-the-loop; reuse the answer surface of the `needs_input` channel,
[ADR-0020](0020-task-lifecycle-states.md)). Acceptance is the completion path for
design work — it does **not** run [ADR-0009](0009-evidence-backed-conformance.md)
code conformance (there is no code to conform to), resolving the `in_review`
dead-end above. Rejection is durable and auditable, never a silent delete
(archive-not-delete, [ADR-0019](0019-task-retention-and-board-hygiene.md)).

### The accept→review→materialize bridge

Acceptance and task materialisation are two durable phases. An optional model
call cannot be part of the SQLite transaction that records human acceptance:
holding that transaction across network I/O would serialize unrelated writers,
and a timeout after acceptance would leave it unclear whether retrying should
create duplicate tasks.

1. `accept_design(id, human)` performs only the attributed, guarded human
  decision. The design becomes `accepted` with promotion state `pending`. It
  does not invoke a model or create tasks.
2. `plan_design_promotion(id, objective_goal_id)` is read-only. It may use the
  same model-assisted planner as `decompose_goal`, with the deterministic
  single-task fallback, but its output is only a suggested `create` plan.
3. A human reviews one explicit mode: `create` reviewed drafts under one or more
  objectives, `link` existing task ids, or `no_work` with a rationale.
4. `promote_design(id, plan)` atomically writes the reviewed plan, current
  design→task/design→goal projection, any newly created tasks/constraints, and
  promotion state. An identical retry returns the same revision.
5. `revise_design_promotion(id, human, plan)` requires a rationale and appends a
  new immutable revision before replacing the current projection. Earlier plans
  and tasks remain durable; registered constitutional clauses are not silently
  removed. `design_materialization_history(id)` exposes the full audit chain.

The reviewed plan may contain:

- one or more new task drafts, each naming its objective;
- one or more existing task ids, including blocked/done work already in the
  authoritative delivery chain;
- no tasks when the decision is already implemented or intentionally requires
  no new work, with a required rationale;
- durable constraints/invariants to register through the existing Constitution
  path; and
- explicit links from every current task/objective and every durable normative
  clause back to the design item.

Planning remains model-assisted with a **deterministic single-task fallback** when
no local model is reachable — never a hot-path or hard LLM dependency. It is
read-only, so a poor fallback cannot manufacture work before review. A failed
plan/materialization leaves human acceptance intact and safely retryable.

`decompose_goal` and `plan_design_promotion` reuse one internal planner. The
write API accepts only the explicit plan, not an objective plus hidden planner
side effect.

### The Design Board

A portal view (editors/vscode) distinct from the executive Intent Board: lists
tainted/proposed design items with the ADR text/link and **accept/reject**
actions and writes the matching `Accepted`/`Rejected` status into the ADR file.
Pending promotion offers **Create new tasks**, **Link existing tasks**, and **No
new work**. Create accepts multiple objectives and previews every draft; all
modes show a modal concrete plan before writing. Materialized rows expose
provenance/history and an attributed **Repair** action. The Intent Board is
improved to (a) exclude tainted
design items, and (b) show which executive tasks descend from which accepted ADR
(provenance rollup). Keep vscode-coupled code thin; pure board/threading logic in
`editors/vscode/src/util.ts` with vitest.

### Natural ADR reconciliation

ADR discovery should be routine, but **not every ADR should create new work**.
Eighteen accepted ADRs predate the Design Board and mostly describe behaviour
already implemented; blindly decomposing them would manufacture duplicate work.

An idempotent `reconcile_designs` path accepts structured ADR metadata from the
workspace sensor and applies these rules:

| Repository ADR | Design state | Scheduling behaviour |
|---|---|---|
| New `Proposed` ADR | `proposed` | Appears on the Design Board; no task exists. |
| Accepted through the Design Board | `accepted` / `pending` | A reviewed create/link/no-work plan materializes exactly once. |
| Accepted before Design Board adoption | `accepted` / `not_required` | Imported for history; creates no tasks unless a human explicitly reopens promotion. |
| Rejected ADR | `rejected` | Retained for audit; never creates tasks. |

The extension runs reconciliation on activation and when an ADR file changes,
and exposes a manual **Sync ADRs** command. Discovery may parse the ADR identity,
title, and declared status; it must not infer implementation tasks from arbitrary
Markdown. Task derivation is read-only until a human confirms an explicit
materialization plan.

### Rejected alternatives

- **Auto-accept ADRs.** Removes the human design gate; a design decision must be
  consciously reviewed before it schedules work.
- **Agents self-accepting their own design.** Defeats human-in-the-loop review.
- **A directly-claimable design item.** Conflates design review with
  implementation; the taint exists precisely to keep them separate.
- **Parsing arbitrary markdown to infer tasks.** Brittle; decomposition takes the
  human-accepted ADR's summary/acceptance text, model-assisted with a
  deterministic fallback.
- **Decompose every accepted ADR during first sync.** Historical acceptance does
  not prove implementation is outstanding; this would duplicate completed work.
- **Run decomposition inside `accept_design`.** Optional model I/O cannot be made
  atomic with SQLite, and an ambiguous retry could create duplicate tasks.
- **Always create the deterministic fallback task.** ADR-0028 proved that a
  syntactically valid fallback can duplicate authoritative blocked work and put
  it under the wrong objective.
- **Repair provenance by rewriting SQLite links.** Hides who changed the mapping
  and destroys the prior reviewed state; repairs are append-only revisions.
- **Completing design tasks via a fake code-evidence bundle.** Would launder a
  docs commit through code conformance; acceptance is the honest completion path.

## Consequences

- Design work finally has a completion path (`accept_design`) that does not fight
  [ADR-0009](0009-evidence-backed-conformance.md); the `in_review` design backlog
  (`a99ebf`, `056c39`, `4b479a72`) becomes the **accept queue** this bridge drains.
- `decompose_goal` and `plan_design_promotion` share one planner. New behaviour
  is pinned for create, cross-objective link, no-work, invalid normative task
  goals, idempotent retries, and append-only repairs; historical reconciliation
  still creates nothing and the whole path works with no local model.
- **Self-referential** (the good kind): once implemented, *this ADR* is the first
  design item the bridge accepts — accepting it decomposes it into its own
  implementation tasks.
- Interacts with [ADR-0020](0020-task-lifecycle-states.md) (taint is a disposition
  aligned with the lifecycle model) and the human channel; coordinate so there is
  one state model, not three.
- This ADR carries no behavioural code; it is the design-first predecessor for the
  implementation work it will itself define.
