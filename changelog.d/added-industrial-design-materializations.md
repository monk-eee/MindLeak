### Added

- `MaterializationStore` (ADR-0121 decision 4): an append-only,
  idempotency-key-scoped revision history of materialization decisions
  against an Industrial design, in a new `industrial_design_materializations`
  table plus a `industrial_design_materialization_work_tasks` junction table
  for FK-checked Work-task references. `record_materialization` mirrors
  `evidence_store`'s established idempotency contract: an identical
  resubmission (same `idempotency_key`, same content) is a no-op returning
  the original revision; the same key resubmitted with different content is
  refused as a conflict. Not yet exposed over any RPC/HTTP route, and does
  not yet implement a Bridge Design Board (decision 7) -- that remains
  separate follow-on work.
