# Architecture

MindLeak is a **Temporal Context Graph Engine (TCGE)** with two planes: an
**episodic memory graph** (`mindleak-*`) whose edges decay, and a durable
**Intent Plane** (`lodestar-*`, ADR-0004) that does not. Each plane is a Rust
library behind its own MCP stdio server; everything communicates only over the
Model Context Protocol — no shared memory, no sockets beyond stdio.

```
Agent worktrees ─┬─ MCP/stdio ─▶ mindleak-mcp ─▶ mindleak-core ─▶ user state/<repo-id>/graph.db
                 │                                     └── async ──▶ Ollama (optional)
                 └─ MCP/stdio ─▶ lodestar-mcp ─▶ lodestar-core ─▶ user state/<repo-id>/spec.db
```

## Crates

### `mindleak-storage` (library)

Shared platform-independent repository identity, user-local database resolution,
legacy online migration, backup, and integrity verification (ADR-0013/0038).
Both planes resolve one per-clone id from shared Git config, so linked worktrees
share state without sharing files, indexes, or branches. Reset and export remain
plane-specific operations.

### `mindleak-session` (library)

The shared ADR-0030 request identity contract. It validates client-minted
128-bit session ids, derives the same restart-stable opaque agent id in both
planes, keeps only process-local registrations, and recognizes the narrow legacy
owner shapes eligible for audited claim recovery. Raw session tokens are never
persisted or logged.

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
`initialize`, `tools/list`, `tools/call`, `ping`, `shutdown`. Schemas and
handlers live under [`tools/`](../crates/mindleak-mcp/src/tools/mod.rs), grouped
by graph, ingestion, lifecycle, consolidation, embeddings, and telemetry; the
root retains the single timed telemetry wrapper around dispatch.

### `lodestar-core` (library) — the Intent Plane

The **durable** counterpart to the decay graph (ADR-0004): a separate crate and
store so the zero-token decay engine stays uncontaminated. Modules: `model`
(goals/tasks/knowledge), `design` (design items and materialization plans),
`policy` (immutable pack schema/digest validation and the five-clause Common
Core), `controls` (typed enforcement mechanisms and their ceilings, ADR-0034),
`amendment` and `waiver` (changing adopted policy, and bounded exceptions to it,
ADR-0039), `scope` (the one matcher both clauses and waivers use, so the two
cannot disagree about how far a scope reaches), `fleet` (staleness and
divergence derived from declared session context), `stalls` (why work is not
moving — a pure function over the board), `discovery`, `schema.sql` +
`indexes.sql`, `db` (+ a knowledge `effective_weight` scalar), `decay`
(long-horizon revalidation), `store` (`LodestarStore`: the `goals` and goal↔code
seam, `coordination` task/handoff/conformance ledger, transactional
`policy_packs` proposal/disposition/provenance ledger, reviewed `design`
materialization plus validation, `amendments` and `waivers`, learned
`knowledge`, and `lifecycle` operations), `llm` (optional local model), and
`lib` (the `Lodestar` facade wiring). Facade behavior is grouped under
`facade/`: `constitution`, `executive`, `design`, `design_materialization`,
`conformance/`, `controls`, `amendments`, `waivers`, `advice`, `fleet`,
`evidence`, and `knowledge`. `conformance/` is itself split by responsibility:
`mod` (the `complete_task` / `check_conformance` surface), `clauses` (which
constitutional clauses govern a check), `verdict` (bounded evidence to a
verdict), `token` (what a check was issued against), and `evidence` (shape and
window validation of a submitted bundle).
Each design materialization writes an immutable plan revision; task/goal link
tables are the current projection and can be repaired without deleting history.

**Boot order is load-bearing.** `db::configure` applies `schema.sql`, then
migrations, then `indexes.sql`. Indexes live in their own file because an index
over a column a migration adds cannot be created before that migration runs: on
an existing database `CREATE TABLE IF NOT EXISTS` is a no-op, so the whole batch
fails and the migration never runs. Keeping the phases ordered makes that
structural rather than something each migration author must remember.

**Repository topology (ADR-0038).** Every concurrent workstream uses its own Git
worktree and branch. `mindleak.repositoryId` lives in shared local Git config and
selects one platform-local `repositories/<id>/` directory for both SQLite files.
Lodestar coordinates claims and proof; MindLeak shares learned context; only a
protected pull-request merge advances `main`. `storage_status` exposes the exact
id, path, origin, and legacy migration result on each plane.

The learned-knowledge loop is wired end to end (ADR-0022): `knowledge`'s
`promote_signals` bridge feeds MindLeak proven-signal candidates through the
existing count+span consolidation gate (deterministic, model-optional), and
`conformance` consults `active_knowledge` on every check — a changed node that
intersects a proven regularity attaches an **advisory** finding and may nudge an
otherwise-`Aligned` verdict to `NeedsHuman`, but can never emit `Violation` (only
the Constitution hard-fails). The read path stays deterministic — no LLM.

**Evidence is the proof-of-work — the load-bearing guarantee.** Completion is not a
status an agent can assert: `complete_task` consumes only a bounded,
provenance-bearing evidence bundle (`evidence_for`, from the Memory plane) that
`check_conformance` scores against the goal's code bindings, writing a durable,
resolvable record to an append-only `conformance_history` (ADR-0009/0025). In a
multi-agent fleet this chain is the **only** trustworthy proof that the agents did
the sanctioned work — every other signal (an agent's summary, a green check, a PR
body) is narration an agent can fabricate. It is bounded by the live claim,
attributed to the acting agent (ADR-0030), and anchored to real executions and
commits in the graph. `export_evidence` (ADR-0031) renders that chain as a
committed, verifiable artifact so the proof leaves the ledger for human review, a
CI conformance gate (`scripts/conformance-gate.mjs`), and audit — the durable
counterweight to decay: episodes fade, but the record of what conformed survives.

ADR-0026's representation and immutable-pack layers are implemented without a
parallel policy source: `Goal` remains the constitutional clause, while
`constitution_versions` freezes attributed snapshots and `policy_packs` copies
reviewed clauses into local goals with source provenance. Pack rejections remain
durable; conflicts require review; upstream versions cannot mutate local law.
Typed controls, ratchets, amendments, and bounded waivers have since landed on
top of that: a **control** declares the enforcement power a clause actually has
and the ceiling it implies; a **ratchet** binds a metric that must not regress to
a clause and refuses to set its own baseline (ADR-0037); an **amendment** is the
only way to change adopted policy, carrying every active clause forward so the
diff shows what actually changed; and a **waiver** is the reviewable form of
`--no-verify` — scoped, attributed, and always expiring, because an exception
that never ends is the policy (ADR-0039). See
[`SPEC-CONSTITUTION.md`](SPEC-CONSTITUTION.md).

**Knowing why nothing is moving is a first-class read.** `stalls` reports lapsed
leases, work awaiting a human or a peer agent, deadlocked waits, blocks behind
something no agent will advance, and paused work. It is read-only, evidence-free,
and deliberately threshold-free: it reports how long a stall has been true and
leaves "is that too long?" to a person, because a staleness threshold invented in
the engine would become policy nobody agreed to.

[ADR-0024](adr/0024-preflight-overlap-detection.md) adds the coordination layer
above the compare-and-swap claim. Lodestar stores optional claim path globs and
opaque symbol ids in `task_scopes`; its read-only `check_overlap` returns live
scope intersections. MindLeak's same-named query derives other agents' direct or
mutation-linked footprint after decay filtering. The caller combines those two
results by node id: no shared tables, transactions, or plane dependency. The VS
Code allocator performs both reads before claiming, displays an overridable
warning, and renders persisted scope in the Work view. The flow is advisory
(never a lock, per ADR-0015) and complements ADR-0018's physical integration
discipline.

### `lodestar-mcp` (binary)

A second MCP stdio server exposing the Intent Plane tools for constitution,
tasks, conformance, and knowledge. It uses the same newline-delimited JSON-RPC
as `mindleak-mcp`; schemas and handlers are grouped under `tools/` by
constitution, executive, conformance, knowledge, lifecycle, controls,
amendments, waivers, design, design materialization, evidence, and fleet
responsibility. See [`USAGE.md`](USAGE.md) for the workflows those verbs
compose into — the tool tables in [`README.md`](../README.md) describe each verb,
not the order to call them in.

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
The Workspace readiness tree follows the same derived-state rule: pure
`src/readiness.ts` maps MCP initialize identities, `graph_stats`, `board`,
`design_board`, and sensor health to one next action; `readinessController.ts`
performs those reads and `readinessViewProvider.ts` is thin VS Code rendering.
Only the one-time teaching-view dismissal uses workspace state; no graph or
intent authority is copied into the extension.
The Work view's allocation flow collects optional concrete paths/symbol ids,
combines both ADR-0024 overlap reads, and shows scoped work as a planning hint;
warnings remain explicitly overridable. Review-needed rows call the existing
`resolve_task`, `reopen_task`, and `conformance_history` tools in place; the
complete Evidence Board remains an advanced, hidden-by-default audit view
(ADR-0040).

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
