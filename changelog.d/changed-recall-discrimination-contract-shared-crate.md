### Changed

- `NodeType` and the recall discrimination contract (`kind_prior`,
  `distinctive_cut`, `DISTINCTIVE_MIN_FIELD`, `DISTINCTIVE_SIGMA`) moved from
  `mindleak-core` into the existing shared `mindleak-model` crate (ADR-0140
  decision 5, extending ADR-0136 decision 6's precedent). `mindleak-core::model`
  re-exports `NodeType` so existing call sites are unaffected, and
  `mindleak-core::embed` now consumes the discrimination functions from
  `mindleak-model::discrimination` with no behavior change (proved by its
  existing, unmodified test suite). This answers ADR-0140 slice 1: a future
  Ackplane recall pipeline can depend on `mindleak-model` for identical
  ranking logic without creating a dependency edge onto `mindleak-core` or
  `lodestar-core` (ADR-0082).
