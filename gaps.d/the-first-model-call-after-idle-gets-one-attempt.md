- **The first model call after the model goes idle gets exactly one attempt, so a
  cold load fails outright and silently degrades — MEASURED 2026-07-31, OPEN.**
  — *What happened.* Both planes build a `ureq` agent with a single flat
  `MINDLEAK_HTTP_TIMEOUT_MS` (default 30000ms) applied to **both**
  `timeout_connect` and `timeout_read`, send one request, and on any error return
  `LodestarError::Http` with no retry and no warmup. A cold `glm4:9b` load
  routinely exceeds 30s to first token, so the first `decompose` /
  `draft_question` / consolidation after the model has been idle fails and the
  caller takes its deterministic fallback (single-task decomposition, template
  question). PR #307 (`fix/mcp-request-timeout-outlasts-llm`) gave the *client*
  MCP request timeout headroom so that fallback now reaches the user, but the
  server-side model call itself still gets one shot.
  — *Where.* `chat_json` in [`crates/lodestar-core/src/llm.rs`](../crates/lodestar-core/src/llm.rs)
  (the `ureq::builder()` with `timeout_connect`/`timeout_read` both set to
  `MINDLEAK_HTTP_TIMEOUT_MS`); the same shape in
  [`crates/mindleak-core/src/consolidate.rs`](../crates/mindleak-core/src/consolidate.rs).
  — *Impact.* Medium. The first model-backed action after any idle period runs
  degraded even when the model is healthy and would answer a warm second call.
  Connect and read share one budget, so a slow first token is indistinguishable
  from an unreachable host, and there is no second attempt to tell them apart.
  — *Left for later.* A retry-once (or a cheap warmup/health call before the real
  request), plus splitting the connect budget from the read budget, would let a
  model that is merely cold — not absent — actually get used.
