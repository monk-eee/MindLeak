- **Execution evidence ingestion can fail with a foreign-key error.** Observed
  on 2026-09-11 while `ingest_execution` recorded a successful supervisor clippy
  run through the native MindLeak MCP server: the call returned
  `sqlite error: FOREIGN KEY constraint failed`. The owning path is
  `crates/mindleak-core/src/facade/ingestion/mod.rs::ingest_execution_for_agent`,
  which writes the execution and then its agent observation. Earlier source and
  execution writes in the same session succeeded; retrying this write succeeded
  as `execution:62366ce9f9df`. Impact: a passing validation can lack recorded
  provenance unless the caller checks the tool result and retries. The root
  cause is unconfirmed; investigate transaction boundaries and concurrent
  maintenance without assuming a race from this one observation. Left for later;
  only the missing evidence write was recovered in this run.
