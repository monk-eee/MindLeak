### Added

- `DesignStore` (ADR-0121 decision 3): a separate Industrial-only authority
  for design/materialization decisions, distinct from the read-only
  Constitution projection. An opaque `(tenant_id, repository_id, design_id)`
  design record carries bounded title/summary/source_version, a
  closed-vocabulary lifecycle state, and optional references into the
  Constitution/Work/Evidence domains, each checked by a real foreign key.
  Creation is a digest-checked idempotent insert that also records the
  design's first (`Proposed`) row in a new append-only
  `industrial_design_decisions` history table; later transitions append
  further history rows via `record_decision`. Not yet exposed over any
  RPC/HTTP route, and does not yet implement materialization plans/revisions
  (decision 4) or a Bridge Design Board (decision 7) -- those remain
  separate follow-on work.
