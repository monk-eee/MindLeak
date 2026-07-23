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

### `mindleak-storage` (library)

Shared platform-independent SQLite online backup and integrity verification
(ADR-0013). Both planes call this primitive through their own stores; reset and
export remain plane-specific operations.

### `mindleak-core` (library)

The engine. Modules:

| Module | Responsibility |
|---|---|
| [`config.rs`](../crates/mindleak-core/src/config.rs) | Strict, layered startup configuration for bounded per-project decay policy (ADR-0014). |
| [`model.rs`](../crates/mindleak-core/src/model.rs) | `Node`, `Edge`, `NodeType`, `RelationType`, per-relation half-lives. |
| [`schema.sql`](../crates/mindleak-core/src/schema.sql) | SQLite tables, indexes, FTS5 virtual table + sync triggers. |
| [`db.rs`](../crates/mindleak-core/src/db.rs) | Connection setup (WAL, FKs), migrations, and the `effective_weight()` scalar SQL function. |
| [`decay.rs`](../crates/mindleak-core/src/decay.rs) | The half-life decay formula and prune threshold. |
| [`graph/`](../crates/mindleak-core/src/graph/mod.rs) | `GraphStore`: shared `types`, atomic `writes`, decay-aware `query`, derived `signal`, conformance `evidence`, and `lifecycle` operations. |
| [`ingest/`](../crates/mindleak-core/src/ingest/mod.rs) | Zero-token deterministic extractors: `execution`, `git`, `ast`, `structure/{imports,hierarchy}` (JS/TS imports and type hierarchy), and `manifest` (direct package dependencies). |
| [`consolidate.rs`](../crates/mindleak-core/src/consolidate.rs) | Optional Ollama consolidation worker. |
| [`embed.rs`](../crates/mindleak-core/src/embed.rs) | Optional semantic-recall embedding index (ADR-0008): local `/v1/embeddings` client, derived `embeddings` table, cosine recall. Off the zero-token write path. |
| [`net.rs`](../crates/mindleak-core/src/net.rs) | Network resilience for optional HTTP (ADR-0010): timeouts, bounded retry with backoff, per-endpoint circuit breaker. |
| [`telemetry.rs`](../crates/mindleak-core/src/telemetry.rs) | Observability (ADR-0010): durable `telemetry_events` audit trail, metrics snapshot, stderr-only `tracing` init. |
| [`lib.rs`](../crates/mindleak-core/src/lib.rs) | `MindLeak` facade wiring; behavior is grouped under `facade/`: `ingestion`, `query`, `observability`, `lifecycle`, and `consolidation`. |

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
model), and `lib` (the `Lodestar` facade wiring). Facade behavior is grouped
under `facade/`: `constitution`, `executive`, `conformance`, and `knowledge`.

### `lodestar-mcp` (binary)

A second MCP stdio server exposing the Intent Plane (23 tools: constitution,
tasks, conformance, knowledge). Same newline-delimited JSON-RPC as `mindleak-mcp`.

### `editors/vscode` (extension)

Passive editor, shell-execution, workspace-mutation, and Git commit sensors plus
a Cytoscape visualizer. It spawns `mindleak-mcp` as a child process and speaks
the same MCP protocol. Stable shell execution events require VS Code 1.93;
unsupported shells are visibly degraded rather than inferred from terminal text.
Platform-targeted VSIX packages contain both native servers under `bin/` and
report memory, intent, terminal, and Git health independently (ADR-0016). A
Telemetry pane renders a derived, real-time effectiveness readout (graph size,
tool success/error rates, latency, per-tool metrics) from `graph_stats` and
`telemetry_snapshot`, with opt-in live event logging; the derivations are the
pure helpers in `src/util.ts`.

## Data model

- **Nodes** — `symbol` · `artifact` · `execution` · `intent` · `agent` ·
  `package` (ADR-0006). Ids are stable and human-readable
  (`artifact:src/auth.ts`, `symbol:src/auth.ts:validateSession`).
- **Edges** — directional, decay-weighted: `contains` · `calls` · `modified` ·
  `failed_on` · `refactored` · `relates_to` · `observed` · `imports` ·
  `extends` · `implements` · `depends_on` (ADR-0006 phases 1-3).

## Decay

Effective weight is computed at query time, never by rewriting rows:

```
W_effective = W_base · 2^(−Δt_hours / (half_life_hours · signal_multiplier))
```

Raw execution evidence uses a 24h half-life; human intent 168h; default 48h.
Edges below the resolved threshold (`0.05` by default) are ignored in queries
and purged by `prune_graph`. Base half-lives and the threshold can be tuned in a
strict `.mindleak.toml` or by environment (ADR-0014); the immutable policy is
loaded once and applied at read time. Re-ingesting an edge reinforces it
(`+0.05`, capped at 1.0) and resets its decay clock. Structural edges additionally carry artifact ownership:
re-ingesting a file replaces that owner's structural snapshot, retracting facts
that disappeared (ADR-0007). `boost_entity` changes attention without refreshing
unrelated incident evidence.

**Signal-weighted decay (ADR-0005/0012).** At query/prune time, `GraphStore`
derives raw `SignalEvidence` from reinforcement span, independent source classes,
failure/change/success consequence, surprise, incoming structural degree, and
explicit decisions. `decay::signal_multiplier` maps those proxies to a bounded
1x-8x half-life multiplier. Returned edges expose the evidence/multiplier for
auditability; neither multiplier nor effective weight is stored. Near-expiry
high-signal episodics are returned by `prune_graph`; expired candidates remain
inactive but retained until optional `consolidate_signal` persists an intent and
acknowledges them, leaving model access off deterministic maintenance.

**Working memory (ADR-0017 phase 1).** `GraphStore::working_set` derives a
per-agent, capacity-bounded focus view from active `observed` edges. No buffer or
LRU is persisted. Repeated observations spanning the existing signal window
become rehearsal evidence only while the target remains inside that agent's
top-K; the write path remains zero-token.

**Autonomous consolidation (ADR-0017 phase 2).** An off-by-default scheduler in
`mindleak-mcp` tracks stdio request activity with a condition variable. After a
bounded idle period it calls the same `MindLeak::consolidate_signal` path through
a second file-backed SQLite connection. Model output becomes deterministic graph
facts; one optimistic transaction stores the gist/provenance and deletes only
candidate edge versions that have not changed meanwhile. Every attempt emits
maintenance telemetry. A persisted workspace lease gates both manual and idle
model calls immediately before inference, preventing duplicate spend across MCP
processes. EOF wakes waiting workers; a bounded grace joins normal exits while a
currently blocked HTTP attempt may be abandoned for process termination without
post-cancellation persistence.

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
- **structure** (ADR-0006) — shipped phases 1-2 parse static
  JavaScript/TypeScript `import` and `require` declarations into `imports`,
  `package`, and named cross-file `calls` facts, plus simple named class/interface
  heritage into `extends`/`implements`. A lightweight lexer excludes comments,
  strings, templates, member calls, generic constraints, and basic lexical
  shadowing. Unresolved relative targets store deterministic candidate ids;
  ingesting a real candidate atomically retargets structural symbol edges and
  removes the stub.
- **manifest** (ADR-0006 phase 3) — direct dependencies from `Cargo.toml`,
  `package.json`, `go.mod`, and `requirements*.txt` become artifact-to-package
  `depends_on` edges. TOML, JSON, and PEP 508 use structured parsers; Go uses its
  narrow `require` grammar. Re-ingestion retracts removed dependencies, while a
  malformed supported manifest fails before replacing its last valid snapshot.

## Optional LLM layer

`consolidate.rs` calls a local, OpenAI-compatible model server
(`/v1/chat/completions`) with a JSON `response_format` to compress a batch of raw
logs into a single `intent` node. It is asynchronous and never on the hot path;
pointed at a local server, nothing leaves the machine.

See [SPEC.md](SPEC.md) for the full design rationale.
