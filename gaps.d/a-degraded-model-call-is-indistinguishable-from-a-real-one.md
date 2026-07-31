- **A degraded (fallback) model result is indistinguishable from a real one, and a
  misconfigured model from an absent one — MEASURED 2026-07-31, OPEN.**
  — *What happened.* Every model failure mode — nothing listening, connection
  refused, HTTP 500, a malformed or non-JSON response, a wrong `LODESTAR_MODEL`
  or `base_url` — collapses to a single `LodestarError::Http`, and every caller
  falls back the same way (`decompose` → one task, `draft_question` →
  `dialogue::template_question`). Nothing records or returns which path was taken,
  so neither the agent nor the operator can tell whether the model answered or the
  fallback fired. A model that is present but misconfigured (typo'd model name,
  wrong `/v1` URL) looks identical to no model at all, so a fleet can run fully
  deterministic for a long time believing it is using the LLM.
  — *Where.* `chat_json` and its callers `draft_question`/`decompose` in
  [`crates/lodestar-core/src/llm.rs`](../crates/lodestar-core/src/llm.rs); the
  design-promotion/decompose facade; and
  [`crates/mindleak-core/src/consolidate.rs`](../crates/mindleak-core/src/consolidate.rs).
  `storage_status` reports each plane's resolved paths but says nothing about
  whether a model is reachable.
  — *Impact.* Medium. Silent degradation is worse than a hard failure here: the
  failsafe is real but invisible, so a broken model configuration produces no
  signal and no one investigates. This is the same "names one plane, hides the
  other" shape as `the-push-gate-names-one-plane-of-two.md`, but for model health
  rather than the Memory Plane.
  — *Left for later.* A one-shot "is a model reachable and returning JSON?" probe
  surfaced in `storage_status` (or a startup snapshot), plus a
  fallback-fired marker on results that took the deterministic path, would make
  the degraded mode observable instead of silent.
