- **Fixed:** `ackplane-server`'s nine near-identical private `rfc3339` helpers
  (one per gRPC service module -- `claim_service.rs`, `constitution_service.rs`,
  `evidence_service.rs`, `enrollment_service/wire.rs`, `knowledge_service.rs`,
  `telemetry_service.rs`, `work_query_service.rs`, and two in
  `work_command_store`) now share one conversion in a new `wire_format`
  module. Each module keeps its own thin wrapper mapping the formatting
  error into its own error type and message (a `String`, a `tonic::Status`,
  or a domain error enum) -- that part is legitimate per-module adaptation,
  not duplication; the actual `OffsetDateTime::from(...).format(&Rfc3339)`
  conversion, identical across all nine, now exists in exactly one place.
