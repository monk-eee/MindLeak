- **PR effectiveness telemetry is now reproducible instead of a one-off
  analysis.** `node scripts/evaluate-pr-effectiveness.mjs --limit=50` joins a
  bounded GitHub cohort to Lodestar tasks through branch, durable thread, and
  evidence-commit provenance, then reports conformance coverage/causes, claim
  timing, human resolution, reconciliation churn, required-check completeness,
  runtime latency/errors, polling share, and memory-read-before-write adoption.
  Timestamped JSON and Markdown land under `target/telemetry`; deterministic
  controls keep missing checks and incomplete attribution visible. Reports
  contain no prompts, secrets, source, model reasoning, raw task threads, or raw
  conformance evidence.
