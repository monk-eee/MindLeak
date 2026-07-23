# ADR-0017: Working-memory tier and autonomous consolidation cycle

- Status: Accepted (phased)
- Date: 2026-07-23
- Deciders: MindLeak maintainers
- Related: [ADR-0002](0002-sqlite-decay-over-vector-llm.md) (decay),
  [ADR-0003](0003-agent-attribution-as-observed-edges.md) (attention/`observed`),
  [ADR-0005](0005-signal-weighted-decay.md) /
  [ADR-0012](0012-derived-signal-evidence.md) (signal-weighted decay, expiring
  signal), [ADR-0010](0010-observability-and-resilience.md) (telemetry)

## Context

MindLeak is already a faithful computational analog of the human
**complementary-learning-systems** picture: an exponential **forgetting curve**
(decay), **rehearsal** (reinforcement on re-ingest), **salience tagging**
(signal-weighted decay), distinct **episodic** (fast) and **semantic/structural**
(slow) representations, and **systems consolidation** (the optional "sleep-phase"
worker that compresses episodic nodes into `intent` gist).

Two pieces of that model are missing, and they are the two halves of the same
mechanism:

1. **No bounded working-memory tier.** Human short-term memory is a small,
   capacity-limited (~7±2) attentional bottleneck. MindLeak has no bounded active
   set — "short-term" is only "high recent weight" over the whole graph. There is
   no *"what am I actively working on right now"* focus that is deliberately
   small and high-signal.
2. **No autonomous consolidation cycle.** Consolidation exists through the
  on-demand `consolidate_session` and `consolidate_signal` tools. In the human
  system, consolidation is automatic and runs during downtime — it is not
  something the agent chooses to invoke. Today, without an explicit call,
  episodic detail simply decays and is pruned, and its *meaning* is lost with it.

Completing these turns MindLeak from "has decaying + long-term memory" into "has a
working-memory bottleneck feeding an automatic consolidation loop" — the part of
the human system not yet modeled.

## Decision

Add both, as one paired mechanism. They compose: the working set is the *active*
material; the consolidation cycle, during idle, rescues the *aging-but-
consequential* material into durable gist before it is pruned.

### Part A — Working-memory tier (bounded, derived)

- **The working set** is the small, capacity-bounded set of nodes with the
  highest *attention-weighted recency* for the active agent — derived from
  `observed`/`boost` recency and effective weight, ranked and **hard-capped at a
  small `K`** (default ≈ 7, `MINDLEAK_WORKING_SET_SIZE`). The cap *is* the point:
  it reproduces the attentional bottleneck instead of returning a whole
  neighborhood. The startup value defaults to 7 and is bounded to 1..32.
- **Derived, not stored.** It is a query-time view over existing `observed` edges
  + decay, not a new buffer to maintain (respecting the derived-not-stored
  invariant). A read tool (`working_set`, or an argument on existing queries)
  returns it; queries may be **scoped** to the working set for tight, low-token,
  high-signal context without the agent supplying a seed. `working_set(limit?)`
  requires `MINDLEAK_AGENT`; an optional lower limit may reduce but never exceed
  the configured hard cap. Results expose attention score, observation count,
  observation span, and last observation time.
- It also provides the signal for Part B: nodes that stay in the working set
  across a span are "rehearsed" and are prime consolidation candidates.

### Part B — Autonomous consolidation cycle (optional, gated, idle-driven)

- An **optional background pass** runs consolidation *without* an explicit tool
  call, on a low-priority worker thread with **its own SQLite connection**
  (WAL + `busy_timeout` already make a second connection safe).
- **Off by default.** The worker exists only when
  `MINDLEAK_AUTONOMOUS_CONSOLIDATION=true`; merely configuring a model never
  spends tokens. This resolves the default-posture question in favor of explicit
  operator consent.
- **Trigger:** idle only in the first release — after the server sees no requests
  for `MINDLEAK_CONSOLIDATE_IDLE_SECS` (default 300, bounded 30..86400), it runs
  a bounded pass ("consolidate during sleep"). A pressure trigger is deferred
  until measured graph growth demonstrates a need.
- **What it does:** target the nodes **about to decay below the prune threshold**
  (reuse the Phase-5 / ADR-0012 *expiring signal candidates*), compress each
  aging-but-consequential episodic cluster into a durable `intent`/gist node, then
  let prune drop the spent detail — **save the meaning before forgetting the
  details**, exactly like systems consolidation.
- **Bounded and observable:** rate-limited
  (`MINDLEAK_CONSOLIDATE_MIN_INTERVAL_SECS`, default 3600, bounded 60..86400),
  capped work per pass (`MINDLEAK_CONSOLIDATE_MAX_NODES`, default 20, bounded
  1..200), transactional after the model response (a failed persistence pass
  rolls back and raw evidence remains), and every pass emits telemetry (ADR-0010)
  so the operator can see what "sleep" did.
- **Gated on a configured model.** No reachable model → no autonomous pass; the
  system degrades to today's on-demand behavior with zero background token spend.

### How they close the loop

```text
ingest (zero-token) -> bounded working set (Part A, ~7 items)
                          |  rehearsed items persist, unrehearsed decay
                    (idle) v
        autonomous consolidation (Part B): rescue expiring signal -> gist
                          |
                 durable MindLeak intent nodes (long-term)
                          |
                    prune the spent detail
```

## Consequences

- **Positive.** Completes the memory metaphor: a real attentional bottleneck plus
  an automatic sleep-phase. Agents get a tight, high-signal active context for
  free; consequential meaning is preserved instead of decaying into nothing.
- **Cost.** Part B introduces a **background thread** into a previously purely
  synchronous stdio server — new lifecycle (spawn on start, clean shutdown on
  exit) and a second DB connection. This is the real architectural weight of this
  ADR.
- **Risk.**
  - *Runaway token spend* from autonomous LLM calls — mitigated by model-gating,
    rate limits, a per-pass node cap, and idle-only default. Consider shipping it
    **off by default** and opt-in.
  - *Background/foreground write contention* — already covered by the WAL +
    `busy_timeout` + bounded-retry contract; keep consolidation transactions
    short.
  - *Working-set thrash* if `K` is too small or ranking is noisy — start
    conservative, make `K` configurable.
- **Invariants preserved.** The **zero-token write path is untouched** —
  ingestion stays deterministic; consolidation remains the async layer, only now
  it can be *scheduled* rather than *summoned*. Working set is derived; decay
  stays the point; stdout stays pure JSON-RPC (the worker logs to stderr).

## Delivery phases

1. Ship the derived `working_set` tool and rehearsal-aware attention evidence.
2. Refactor `consolidate_signal` persistence into one transaction, then add the
   opt-in idle worker with a second connection, clean shutdown, and telemetry.

Part B must call the same model client used by `consolidate_session`, the same
`consolidate_signal` persistence path used manually, and the same
`expiring_signal_candidates` query as Phase 5. This ADR adds scheduling, not a
second consolidation implementation.

Each phase is independently right-shaped; phase 1 adds no model or thread and
does not pretend autonomous consolidation exists before phase 2 lands. A stored
focus buffer, pressure trigger, and direct Lodestar write are excluded until
measured use demonstrates a need.
