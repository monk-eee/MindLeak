# Specification — Temporal Context Graph Engine (TCGE)

**Codename:** MindLeak
**Role:** A local, event-driven **context graph** for coding agents. Ingests raw
telemetry (executions, commits, file symbols) deterministically — **zero LLM
tokens on the write path** — converts it into a decay-weighted directional graph,
and exposes graph-traversal tools to LLM agents over MCP. An optional local model
(Ollama / GLM) consolidates noisy logs into high-level intent nodes asynchronously.

This is a **complete replacement** for flat log / vector-only memory: instead of
storing sequential events forever, MindLeak maps explicit **nodes** and **edges**
whose weights **decay on an exponential half-life** so stale context fades out.

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
| Working memory / write-gate | "What is worth keeping?" | **Decay + prune** |

Vector search matches keywords with extra steps; it can't do multi-hop
structural reasoning ("what breaks if I change this schema?"). Flat logs give
chronology but not topology. MindLeak gives both, and forgets on purpose.

---

## 2. System topology

```
┌────────────────────────────────────────────┐
│              VS Code extension              │
│  passive sensor (focus/save) + Cytoscape UI │
└──────────────────────┬──────────────────────┘
                       │ MCP over stdio (child process)
                       ▼
┌────────────────────────────────────────────┐
│                MindLeak core engine          │
│  deterministic ingest ─▶ SQLite graph + FTS │
│  decay engine (half-life) ─▶ prune          │
│        │ (optional async queue)             │
└────────┼────────────────────────────────────┘
         ▼
┌────────────────────────────────────────────┐
│      Local Ollama consolidation worker      │
│  glm4:9b / codegeex4:9b — log → intent node │
└────────────────────────────────────────────┘
                       ▲ MCP tools
┌────────────────────────────────────────────┐
│  Agents: Copilot / Claude / Cursor / CLI    │
└────────────────────────────────────────────┘
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
| `package` † | `package:<name>` | external dependency (non-workspace) |

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
| Artifact → Artifact/Package † | `imports` | `use` / `import` / `require` / `from` statements |
| Artifact → Package † | `depends_on` | manifest deps (`Cargo.toml`, `package.json`, …) |
| Symbol → Symbol † | `extends` | class / trait inheritance |
| Symbol → Symbol † | `implements` | interface / trait conformance |

> **`calls` scope.** Resolution is **in-file and heuristic**: a function/method
> body is bracket- or indentation-scoped, and callee names are matched against
> the symbols defined in the same file. **Cross-file** resolution arrives with
> `imports` (ADR-0006) via the per-file import table; precise, scope-accurate
> resolution remains the Tree-sitter upgrade (see
> [ADR-0002](adr/0002-sqlite-decay-over-vector-llm.md)).

> **† Structural enrichment — [ADR-0006](adr/0006-structural-dependency-edges.md)
> (approved, in build).** `imports`, cross-file `calls`, `extends`, `implements`,
> `depends_on`, and the `package` node make impact analysis **cross-file** and
> give [ADR-0005](adr/0005-signal-weighted-decay.md) the structural substrate
> (centrality, corroboration) it needs. Delivered in phases: imports + package +
> cross-file calls → extends/implements → manifest depends_on. `references`,
> `consumes`, and `produces` are deferred (determinism/noise cost).

Re-ingesting an existing edge **reinforces** it (weight `+0.05`, capped at 1.0)
and **resets its decay clock**.

---

## 4. Storage & decay

SQLite (bundled, single file) with **FTS5** for text seed lookup and a registered
`effective_weight()` scalar function so decay math runs in SQL.

```
W_effective = W_base · 2^(−Δt_hours / half_life_hours)
```

* `Δt` — hours since the edge was last reinforced (`updated_at`).
* `half_life` — 24h for raw execution evidence, 168h for durable structure
  (`contains`, `calls`, `imports`, `extends`, `implements`, `depends_on`) and
  human intent, 48h default (`relates_to`, `observed`).
* **Prune rule:** edges with `W_effective < 0.05` are ignored at query time and
  purged during maintenance; orphaned `execution` nodes are dropped.

Schema: [`crates/mindleak-core/src/schema.sql`](../crates/mindleak-core/src/schema.sql).

> **Signal-weighted decay (ADR-0005).** Pure time-decay treats signal and noise
> alike; frequency and recency are weak proxies. *Signal-weighted decay*
> ([ADR-0005](adr/0005-signal-weighted-decay.md)) adds an evidence term so proven
> signal resists decay and noise fades — "decay noise, not signal". **Shipped:**
> reinforcement-graduated half-life (`signal_half_life()` over an edge's
> `reinforcement_count`/`first_seen`). Richer proxies (corroboration, surprise)
> and consolidating proven clusters into durable learned-knowledge are next.

---

## 5. MCP tool surface

**Agent-facing (spec core):**

1. `graph_multi_hop_query(seed_entity, max_depth=2, min_weight=0.2)` — traverse N
   hops from a node id or FTS phrase; returns nodes, decayed edges, and scores.
2. `get_impact_radius(target_artifact)` — bidirectional depth-2 blast radius:
   dependents, prior failing executions, related intents.
3. `record_architectural_decision(decision_text, related_nodes[])` — write an
   intent node linked to affected nodes.

**Ingestion & maintenance (also over MCP):** `ingest_execution`, `ingest_commit`,
`ingest_file`, `boost_entity`, `graph_snapshot`, `prune_graph`, `graph_stats`,
`consolidate_session`, `list_agents`.

---

## 6. Optional LLM augmentation (local, async)

The deterministic pipeline owns the write path. An optional consolidation worker
calls a local model over the **OpenAI-compatible** `/v1/chat/completions` API
(so it works with Ollama's OpenAI endpoint, LM Studio, llama.cpp's server, or any
compatible host — not just Ollama) with a strict JSON `response_format` to:

- **Sleep-phase consolidation** — compress N raw execution nodes into one
  high-level `intent` node, then prune the noise.
- **Semantic edge synthesis** — extract rationale ("why") from commit messages
  and `// DECISION:` / `// HACK:` comments.

Configuration (all optional; sensible local defaults):

| Variable | Default | Meaning |
|---|---|---|
| `MINDLEAK_LLM_URL` | `http://localhost:11434/v1` | OpenAI-compatible base URL (Ollama's `/v1`) |
| `MINDLEAK_MODEL` | `glm4:9b` | model name |
| `MINDLEAK_LLM_API_KEY` | *(empty)* | bearer token for hosted servers; Ollama ignores it |

Nothing leaves the machine when pointed at a local server; the model is optional
and never on the hot path. Exposed as the `consolidate_session` MCP tool, which
errors cleanly when no model is reachable.

---

## 7. VS Code extension

- **Passive sensor:** `onDidChangeActiveTextEditor` boosts the focused file's
  node; `onDidSaveTextDocument` ingests its symbols.
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
- **Data at rest.** The graph (`.mindleak/graph.db`) may contain source
  excerpts, commit messages, and command output. It is gitignored and
  regenerable; treat it with the same sensitivity as the workspace.
- **LLM boundary.** Consolidation only reaches the configured
  `MINDLEAK_LLM_URL`. Pointed at a local server (the default), nothing leaves
  the machine. See [SECURITY.md](../SECURITY.md).

---

## 9. Multi-agent attribution

Multiple agents can share one graph safely: WAL SQLite gives multi-process
read/write, and deterministic ids mean concurrent ingests of the same file/run
**reinforce** the same nodes instead of clobbering them. Attribution is layered
on *without* breaking that merge.

- **Set `MINDLEAK_AGENT=<id>`** when launching the server (per agent / client).
  When set, every ingest and focus also records a decay-weighted
  `agent:<id> --observed--> <node>` edge. Unset ⇒ no attribution (byte-identical
  to before — no `agent` nodes, no `observed` edges).
- **Attention decays.** Because an observation is an *edge*, an agent's recent
  focus ranks high and fades over days, exactly like every other signal — the
  graph forgets who looked at what, on the same half-life curve.
- **It merges, never clobbers.** Two agents observing the same file each get
  their own `observed` edge to the one shared node; the node itself is reinforced
  by both. No last-writer-wins.

Query it with the existing surface plus one helper:

- `graph_multi_hop_query("agent:<id>")` — what an agent has touched (its
  decayed observations).
- `get_impact_radius(<file>)` — bidirectional, so it surfaces the agents who
  observed a file alongside its code dependents.
- `list_agents` — the roster: each `agent` node with its active observation
  count and last-active time.

**Isolation is per-database.** Point each agent / worktree at its own
`MINDLEAK_DB` for separate brains, or a shared absolute path for a merged one.
Ids are workspace-relative, so worktrees of the *same* repo merge cleanly;
sharing one DB across *different* repos is not supported (path ids collide).
Rationale: [ADR-0003](adr/0003-agent-attribution-as-observed-edges.md).
