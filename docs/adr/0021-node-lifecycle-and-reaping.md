# ADR-0021: Node lifecycle and maintenance reaping semantics

- Status: Proposed
- Date: 2026-07-23
- Deciders: MindLeak maintainers
- Related: [ADR-0002](0002-sqlite-decay-over-vector-llm.md) (decay is the point),
  [ADR-0005](0005-signal-weighted-decay.md) / [ADR-0012](0012-derived-signal-evidence.md)
  (signal-protected edges), [ADR-0007](0007-structural-snapshot-reconciliation.md)
  (artifact-owned structure, stubs), [SPEC.md](../SPEC.md) §4

## Context

Decay hides edges at query time; `prune` deletes the decayed ones and reaps nodes
left unreferenced. [SPEC.md](../SPEC.md) §4 states the reaping rule in one line
("unreferenced execution, symbol, package, and unresolved artifact-stub nodes are
dropped") but does not define, per node type, *what counts as referenced*, *when*
a node is reaped relative to edge deletion, or the intended fate of `artifact`,
`intent`, and `agent` nodes. That ambiguity produced a scare and a genuine open
question:

- **The scare (not a code bug).** A live experiment showed an 8-day-old
  `execution` node surviving `prune` with its decayed edge gone (`nodes_removed:0`).
  Reading the source, `graph/signal.rs::prune_with_signal` already deletes orphan
  `execution` / `symbol` / `package` nodes **after** edge deletion in the same
  transaction, and the test `prune_removes_decayed_edges_and_orphan_executions`
  passes. The live miss was a **stale running `mindleak-mcp` binary**, not a logic
  defect — a dogfood-hygiene issue, not an ordering bug. This ADR pins the ordering
  as an invariant so it cannot regress, and records that the running server must
  match the built source.
- **The real open question.** When decay reaps an *artifact's* last edge, is the
  artifact swept or retained? And what about `intent` and `agent` nodes? The code
  answers implicitly (by omission); this ADR makes the contract explicit so
  "unbounded node growth" is understood as *durable structure*, not a leak.

## Decision

Reaping is **derived, prune-time only, and edge-deletion-first**. Define the
per-type contract explicitly.

### Ordering invariant (pin, do not regress)

Within one `prune` transaction: delete below-threshold, non-signal-protected edges
**first**, then detect and delete orphans against the post-deletion edge set. A
node orphaned by the current pass is reaped in the **same** pass — no "second
prune" is ever required. Signal-protected near-expiry edges
([ADR-0005](0005-signal-weighted-decay.md)) are retained, so their endpoints
survive until `consolidate_signal` resolves them.

### Per-type reaping

| Node type | Reaped when… | Rationale |
|---|---|---|
| `execution` | it has no edge (source or target) after edge deletion | episodic act; pure noise once its edges decay |
| `symbol` | it has no edge after edge deletion | structural detail; meaningless without its owning artifact/edges |
| `package` | it has no edge after edge deletion | external dep; meaningless without a `depends_on`/`imports` edge |
| `artifact` **stub** (in `artifact_stubs`: referenced but never ingested as a real file) | it becomes orphan | a placeholder that never resolved to a real file |
| `artifact` **real** (ingested via `ingest_file`) | **never** (retained) | models a file that exists / may be re-ingested; a stable, human-readable id worth keeping; recreating it churns ids |
| `intent` | **never** (retained) | durable decision/rationale/commit history — the opposite of noise; erasing it would delete *why* |
| `agent` | **never** (retained) | the roster; attention *decays via its `observed` edges*, but the node persists so `list_agents` stays stable |

Consequently the node set is **bounded by durable structure** (real artifacts ≈
files, intents ≈ decisions/commits, agents ≈ roster) plus **transient episodic
nodes that self-reap** as their edges decay. Growth in the durable tier is
expected and correct — it mirrors the repo and its decision history — not a leak.

### Invariants

- Reaping **never** deletes a node still reachable by an above-threshold edge
  (guaranteed by detecting orphans only *after* deleting sub-threshold edges, and
  by protecting signal edges).
- Effective weight is **never** stored; orphan detection is a derived `NOT IN
  (SELECT … FROM edges)` query, never a maintained reference-count column.

### Rejected alternatives

- **Reference-counting columns on `nodes`.** A stored count must be updated on
  every edge upsert *and* every decay-driven deletion — a persisted derived value
  that violates "derived, never stored" ([ADR-0002](0002-sqlite-decay-over-vector-llm.md))
  and will drift. Derived orphan queries at prune time are correct.
- **A background sweeper** continuously deleting orphans. Reintroduces background
  row rewrites the project forbids; prune-time-only keeps maintenance explicit and
  bounded.
- **Reaping real (ingested) artifacts when edgeless.** They model files that still
  exist; deleting and recreating their nodes churns stable ids and loses the
  re-ingestion anchor. Retain.
- **Reaping `intent` when edgeless.** Would erase durable rationale/decision
  history — the durable counterpart to decaying memory. Retain.

## Consequences

- SPEC.md §4 is updated to reference this per-type contract and the ordering
  invariant; the existing one-liner stays accurate for execution/symbol/package/
  stub and is clarified to state that real artifacts, intents, and agents are
  retained by design.
- `task:69100fe4d1c8` (the "orphan-node leak" fix) is re-scoped: the execution
  orphan reaping already works in source, so its remaining, real deliverables are
  (a) a **dogfood rebuild/version check** so the running server matches source,
  and (b) a regression test asserting a decayed **artifact stub** is reaped while a
  real ingested artifact and an intent are retained — codifying this contract.
- New tests pin the ordering invariant (single-pass reap) and each retention rule,
  with exhaustive coverage of the per-type table.
- This ADR carries no behavioural code; it is the design-first predecessor for the
  re-scoped `task:69100fe4d1c8`.
