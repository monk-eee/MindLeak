# ADR-0010: Observability, telemetry, and network resilience

- Status: Accepted
- Date: 2026-07-22
- Deciders: MindLeak maintainers
- Supersedes: none
- Related: [ADR-0002](0002-sqlite-decay-over-vector-llm.md) (local-first),
  [ADR-0008](0008-semantic-recall-embedding-index.md) (optional embedding index)

## Context

MindLeak is written by, and for, AI coding agents. For a human operator, the two
instruments that confirm an agent did what was asked are **tests** and
**observability**. Tests prove behaviour in the abstract; observability proves
what actually happened at runtime — which tools ran, in what order, with what
inputs, how long they took, and whether they failed. Without it, an agent can
report success while silently doing nothing, and the operator has no independent
record to check against.

Before this ADR the system had almost none of that instrument:

- **No structured logging.** Two `eprintln!` startup banners and a handful of
  debug prints in `#[ignore]`d tests. No levels, no spans, no timing, no way to
  turn detail up or down.
- **No metrics.** No call counts, error rates, or latencies for any tool.
- **No durable record.** Nothing survived a run. There was no answer to "what
  did the agent do in this workspace, and did it work?"
- **No network resilience.** Every outbound call (`llm`, `consolidate`, `embed`)
  used `ureq::post(...).send_json(...)` with **no timeout**. A server that
  accepts the socket but stalls hangs the call — and therefore the agent —
  indefinitely. There was no retry policy and no circuit breaker, so a degraded
  optional endpoint could stall every subsequent request.

The hard constraint is the transport: `mindleak-mcp` and `lodestar-mcp` speak
newline-delimited JSON-RPC 2.0 on **stdout**. Any diagnostic written to stdout
corrupts the protocol. Observability must therefore never touch stdout.

## Decision

Add a first-class **observability and resilience layer**. It has four parts, all
local, all off the zero-token deterministic write path's *semantics* (telemetry
is itself deterministic — it makes no LLM calls), and all stdout-safe.

### 1. Structured tracing (real-time, stderr-only)

Adopt `tracing` + `tracing-subscriber`. The MCP binaries initialise a subscriber
that writes to **stderr only**, gated by environment:

| Variable | Default | Meaning |
|---|---|---|
| `MINDLEAK_LOG` | `info` | `tracing`/`RUST_LOG`-style filter (`off`, `warn`, `debug`, `mindleak_core=debug`, …) |
| `MINDLEAK_LOG_FORMAT` | `pretty` | `pretty` for humans, `json` for machine ingestion |

Every MCP tool dispatch is a span carrying the tool name, elapsed time, and
outcome. Network calls emit events on retry, timeout, and circuit-breaker
transitions. `stdout` remains pure JSON-RPC; the subscriber is installed once per
process and is a no-op if `MINDLEAK_LOG=off`.

### 2. Durable audit trail (the record)

A dedicated, append-only table in the existing workspace database records every
tool invocation and autonomous maintenance attempt. It is created idempotently (`CREATE TABLE IF NOT EXISTS`) by the
telemetry module and is **owned by telemetry, not the graph** — it never
participates in decay, traversal, or pruning.

```sql
CREATE TABLE IF NOT EXISTS telemetry_events (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    ts          INTEGER NOT NULL,   -- unix seconds
    kind        TEXT    NOT NULL,   -- 'tool_call' | 'maintenance' | 'llm_call' | 'embed_call' | 'circuit'
    name        TEXT    NOT NULL,   -- tool name / endpoint / breaker key
    outcome     TEXT    NOT NULL,   -- 'ok' | 'error' | 'skipped'
    duration_ms INTEGER,            -- elapsed, when meaningful
    detail      TEXT                -- JSON: counts, error message, breaker state
);
```

Recording is **best-effort and non-fatal**: a telemetry write that fails is
logged at `warn` and swallowed. Instrumentation must never change the result of
the operation it observes.

ADR-0017 autonomous consolidation uses `kind='maintenance'`; no-candidate,
rate-limited, lease-busy, and shutdown passes use `outcome='skipped'`. Details
contain bounded counts/coarse categories, never candidate text, prompts, model
responses, or credentials. Maintenance telemetry is append-only like tool
telemetry and is erased only by explicit memory reset.

### 3. In-process metrics + a query surface

Aggregate counters and latency summaries (count, errors, total/min/max ms) are
derived from `telemetry_events` on demand. A new MCP tool, **`telemetry_snapshot`**,
returns the aggregates plus the most recent N events as JSON — so an agent (or a
human) can interrogate observability the same way they interrogate the graph:
"what did I just do, and did it work?" This closes the verification loop the
operator asked for.

**Lifetime tally vs current health.** The append-only trail means a tool's
`calls`/`errors` are *cumulative history* that never shrinks: one transient
failure keeps `errors >= 1` forever, even after the tool recovers. Presenting
that lifetime tally as the live fault state is misleading — a long-resolved error
looks like an active outage. So each per-tool metric also carries an **ordered
event** signal: `last_success_at`, `last_error_at`, the most recent error's `detail`
(retained as an audit path even after the raw event scrolls out of the bounded
`recent` window), and a derived `currently_failing` (the tool's most recent event
was the error, resolved by append order when timestamps match). The snapshot rolls
these up into `currently_failing_tools`. Lifetime errors answer "has this ever
failed?"; current health answers "is it failing now?" The VS Code Telemetry pane
and the Markdown rendering surface both, and never dress a resolved historical
error as a live fault.

### 4. Network resilience (`net` module)

All outbound HTTP is routed through one module that provides, in order:

1. **Timeouts** — explicit connect and read timeouts on a configured `ureq`
   agent. No call can hang forever.
2. **Bounded retry with exponential backoff** — transient failures (timeouts,
   connection errors, 5xx) are retried up to a small cap; 4xx and decode errors
   are not.
3. **Circuit breaker** — per-endpoint failure tracking. After `threshold`
   consecutive failures the circuit **opens** and fast-fails calls for a cooldown
   window, then allows a single **half-open** probe; success **closes** it. This
   stops a degraded optional endpoint from stalling every request, and each
   transition is traced and recorded.

| Variable | Default | Meaning |
|---|---|---|
| `MINDLEAK_HTTP_TIMEOUT_MS` | `30000` | overall timeout per attempt, bounded 100-300000 ms |
| `MINDLEAK_HTTP_RETRIES` | `2` | extra attempts after the first, bounded 0-5 |
| `MINDLEAK_BREAKER_THRESHOLD` | `5` | consecutive failures before the circuit opens |
| `MINDLEAK_BREAKER_COOLDOWN_MS` | `30000` | how long the circuit stays open before a probe |

The breaker guards **only optional** endpoints (LLM consolidation, embeddings).
The deterministic ingest/query path never calls the network and therefore never
depends on the breaker. When the circuit is open, optional features degrade
exactly as they already do when a server is absent: a clean typed error, never a
hang.

## Consequences

- **Positive.** The operator gets an independent, durable record of agent
  behaviour; detail is tunable from silent to per-call; no outbound call can hang
  the agent; a flapping optional endpoint is isolated automatically; the query
  tool makes runtime behaviour inspectable in-band.
- **Cost.** Two new dependencies (`tracing`, `tracing-subscriber`), one new table
  per workspace database, and one small write per tool call. The write is local,
  best-effort, and off the critical correctness path.
- **Invariants preserved.** stdout stays pure JSON-RPC (tracing is stderr-only);
  telemetry makes no LLM calls (zero-token rule intact); telemetry never mutates
  graph state or fails a tool; the breaker is scoped to optional endpoints so the
  deterministic path is unaffected.
- **Scope.** The reference implementation lands in the MindLeak plane
  (`mindleak-core` + `mindleak-mcp`): tracing, the audit trail, metrics, the
  `telemetry_snapshot` tool, and full resilience for the embedding and
  consolidation calls. The Lodestar plane adopts HTTP timeouts on its LLM client
  now; stderr tracing, the audit trail, and the breaker are a mechanical
  follow-up that reuses this design unchanged.
```
