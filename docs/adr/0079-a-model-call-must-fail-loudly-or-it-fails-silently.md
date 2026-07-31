# ADR-0079: A model call must fail loudly, or it fails silently

- Status: Accepted
- Date: 2026-07-31
- Deciders: monk-eee
- Related: [ADR-0002](0002-sqlite-decay-over-vector-llm.md) (SQLite/decay over a
  vector-LLM core), [ADR-0004](0004-intent-plane-spec-brain.md) (the optional
  OpenAI-compatible model), [ADR-0017](0017-working-memory-and-autonomous-consolidation.md)
  (consolidation is the only LLM writer), [ADR-0059](0059-the-tool-surface-is-a-vocabulary.md)
  (the tool surface is a vocabulary); the recorded gaps
  `gaps.d/the-first-model-call-after-idle-gets-one-attempt.md` and
  `gaps.d/a-degraded-model-call-is-indistinguishable-from-a-real-one.md`;
  PR #307 (`fix/mcp-request-timeout-outlasts-llm`, the client-timeout half)

## Context

Nothing load-bearing depends on the model. Ingestion is the zero-token
deterministic path, and every model call — `decompose`, `draft_question`,
consolidation — is a fallible `Result` with a deterministic fallback wired
behind it (a single task, a template question, no consolidation). That design is
correct and is not in question here. The model is a garnish, not a dependency.

But the *plumbing around* those calls is not durable, and two recorded gaps say
how. They live in `chat_json`
([`crates/lodestar-core/src/llm.rs`](../../crates/lodestar-core/src/llm.rs)) and
the shared MindLeak request path
([`crates/mindleak-core/src/net.rs`](../../crates/mindleak-core/src/net.rs)):

1. **One attempt, one flat budget.** `MINDLEAK_HTTP_TIMEOUT_MS` (default
   30000ms) is applied to *both* `timeout_connect` and `timeout_read`, one
   request is sent, and any error becomes `LodestarError::Http` with no retry. A
   cold `glm4:9b` load routinely exceeds 30s to first token, so the first call
   after the model has been idle fails and the caller silently falls back — even
   though a warm second call would have answered. PR #307 gave the *client* MCP
   request timeout headroom so the fallback now reaches the user, but the
   server-side call itself still gets exactly one shot, and a slow first token is
   indistinguishable from an unreachable host because they share one budget.

2. **Every failure looks the same.** Nothing listening, connection refused, a
   500, a non-JSON reply, a typo'd `LODESTAR_MODEL` or `base_url` — all collapse
   to one `Http` error and the same fallback, and nothing records which path was
   taken. So a *misconfigured but present* model is indistinguishable from *no
   model*, and a fleet can run fully deterministic for a long time believing it
   is using the LLM. `storage_status` reports each plane's paths but says nothing
   about whether a model answers.

The through-line is the title: a fallback that fires invisibly is worse than a
hard failure, because the failsafe is real but no one can see it engage. The fix
is not to make the model more reliable — it is optional and stays optional — but
to make its **absence, slowness, and misconfiguration observable**, and to give a
merely-cold model the one warm retry that turns a silent degrade into a success.

## Decision

**Make model-call failure legible without ever making the deterministic path
depend on the model.** Four changes, each preserving invariant 4 (the model is
optional and must error cleanly) and the zero-token write path (invariant 1).

1. **Split the connect budget from the read budget.** `timeout_connect` stays
   short (a host either accepts a socket promptly or it is not there);
   `timeout_read` gets the generous budget (a model that accepted the connection
   is thinking, not absent). A refused connection now fails in a second instead
   of blocking for the full read budget, and a slow first token is no longer
   mistaken for an unreachable host. The two budgets are separately configurable;
   `MINDLEAK_HTTP_TIMEOUT_MS` keeps its meaning as the read budget for
   compatibility.

2. **Retry the read exactly once on a cold-start timeout.** A single bounded
   retry — never a loop — turns the common cold-load case (first token past the
   budget, model then warm) into a success rather than a silent fallback. It is
   guarded to the timeout/transient case, not applied to a refused connection (no
   host) or a 4xx (a real rejection), so it never lengthens the "no model" path.
   One retry, because unbounded retry would quietly make callers *wait on* a
   model the whole design says they must never depend on.

3. **Record which path was taken.** A model-backed result carries whether it came
   from the model or the deterministic fallback, and the reason the fallback
   fired (unreachable / timeout / bad-json / misconfigured). The caller no longer
   discards that distinction into a bare `Http` error. This is the load-bearing
  change: it is what makes gap 2 observable rather than silent. Both planes use
  the same serialized vocabulary from `mindleak-model`, so `unreachable`,
  `timeout`, `bad_json`, and `misconfigured` cannot drift apart.

4. **Surface model reachability in `storage_status`.** `storage_status` gains an
   optional, on-demand model-health field: is a model reachable at the configured
   `base_url`, and does it return parseable JSON for a trivial prompt. It is a
   one-shot probe taken when asked, not a background poller (no new process,
   ADR-0004's stdio-only shape holds), and it distinguishes "no model configured"
   from "configured but unreachable" from "reachable but not answering JSON". Per
   ADR-0059 this extends an existing tool's vocabulary rather than adding a verb.

## Consequences

- A cold model is used on the retry instead of silently dropped; a misconfigured
  model is now visibly different from an absent one, in `storage_status` and on
  every degraded result. Silent degradation stops being silent.
- New configuration: a connect budget distinct from the read budget. Existing
  `MINDLEAK_HTTP_TIMEOUT_MS` continues to mean the read budget, so nothing breaks
  for callers who set only it.
- `storage_status` output grows a field, and `draft_question`/`decompose`/
  consolidation results grow a provenance marker — both are additive; readers
  that ignore them are unaffected. `docs/TOOLS.md` gains the `storage_status`
  field description.
- The retry adds at most one extra read-budget of latency to a genuinely cold
  first call, and only on timeout — the path that already had nothing but a
  fallback to show for its wait. Connection refusal gets *faster*, not slower.
- The prerequisite `refactor/the-model-splits-by-concern` landed in PR #176
  before this decision was accepted and materialized, avoiding a same-layer
  collision. Implementing this decision closes both recorded gap fragments.

## Rejected alternatives

**Unbounded retry / a retry budget.** Simple to write and wrong in spirit: it
makes a caller *wait on* an optional model, and a wedged model would stall every
decompose and dialogue draft behind it. One retry addresses cold start; more
turns a garnish into a dependency.

**A background health poller.** A daemon that pings the model on a timer would
keep `storage_status` instant, but it adds a process and a network cadence to a
stdio-only, no-listener design (ADR-0004). An on-demand probe answers the same
question when it is actually asked, at the cost only of that call's latency.

**Make the model call synchronous and fail the operation on model error.** The
opposite of the whole design: it would make `decompose`, dialogue drafting, and
consolidation *require* a reachable model, breaking invariant 4 and the zero-
token path. The fallback is the floor and stays the floor; this ADR only makes
the floor visible when you land on it.

**Leave it as fallback-only (status quo).** What the two gap fragments describe.
The failsafe works, but its silence is the defect: a broken model configuration
produces no signal, so no one fixes it, and the fleet runs degraded indefinitely
believing otherwise.
