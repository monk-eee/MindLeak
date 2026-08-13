# Memory Plane Specification — Temporal Context Graph Engine (TCGE)

**Codename:** MindLeak
**Role:** The local, event-driven memory plane of MindLeak. It ingests executions,
commits, files, symbols, and structural relationships deterministically — **zero
model tokens on the write path** — into a decay-weighted directional graph and
exposes bounded retrieval over MCP. Optional OpenAI-compatible endpoints support
asynchronous consolidation and semantic recall without becoming authoritative.

This plane is a structured alternative to flat event history and vector-only
retrieval. It maps explicit **nodes** and **edges** whose weights **decay on an
exponential half-life**, so stale episodes lose influence instead of accumulating
forever. Embeddings are not rejected; an optional index
([ADR-0008](adr/0008-semantic-recall-embedding-index.md)) ranks candidate entry
nodes, after which graph traversal supplies explicit relationships and provenance.

> **Scope.** This document specifies the *episodic* plane — memory of the act,
> which decays. Its durable counterpart, the **Intent Plane** (authoritative
> spec + task coordination for parallel agents), is specified separately in
> [SPEC-INTENT.md](SPEC-INTENT.md) / [ADR-0004](adr/0004-intent-plane-spec-brain.md).

---

## 1. Why a graph (not vectors, not logs)

| Layer | Question it answers | Handled by |
|---|---|---|
| Structural / ontological | "How do entities relate?" (`A depends_on B`) | **MindLeak graph** |
| Temporal / execution | "What happened, when, in what order?" | MindLeak `execution` nodes |
| Semantic recall | "What is conceptually near this query?" | Optional embedding index |
| Working memory / write-gate | "What is worth keeping?" | **Decay + prune** |

Vector similarity ranks semantic proximity; by itself it does not encode a
directional dependency path or prove why two entities are related. Flat logs
preserve chronology but not topology. MindLeak combines explicit structure with
time and provenance, then forgets low-value episodes on purpose.

Embeddings therefore have a **subordinate** role: an optional index
([ADR-0008](adr/0008-semantic-recall-embedding-index.md)) answers *"what is
semantically near this phrase?"* to pick an entry node, then the graph reasons
from there. Similarity proposes where to look; decay-weighted traversal explains
what is connected.

---

## 2. System topology

```
┌──────────────────────────────────────────────────────┐
│ MCP clients: coding agents · CLI · VS Code extension │
│ Optional extension sensors: focus · save · shell · Git │
└──────────────────────────┬───────────────────────────┘
                           │ MCP over stdio
                           ▼
┌──────────────────────────────────────────────────────┐
│ mindleak-mcp → mindleak-core                         │
│ deterministic ingest · traversal · decay · evidence  │
└───────────────────┬──────────────────┬───────────────┘
                    │                  └ - optional async - ▶ OpenAI-compatible
                    ▼                                         endpoint
           graph.db (SQLite + FTS5)
```

---

## 3. Zero-token deterministic ingestion

### Node types

| Type | Id form | Source |
|---|---|---|
| `symbol` | `symbol:<path>:<name>` | heuristic extraction — definitions + call sites |
| `artifact` | `artifact:<path>` | file / config / test |
| `execution` | `execution:<hash>` | terminal command + exit code |
| `intent` | `intent:<sha\|hash>` | commit, decision, tradeoff |
| `agent` | `agent:<id>` | an AI agent / client session (optional attribution) |
| `package` | `package:<name>` | external dependency (non-workspace) |

### Edge types & extraction triggers (0-token)

| Source → Target | Relation | Trigger |
|---|---|---|
| Execution → Artifact | `modified` | files changed during the command window |
| Execution → Artifact/Symbol | `failed_on` | stack-trace regex on non-zero exit |
| Artifact → Symbol | `contains` | symbols defined in the file |
| Symbol → Symbol | `calls` | a definition body references another symbol defined in the same file |
| Intent → Artifact/Symbol | `refactored` | commit diff ∩ symbol boundaries |
| Intent → * | `relates_to` | explicit recorded decision |
| Agent → * | `observed` | an agent ingested or focused this node (attribution; decays) |
| Artifact → Artifact/Package | `imports` | shipped for static JS/TS `import` and `require` declarations |
| Artifact → Package † | `depends_on` | shipped for direct deps in `Cargo.toml`, `package.json`, `go.mod`, and `requirements*.txt` |
| Symbol → Symbol | `extends` | shipped for simple named JS/TS class/interface inheritance |
| Symbol → Symbol | `implements` | shipped for simple named JS/TS class conformance |

> **`calls` scope.** In-file resolution remains heuristic. ADR-0006 phase 1 also
> resolves direct calls to named JS/TS import bindings. Default/namespace calls,
> re-exports, path aliases, and other languages remain unsupported; precise,
> scope-accurate resolution remains the Tree-sitter upgrade (ADR-0002).

> **Structural enrichment — [ADR-0006](adr/0006-structural-dependency-edges.md).**
> Phase 1 is shipped for static JS/TS imports, packages, and named cross-file
> calls. Phase 2 is shipped for local and named-import JS/TS
> `extends`/`implements`; expression-based mixins and default/namespace heritage
> remain unsupported. Phase 3 is shipped for direct dependencies in the four
> supported manifest families; lockfiles and transitive catalogs are excluded.
> `references`, `consumes`, and `produces` remain deferred.

Episodic edges are append-and-reinforce: re-ingesting one raises its weight
(`+0.05`, capped at 1.0) and resets its decay clock. Structural extraction is an
authoritative per-artifact snapshot: re-ingestion transactionally retracts owned
facts absent from the latest source before orphan cleanup (ADR-0007).

---

## 4. Storage & decay

SQLite (bundled, single file) with **FTS5** for text seed lookup and a registered
`effective_weight()` scalar function so decay math runs in SQL.

Structural edges carry nullable `owner_id` provenance so one artifact snapshot
can be reconciled atomically. Episodic edges have no structural owner.

```
W_effective = W_base · 2^(−Δt_hours / half_life_hours)
```

* `Δt` — hours since the edge was last reinforced (`updated_at`).
* `half_life` — 24h for raw execution evidence, 168h for durable structure
  (`contains`, `calls`, `imports`, `extends`, `implements`, `depends_on`) and
  human intent, 48h default (`relates_to`, `observed`). ADR-0014 permits bounded
  per-relation overrides from `.mindleak.toml` and environment variables.
* **Prune rule:** edges below the resolved threshold (`0.05` by default, with a
  bounded ADR-0014 project override) are ignored at query time and purged during
  maintenance; then, in the same transaction, unreferenced `execution`, `symbol`,
  `package`, and unresolved artifact-stub nodes are dropped. Real (ingested)
  `artifact`, `intent`, and `agent` nodes are **retained** by design — durable
  structure, not noise. Orphan detection runs *after* edge deletion so a node
  orphaned by the pass is reaped in that pass; effective weight is never stored.
  Full per-type contract: [ADR-0021](adr/0021-node-lifecycle-and-reaping.md).

Schema: [`crates/mindleak-core/src/schema.sql`](../crates/mindleak-core/src/schema.sql).

> **Signal-weighted decay (ADR-0005).** Pure time-decay treats signal and noise
> alike; frequency and recency are weak proxies. *Signal-weighted decay*
> ([ADR-0005](adr/0005-signal-weighted-decay.md)) adds an evidence term so proven
> signal resists decay and noise fades — "decay noise, not signal". **Shipped
> (ADR-0012):** a derived, bounded 1x-8x half-life multiplier over
> span-qualified reinforcement, independent source diversity, consequence,
> surprise, structural centrality, and explicit decisions. Effective weight is
> never stored. `prune_graph` surfaces near-expiry proven signal with provenance
> and retains expired candidates until optional `consolidate_signal` succeeds.

> **Project decay policy (ADR-0014).** The server resolves defaults, a strict
> committable `.mindleak.toml`, and environment overrides once at startup.
> `GraphStore` applies the resolved base half-life and threshold at read time in
> traversal, signal handoff, counts, snapshots, exports, and prune. Existing edge
> rows are not rewritten, so a policy change takes effect retroactively without
> storing effective weight.

---

## 5. MCP tool surface

**Agent-facing (spec core):**

1. `graph_multi_hop_query(seed_entity, max_depth=2, min_weight=0.2)` — traverse N
   hops from a node id or FTS phrase; returns nodes, decayed edges, and scores.
2. `get_impact_radius(target_artifact)` — relation-aware depth-2 blast radius:
  dependents, prior failing executions, related intents; `observed` attention
  edges are excluded from dependency expansion.
3. `record_architectural_decision(decision_text, related_nodes[])` — write an
   intent node linked to affected nodes.

**Ingestion & maintenance (also over MCP):** `ingest_execution`, `ingest_commit`,
`ingest_file`, `boost_entity`, `graph_snapshot`, `prune_graph`, `graph_stats`,
`consolidate_session`, `list_agents`, `working_set`.

**Working memory (ADR-0017 phase 1):** `working_set(limit?, session_id)` requires
a token previously registered by `open_session` and returns that session's
highest active `observed` targets, ranked by effective attention and hard-capped
at the startup-resolved `MINDLEAK_WORKING_SET_SIZE` (default 7, bounded 1-32).
The view is derived, never stored. Observation count/span are exposed, and
sustained active observation contributes rehearsal signal only while the target
remains inside that session's top-K. Autonomous idle consolidation is phase 2
and is not implied by this tool.

`prune_graph` returns deletion counts plus `signal_candidates`; deterministic
maintenance never invokes a model. `consolidate_signal` uses the configured
optional endpoint, persists an intent and provenance links, then acknowledges
raw candidates only after success.

**Local data lifecycle (ADR-0013):** `backup_database(path)` exists on each
plane and uses SQLite online backup plus integrity verification. MindLeak also
exposes `export_graph()` for human-readable active graph JSON and
`reset_database(confirm="RESET MINDLEAK")`; Lodestar reset requires the distinct
`RESET LODESTAR` token. Export is not a restorable backup, live restore is not
supported, and resetting one plane never touches the other.

**Optional semantic recall (ADR-0008):** `index` embeds nodes lacking a current
vector; `recall(query, limit)` returns the nearest node ids by cosine similarity
— entry points to *seed* `graph_multi_hop_query`, not a replacement for it.

---

## 6. Optional model augmentation (off-path, asynchronous)

The deterministic pipeline owns the write path. An optional consolidation worker
calls a configured model through the **OpenAI-compatible**
`/v1/chat/completions` API. Ollama, LM Studio, llama.cpp, and compatible hosted
services can satisfy that contract. A strict JSON `response_format` asks it to:

- **Sleep-phase consolidation** — compress N raw execution nodes into one
  high-level `intent` node, then prune the noise.
- **Signal consolidation** — distil near-expiry proven failure/refactor evidence
  and preserve deterministic provenance links before acknowledging raw details.

Sleep-phase consolidation asks the model to choose one relation per impacted file
from a closed set — `fixed` (fix/bug work), `relates_to` (a `DECISION:`/`WHY:`
rationale marker), or `refactored` (the default). The deterministic layer is
authoritative: any omitted, unknown, or structurally-invalid relation is coerced
to `refactored`, so the engine — never the model's free text — decides the
persisted `RelationType`.

Configuration is optional; defaults target a local Ollama endpoint:

| Variable | Default | Meaning |
|---|---|---|
| `MINDLEAK_LLM_URL` | `http://localhost:11434/v1` | OpenAI-compatible base URL (Ollama's `/v1`) |
| `MINDLEAK_MODEL` | `glm4:9b` | model name |
| `MINDLEAK_LLM_API_KEY` | *(empty)* | bearer token for hosted servers; Ollama ignores it |
| `MINDLEAK_AUTONOMOUS_CONSOLIDATION` | `false` | explicit opt-in for idle consolidation |
| `MINDLEAK_CONSOLIDATE_IDLE_SECS` | `300` | idle trigger (30-86400) |
| `MINDLEAK_CONSOLIDATE_MIN_INTERVAL_SECS` | `3600` | minimum attempt interval (60-86400) |
| `MINDLEAK_CONSOLIDATE_MAX_NODES` | `20` | bounded candidates per pass (1-200) |

No model request occurs on deterministic ingest or query. Data stays on the
machine when the configured endpoint is local and leaves it when the operator
chooses a remote endpoint. `consolidate_session` errors cleanly when no model is
reachable. ADR-0017 phase 2 optionally schedules the existing
`consolidate_signal` path after idle. The worker is absent unless explicitly
enabled, uses a second file-backed SQLite connection, emits maintenance
telemetry, and joins on server exit. Model inference occurs outside SQLite;
resulting facts and optimistic acknowledgement of unchanged candidates commit in
one transaction. A persisted workspace lease gates manual and idle inference
with the same minimum interval. Gist output is bounded to 200 impacted files
(1024 bytes per path) and stores no raw model input. Waiting workers join on exit;
a blocked bounded HTTP attempt may be abandoned after shutdown grace, with
cancellation preventing later persistence if it returns before process exit.

### 6.1 Semantic-recall embedding index (ADR-0008)

A second optional augmentation is also off the write path. An async `index` pass
embeds graph nodes through the configured **OpenAI-compatible** `/v1/embeddings`
endpoint and stores vectors in a derived, recall-only `embeddings` table;
`recall` scores a query embedding against them by cosine similarity and returns
the nearest node ids to seed traversal. Embeddings are *derived* — regenerable,
never authoritative, and never consulted on the deterministic ingest/query hot
path.

| Variable | Default | Meaning |
|---|---|---|
| `MINDLEAK_EMBED_URL` | `http://localhost:11434/v1` | OpenAI-compatible embeddings base URL |
| `MINDLEAK_EMBED_MODEL` | `nomic-embed-text` | embedding model name |
| `MINDLEAK_EMBED_API_KEY` | *(empty)* | bearer token for hosted servers; Ollama ignores it |
| `MINDLEAK_AUTONOMOUS_INDEX` | `true` | refresh missing vectors on a wall-clock cadence |
| `MINDLEAK_INDEX_INTERVAL_SECS` | `300` | autonomous index cadence (30-86400) |
| `MINDLEAK_INDEX_BATCH` | `128` | nodes attempted per pass (1-1000) |

When the embedding endpoint is unavailable, explicit `index` calls and
autonomous-index telemetry name the model, URL, and remediation. The MCP
`recall` boundary instead returns FTS-seeded deterministic graph traversal in
its existing `results` field, with fallback metadata that recommends installing
and starting Ollama plus `ollama pull nomic-embed-text`, or configuring another
OpenAI-compatible endpoint. Ingestion, graph traversal, impact, decay, and prune
remain model-free.

### 6.2 Observability, telemetry & resilience (ADR-0010)

Observability is a first-class instrument: alongside tests, it is how an operator
confirms an agent did what was asked. Three parts, all local and all stdout-safe:

- **Structured tracing** to **stderr only** (stdout carries JSON-RPC), gated by
  `MINDLEAK_LOG` (filter, default `info`) and `MINDLEAK_LOG_FORMAT`
  (`pretty` | `json`). Every tool dispatch is a timed span.
- **Durable audit trail.** Every tool call is recorded to an append-only
  `telemetry_events` table the telemetry module owns — never graph state, never
  decayed. The `telemetry_snapshot` tool returns per-tool lifetime counts, error
  counts, latency, per-tool current health (whether the most recent call failed,
  distinct from the cumulative lifetime error tally), and recent events: the
  queryable record of what ran and whether it worked.
- **Network resilience** (`net`). All optional HTTP (embeddings, consolidation,
  LLM) gets explicit timeouts, bounded retry with backoff, and a per-endpoint
  circuit breaker, so a degraded server fast-fails instead of hanging the agent.

| Variable | Default | Meaning |
|---|---|---|
| `MINDLEAK_LOG` | `info` | tracing/`RUST_LOG`-style filter; `off` silences |
| `MINDLEAK_LOG_FORMAT` | `pretty` | `pretty` or `json` (both to stderr) |
| `MINDLEAK_HTTP_CONNECT_TIMEOUT_MS` | `1000` | connect/DNS budget per attempt, bounded 100-300000 ms |
| `MINDLEAK_HTTP_TIMEOUT_MS` | `30000` | response-read budget per optional HTTP attempt, bounded 100-300000 ms |
| `MINDLEAK_MODEL_TIMEOUT_MS` | `120000` | MindLeak model response-read budget, bounded 100-300000 ms |
| `MINDLEAK_HTTP_RETRIES` | `2` | extra attempts for generic transient failures, bounded 0-5; model calls instead retry one read timeout exactly once |
| `MINDLEAK_BREAKER_THRESHOLD` | `5` | consecutive failures before the circuit opens |
| `MINDLEAK_BREAKER_COOLDOWN_MS` | `30000` | how long the circuit stays open before a probe |

Telemetry is deterministic (no LLM calls), best-effort (a telemetry write failure
never changes a tool's result), and the breaker guards only optional endpoints —
the deterministic ingest/query path never touches the network.

---

## 7. VS Code extension

- **Passive editor sensor:** `onDidChangeActiveTextEditor` boosts the focused
  file's node; `onDidSaveTextDocument` ingests its symbols.
- **Passive execution sensor (ADR-0011):** on VS Code 1.93+, shell-integration
  start/end events ingest command, exit code, portable changed-file delta, and
  optionally bounded/redacted output. Low-confidence or secret-bearing commands
  are suppressed; missing shell integration is visibly degraded.
- **Passive Git sensor (ADR-0011):** the built-in Git extension's commit events
  ingest new HEAD metadata and changed paths without polling or `.git` scraping.
- **Privacy defaults:** execution metadata is enabled; terminal output retention
  is opt-in and capped before crossing MCP. Environment variables and terminal
  input are never captured.
- **Visualizer:** a `WebviewViewProvider` renders the live subgraph with
  Cytoscape.js (blue = file, orange = symbol, green = intent, red = execution;
  edge width ∝ effective weight). Refresh / Prune / Export controls.
- **Offline-first:** `cytoscape.min.js` is **vendored** into the extension
  (`media/vendor/`) and loaded from there via `webview.asWebviewUri` — no CDN,
  no network at render time.
- Talks to `mindleak-mcp` as a child process over the same MCP stdio protocol.

---

## 8. Security boundary

MindLeak is a **local, single-user tool** and its threat model is scoped to that:

- **stdio only.** `mindleak-mcp` has no network listener; it speaks JSON-RPC over
  stdin/stdout to a parent process (the agent client or the extension).
- **Ingest tools are unauthenticated by design.** Any process with stdio access
  can write nodes/edges — acceptable locally, **not** safe to expose over a
  network. Exposing the server remotely requires an auth layer and is out of
  scope; the server will not open a socket on its own.
- **Data at rest.** Each clone's graph and Intent Plane live in a user-local,
  non-roaming `repositories/<repository-id>/` directory (ADR-0038). They may
  contain source excerpts, commit messages, command output, goals, and proof;
  treat the directory with the same sensitivity as the workspace.
- **Model boundary.** Consolidation only reaches the configured
  `MINDLEAK_LLM_URL`. Pointed at a local server (the default), nothing leaves
  the machine; a remote URL deliberately sends the bounded request to that
  service. See [SECURITY.md](../SECURITY.md).

---

## 9. Multi-agent attribution

Multiple agents can share one graph safely: WAL SQLite gives multi-process
read/write, and deterministic ids mean concurrent ingests of the same file/run
**reinforce** the same nodes instead of clobbering them. Attribution is layered
on *without* breaking that merge.

- **Register the client session.** `MINDLEAK_AGENT` is a display name only. The
  client calls `open_session` with one opaque 128-bit token and includes that
  token on identity-bearing operations; every ingest and focus then records a
  decay-weighted `agent:session:v1:<fingerprint> --observed--> <node>`
  edge. Unknown tokens and arbitrary caller-selected ids are rejected.
- **Attention decays.** Because an observation is an *edge*, an agent's recent
  focus ranks high and fades over days, exactly like every other signal — the
  graph forgets who looked at what, on the same half-life curve.
- **It merges, never clobbers.** Two agents observing the same file each get
  their own `observed` edge to the one shared node; the node itself is reinforced
  by both. No last-writer-wins.

Query it with the existing surface plus one helper:

- `graph_multi_hop_query("agent:<id>")` — what an agent has touched (its
  decayed observations).
- `graph_multi_hop_query(<file>)` — use general traversal when agent observations
  are relevant. `get_impact_radius` excludes `observed` so shared attention
  cannot manufacture a dependency path.
- `list_agents` — the roster: each `agent` node with its active observation
  count and last-active time.

**Isolation is per clone; learning is shared across its worktrees.** Linked
worktrees resolve one random repository id from shared local Git config and use
the same graph automatically. Independent clones receive different ids. Direct
`MINDLEAK_DB` overrides remain available for deliberate isolation, but sharing
one graph across unrelated repositories is unsupported (path ids collide).
Rationale: [ADR-0003](adr/0003-agent-attribution-as-observed-edges.md) and
[ADR-0038](adr/0038-isolated-worktrees-shared-repository-state.md).
