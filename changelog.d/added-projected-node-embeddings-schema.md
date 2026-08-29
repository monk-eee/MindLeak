### Added

- Added `projected_node_embeddings`, a `pgvector`-backed embeddings table
  scoped to the ledger-derived `projected_nodes` projection (ADR-0140 decision
  1) — schema only, applied idempotently via `Projector::connect` alongside
  `0002_projection.sql`. Deliberately distinct from the curated,
  human-governed `knowledge`/`knowledge_embeddings` domain (ADR-0113): an
  embedding here cannot outlive or precede its projected node (`FOREIGN KEY
  ... ON DELETE CASCADE`), matching the same load-bearing property
  `mindleak-core::embed`'s local, SQLite-backed table already documents.
  Population (an optional second pass over `projected_nodes`, never a second
  writer) and the ranking pipeline (`kind_prior`/`distinctive_cut`, not a bare
  `pgvector` distance `ORDER BY`) are separate, larger slices — this is
  schema only, proven against a real Postgres instance.
