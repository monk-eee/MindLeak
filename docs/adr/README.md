# Architecture Decision Records

This log captures decisions that are **hard to reverse or surprising** — the
kind someone might otherwise "simplify" back into a bug. Each ADR is dated and
immutable; supersede rather than edit.

Format: [MADR](https://adr.github.io/madr/)-lite. Keep them short.

| ADR | Title | Status |
|---|---|---|
| [0001](0001-record-architecture-decisions.md) | Record architecture decisions | Accepted |
| [0002](0002-sqlite-decay-over-vector-llm.md) | SQLite + half-life decay over vector-only / per-event LLM memory | Accepted |
| [0003](0003-agent-attribution-as-observed-edges.md) | Agent attribution as decay-weighted `observed` edges | Accepted |
| [0004](0004-intent-plane-spec-brain.md) | Intent Plane: a durable "spec brain" separate from the decay graph | Accepted |
| [0005](0005-signal-weighted-decay.md) | Signal-weighted decay ("decay noise, not signal") | Accepted |
| [0006](0006-structural-dependency-edges.md) | Structural & dependency edges (graph enrichment for impact analysis) | Accepted |
| [0007](0007-structural-snapshot-reconciliation.md) | Structural snapshots replace owned facts | Accepted |
| [0008](0008-semantic-recall-embedding-index.md) | Optional semantic recall via a local embedding index | Accepted |
| [0009](0009-evidence-backed-conformance.md) | Evidence-backed conformance across the memory and intent planes | Accepted |
| [0010](0010-observability-and-resilience.md) | Observability, telemetry, and network resilience | Accepted |
| [0011](0011-passive-terminal-and-git-sensors.md) | Passive terminal and Git evidence sensors | Accepted |
| [0012](0012-derived-signal-evidence.md) | Derived bounded signal evidence | Accepted |
| [0013](0013-local-data-lifecycle.md) | Local data backup, export, and reset lifecycle | Accepted |
| [0014](0014-per-project-decay-configuration.md) | Per-project decay configuration | Accepted |
| [0015](0015-advisory-symbol-leases.md) | Progressive task handoffs before advisory symbol leases | Accepted (no symbol lease) |
| [0016](0016-platform-packaging-and-registration.md) | Platform packaging and workspace registration | Accepted |
| [0017](0017-working-memory-and-autonomous-consolidation.md) | Working-memory tier and autonomous consolidation cycle | Accepted (implemented) |
| [0018](0018-conflict-safe-concurrent-editing.md) | Conflict-safe concurrent editing in a shared working tree (worktrees optional) | Proposed |
| [0019](0019-task-retention-and-board-hygiene.md) | Task retention and board hygiene (archive, never decay) | Proposed |
| [0020](0020-task-lifecycle-states.md) | Task lifecycle states — `needs_input` and `paused` | Proposed |
| [0021](0021-node-lifecycle-and-reaping.md) | Node lifecycle and maintenance reaping semantics | Proposed |
| [0022](0022-learned-knowledge-loop.md) | Learned-knowledge loop — promotion, revalidation, advisory conformance | Proposed |
| [0023](0023-design-board-accept-bridge.md) | Design items, the Design Board, and the accept→decompose bridge | Proposed |
| [0024](0024-preflight-overlap-detection.md) | Pre-flight work-overlap detection across both planes | Proposed |
| [0025](0025-authoritative-checked-conformance.md) | Authoritative checked conformance | Accepted |
| [0026](0026-constitutional-policy-over-mechanistic-ratchets.md) | Constitutional policy over mechanistic ratchets | Proposed |

## Writing a new ADR

1. Copy an existing file to `NNNN-short-title.md` (next number).
2. Fill in Context / Decision / Consequences.
3. Add a row above. Link it from the code or doc it constrains.
