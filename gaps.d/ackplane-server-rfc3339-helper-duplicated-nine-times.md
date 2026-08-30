- **`ackplane-server` now has nine near-identical private `rfc3339` helpers,
  one per gRPC service module — MEASURED 2026-08-30, OPEN.**
  `claim_service.rs`, `constitution_service.rs`, `evidence_service.rs`,
  `knowledge_service.rs`, `telemetry_service.rs`, `enrollment_service/wire.rs`,
  `work_command_store/execute/supervisor_directives.rs`, and (as of this
  change) `work_query_service.rs` each define their own copy of
  `fn rfc3339(timestamp: SystemTime) -> Result<String, String>` (or an
  `Err`-type variant), all doing the identical
  `time::OffsetDateTime::from(timestamp).format(&Rfc3339)` conversion. This
  run added the ninth copy rather than extracting a shared one, to keep the
  ADR-0139 clause 2 diff scoped to `task_query` and consistent with every
  sibling service module's existing self-contained-module convention, not
  because the duplication is fine.
  **What is actually needed:** extract one `pub(crate) fn rfc3339` (module
  path TBD, e.g. a small `wire_format.rs`) that every service module calls,
  removing the eight-times-repeated definition. A pass across all nine call
  sites, not a single-module fix — left for a dedicated small refactor
  commit so it does not get bundled invisibly into an unrelated feature.
