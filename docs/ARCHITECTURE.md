# Architecture

MindLeak is a **Temporal Context Graph Engine (TCGE)** with two planes: an
**episodic memory graph** (`mindleak-*`) whose edges decay, and a durable
**Intent Plane** (`lodestar-*`, ADR-0004) that does not. Each plane is a Rust
library behind its own MCP stdio server; everything communicates only over the
Model Context Protocol — no shared memory, no sockets beyond stdio.

```
Agents · VS Code ─┬─ MCP/stdio ─▶ mindleak-mcp ─▶ mindleak-core ─▶ .mindleak/graph.db  (decays)
                  │                                     └── async ──▶ Ollama (optional)
                  └─ MCP/stdio ─▶ lodestar-mcp ─▶ lodestar-core ─▶ .lodestar/spec.db   (durable)
```

## Crates

### `mindleak-core` (library)

The engine. Modules:

| Module | Responsibility |
|---|---|
| [`model.rs`](../crates/mindleak-core/src/model.rs) | `Node`, `Edge`, `NodeType`, `RelationType`, per-relation half-lives. |
| [`schema.sql`](../crates/mindleak-core/src/schema.sql) | SQLite tables, indexes, FTS5 virtual table + sync triggers. |
| [`db.rs`](../crates/mindleak-core/src/db.rs) | Connection setup (WAL, FKs), migrations, and the `effective_weight()` scalar SQL function. |
| [`decay.rs`](../crates/mindleak-core/src/decay.rs) | The half-life decay formula and prune threshold. |
| [`graph.rs`](../crates/mindleak-core/src/graph.rs) | `GraphStore`: upsert, structural snapshot reconciliation, FTS search, decay-aware neighbours, BFS traversal, snapshot, prune. |
| [`ingest/`](../crates/mindleak-core/src/ingest/mod.rs) | Zero-token deterministic extractors: `execution`, `git`, `ast`, `structure` (JS/TS imports). |
| [`consolidate.rs`](../crates/mindleak-core/src/consolidate.rs) | Optional Ollama consolidation worker. |
| [`lib.rs`](../crates/mindleak-core/src/lib.rs) | `MindLeak` facade: ingestion + the three agent-facing queries. |

### `mindleak-mcp` (binary)

A minimal MCP stdio server (newline-delimited JSON-RPC 2.0). Handles
`initialize`, `tools/list`, `tools/call`, `ping`, `shutdown`. Tool definitions
and dispatch live in [`tools.rs`](../crates/mindleak-mcp/src/tools.rs).

### `lodestar-core` (library) — the Intent Plane

The **durable** counterpart to the decay graph (ADR-0004): a separate crate and
store so the zero-token decay engine stays uncontaminated. Modules: `model`
(goals/tasks/knowledge), `schema.sql`, `db` (+ a knowledge `effective_weight`
scalar), `decay` (long-horizon revalidation), `store` (`LodestarStore`: the
constitution, the task ledger with the atomic claim/lease compare-and-swap, the
goal↔code seam, conformance audit, and learned knowledge), `llm` (optional local
model), and `lib` (the `Lodestar` facade + conformance and gated consolidation).

### `lodestar-mcp` (binary)

A second MCP stdio server exposing the Intent Plane (21 tools: constitution,
tasks, conformance, knowledge). Same newline-delimited JSON-RPC as `mindleak-mcp`.

### `editors/vscode` (extension)

A passive sensor + Cytoscape visualizer that spawns `mindleak-mcp` as a child
process and speaks the same MCP protocol.

## Data model

- **Nodes** — `symbol` · `artifact` · `execution` · `intent` · `agent` ·
  `package` (ADR-0006). Ids are stable and human-readable
  (`artifact:src/auth.ts`, `symbol:src/auth.ts:validateSession`).
- **Edges** — directional, decay-weighted: `contains` · `calls` · `modified` ·
  `failed_on` · `refactored` · `relates_to` · `observed` · `imports` (JS/TS,
  ADR-0006 phase 1). **Planned:** `depends_on` · `extends` · `implements`
  (ADR-0006, in build).

## Decay

Effective weight is computed at query time, never by rewriting rows:

```
W_effective = W_base · 2^(−Δt_hours / half_life_hours)
```

Raw execution evidence uses a 24h half-life; human intent 168h; default 48h.
Edges below `0.05` effective weight are ignored in queries and purged by
`prune_graph`. Re-ingesting an edge reinforces it (`+0.05`, capped at 1.0) and
resets its decay clock. Structural edges additionally carry artifact ownership:
re-ingesting a file replaces that owner's structural snapshot, retracting facts
that disappeared (ADR-0007). `boost_entity` changes attention without refreshing
unrelated incident evidence.

**Signal-weighted decay (ADR-0005).** The half-life is not fixed. An edge
reinforced at least 3 times across a ≥48h span earns a longer half-life via
`signal_half_life()` — derived at query time from the edge's `reinforcement_count`
and `first_seen` — so corroborated-over-time signal resists decay while one-offs
and same-session spam fade on the base clock ("decay noise, not signal"). Only the
raw count and first-seen timestamp are stored; the effective weight stays derived.

## Ingestion (zero-token)

All write-path extraction is pure pattern matching:

- **execution** — command + exit code → `execution` node; changed files →
  `modified` edges; stack-trace `path:line` regex on failure → `failed_on` edges.
- **git** — commit → `intent` node; changed files → `refactored` edges;
  `DECISION:`/`HACK:`/`WHY:` markers extracted into node content.
- **ast** — heuristic extraction (pattern-based per language) → `symbol` nodes +
  `contains` edges, plus **in-file `calls` edges** (a definition body referencing
  another symbol defined in the same file). The complete result transactionally
  replaces the artifact's prior structural snapshot. Structured behind a
  swappable interface; Tree-sitter is the precision upgrade for cross-file/scoped
  calls.
- **structure** (ADR-0006, in build) — `imports` from `use`/`import`/`require`
  statements (→ cross-file `calls` + `package` nodes), `extends`/`implements`
  from inheritance, and `depends_on` from manifests. Same zero-token extractor;
  gives cross-file impact analysis and the structural substrate ADR-0005 needs.

## Optional LLM layer

`consolidate.rs` calls a local, OpenAI-compatible model server
(`/v1/chat/completions`) with a JSON `response_format` to compress a batch of raw
logs into a single `intent` node. It is asynchronous and never on the hot path;
pointed at a local server, nothing leaves the machine.

See [SPEC.md](SPEC.md) for the full design rationale.
