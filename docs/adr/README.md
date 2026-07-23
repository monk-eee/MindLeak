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
| [0016](0016-platform-packaging-and-registration.md) | Platform packaging and workspace registration | Accepted |

## Writing a new ADR

1. Copy an existing file to `NNNN-short-title.md` (next number).
2. Fill in Context / Decision / Consequences.
3. Add a row above. Link it from the code or doc it constrains.
