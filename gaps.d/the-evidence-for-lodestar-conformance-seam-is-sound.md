- **The `evidence_for` → Lodestar conformance seam is sound, but convention-
  sensitive.** — The producer and consumer agree on schema version 1, normalized
  `agent:<id>` observation provenance, successful-execution subset rules, and
  inclusive claim bounds. Executions source `modified` / `failed_on`; commit
  intent nodes source `refactored`, so every changed or failed node names a
  source accepted by `validate_evidence_shape`. This is not a product bug. The
  otherwise-unenforced ingestion convention is pinned by
  `evidence_for_emits_self_consistent_provenance`, which exercises execution,
  failure, and commit evidence and fails if a future ingester emits an unusable
  bundle; `evidence_for_normalizes_prefixed_agent_and_includes_window_boundaries`
  pins agent normalization and inclusive endpoints. — Verified Jul 2026 on
  `task:40c4e757e601`.
