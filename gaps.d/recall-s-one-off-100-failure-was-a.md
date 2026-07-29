- **`recall`'s one-off "100% failure" was a missing embedding model, not a bug.** —
  Telemetry showed `recall` as the only tool with an error (1 call / 1 error, 3ms
  fast-fail); the recorded detail was `/v1/embeddings status 404`. Root cause: the
  embedding model (`MINDLEAK_EMBED_MODEL`, default `nomic-embed-text`) was not yet
  pulled into Ollama, so the query-embedding POST 404'd — an environment/config
  issue (ADR-0008: recall is optional and off the deterministic hot path).
  Verified it degrades cleanly (typed `MindLeakError::Http`, no panic/block, no
  hot-path poisoning — 2362 events, only `recall` ever errored) and, once
  `ollama pull nomic-embed-text` was run, recall returns scored results (94ms,
  confirmed live). — Low impact (optional feature, self-announcing at startup and
  documented in QUICKSTART/USAGE). — Resolved: operator remediation already
  documented; contract covered by
  `recall_and_index_degrade_cleanly_when_the_embedder_is_unreachable` (unreachable
  model → error) and `recall_returns_empty_not_error_when_the_index_is_unpopulated`
  (reachable model, empty index → empty, not error); observed on
  `task:2c86cc1f51ea`.
