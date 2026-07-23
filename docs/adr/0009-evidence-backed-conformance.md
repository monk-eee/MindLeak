# ADR-0009 - Evidence-backed conformance across the memory and intent planes

- **Status:** Accepted
- **Date:** 2026-07-22

> **Amended by [ADR-0025](0025-authoritative-checked-conformance.md):**
> `check_conformance` now persists and returns the authoritative result;
> `complete_task` consumes that exact checked result without evaluating the
> optional semantic judge again.

## Context

[ADR-0004](0004-intent-plane-spec-brain.md) separated durable intent from
decaying episodic memory and made MindLeak the feedback sensor for Lodestar.
The current conformance implementation does not complete that loop: it derives
the files to check from a task's goal and treats the presence of a covering task
as alignment. It never proves what the agent changed, which commands succeeded
or failed, or whether that activity happened while the agent held the claim.

That creates false alignment. A task can complete without touching its
acceptance surface, an unrelated change can be presented as sanctioned, and a
covering task can hide an invariant violation. Letting Lodestar query MindLeak's
tables directly would fix the symptom by breaking the loose-store boundary.
Making agents self-report an unstructured list of files would preserve the
boundary but would not provide auditable evidence.

The two planes need a small, versioned evidence contract. MindLeak should
produce a bounded account of what happened; Lodestar should validate that
account against the current claim and governing intent, store the proof with the
verdict, and own the resulting task transition.

## Decision

### Versioned evidence bundle

MindLeak exposes one evidence query and returns this wire-level value object:

```text
ConformanceEvidence {
  schema_version, task_id, agent_id, started_at, ended_at,
  changed_node_ids[], failed_node_ids[],
  execution_ids[], successful_execution_ids[], commit_ids[],
  summary, provenance[]
}
```

- `schema_version` starts at `1`; readers reject unsupported versions rather
  than guessing.
- Every node id is an opaque MindLeak id. `provenance` identifies the MindLeak
  nodes and relations from which each claim was derived.
- The bundle is bounded in count and text size before crossing MCP. Semantic
  conformance receives the bounded summary, never an unbounded log or a
  comma-separated list of ids.
- `changed_node_ids` come from mutation evidence such as `modified` and
  `refactored`. An ADR-0003 `observed` edge establishes attribution only; focus
  or observation alone never proves a change.

MindLeak builds the bundle with `evidence_for(agent, since, until)`. It follows
the existing `agent:<id> --observed--> execution|intent` edges, then their
episodic `modified`, `failed_on`, and `refactored` edges. It does not add an
origin column, a second event store, or a durable copy of raw episodes. The
bounded work-window query may read matching raw evidence regardless of its
general retrieval rank; after conformance, normal decay and pruning still apply.

### Claim-bounded identity and time

Lodestar records `claim_started_at` when a task is first won or reclaimed; lease
renewal does not move that boundary. Completion accepts evidence only when:

- `task_id` identifies the task being completed;
- `agent_id` matches the current owner and the configured agent identity;
- the claim is still live; and
- the evidence interval is contained within the current claim interval.

`LODESTAR_AGENT` is the server-side default identity. If a caller also supplies
an agent id, a mismatch is rejected rather than silently attributed. A new
owner or reclaimed lease starts a new evidence window.

### Minimal deterministic code policy

The goal-to-code seam gains a `mode` with two values:

- `governed` (default): changing the node requires a task serving that goal;
- `forbid_change`: changing the node while the goal is active is a deterministic
  violation. This mode is valid only for constraints and invariants.

This is deliberately not a general policy language. Richer deterministic rules
need their own evidence, fixtures, and decision record.

### Verdicts own task transitions

Conformance uses the following exhaustive transition table:

| Condition | Verdict | Task state |
|---|---|---|
| Evidence is missing, malformed, outside the claim, or insufficient | `needs_human` | `in_review` |
| Governed code changed with no covering task, or the task serves another goal | `drift` | `in_review` |
| An active `forbid_change` binding was changed | `violation` | `blocked` |
| A required semantic judgment is unavailable or uncertain | `needs_human` | `in_review` |
| Evidence covers the task goal and all deterministic checks pass | `aligned` | `done` |

A covering task alone can never produce `aligned`. Only `aligned` reaches
`done`; non-violation ambiguity remains reviewable rather than being silently
accepted.

`check_conformance(evidence, task_id?)` performs conformance and persists one
authoritative audit result. Per ADR-0025,
`complete_task(task_id, agent_id, evidence, check)` verifies and consumes that
exact result without invoking the semantic judge again, then rechecks the live
owner/status and atomically applies the resulting state transition.

### Durable audit, loose stores

Lodestar stores the versioned evidence bundle with each conformance record. The
stored bundle is a bounded proof of a coordination decision, not a replacement
history for MindLeak. The databases share no tables, foreign keys, connection,
or transaction. The local stdio threat model remains unchanged: provenance is
auditable, not cryptographically attested.

## Consequences

- Lodestar can distinguish sanctioned work from work that merely has a task
  nearby, and every completion verdict retains its evidence after raw episodes
  decay.
- Task claims need a stable start timestamp; conformance records need evidence
  schema/version and provenance fields; goal-code links need a mode migration.
- MindLeak gains one read-side evidence bundle API over existing graph facts.
  Passive terminal and Git sensors feed the existing ingestion APIs rather than
  writing a second telemetry pipeline.
- Commit-backed `refactored` evidence is the first trustworthy source of changed
  files. Supporting uncommitted editor changes later requires explicit episodic
  mutation evidence; `observed` must not be reinterpreted as `modified`.
- Missing model access cannot fabricate alignment. Semantic uncertainty is
  `needs_human`, while deterministic checks continue to run without a model.
- Integration tests must reach all four verdicts, reject cross-agent and
  out-of-window evidence, and prove that a covering task with no evidence does
  not complete.

## Rejected alternatives

- **Lodestar reads the MindLeak database:** violates ADR-0004's loose seam and
  couples migrations, availability, and transactions across planes.
- **Agents submit arbitrary changed-file lists:** self-report without graph
  provenance is not evidence.
- **Task presence implies alignment:** this is the false-positive behavior the
  decision removes.
- **Add origin columns to graph rows:** duplicates and contradicts ADR-0003's
  merge-safe attribution edges.
- **Build a generic conformance DSL now:** expands the surface before the first
  end-to-end evidence loop is proven.
