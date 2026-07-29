- **Memory tools now tell the model when to use them, without adding another
  tool or growing their advertised surface.** Telemetry measured the adoption
  failure on 2026-07-29: `ingest_execution`, `ingest_file`, and `ingest_commit`
  had run 10,122 times, while `recall`, `working_set`, `get_impact_radius`, and
  `graph_multi_hop_query` had run only 70 times between them. Their
  `tools/list` descriptions previously defined mechanisms but supplied no cue,
  so writing became habitual and reading did not. The four existing tools now
  name the moments already present in an agent's work: resume or task switch,
  before the first edit, questions about why/prior decisions/regressions, and
  deterministic task-text traversal when semantic recall is unavailable. A
  contract test reads the actual advertised definitions and preserves those
  cues while holding their combined compact JSON at or below the measured
  2,072-byte baseline.
