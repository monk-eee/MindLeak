### Added

- `ackplane-server`'s `Projector` gains `nodes_missing_embedding` and
  `upsert_embedding` (ADR-0140 decision 2): the same offline-pass shape
  `mindleak_core::embed::nodes_missing_embeddings` already runs locally,
  applied to the ledger-derived `projected_nodes` projection instead of the
  local `nodes` table. `upsert_embedding` never invents a node the ledger
  replay did not produce -- the foreign key on `projected_node_embeddings`
  refuses an embedding for a node absent from the projection, exactly as
  `mindleak-core::embed::ensure_table`'s local table already does. Proven
  against a real Postgres instance: a node stops appearing as missing once
  embedded, missingness is scoped per model (re-embedding under a new model
  is not silently "already done"), re-upserting replaces rather than
  duplicates, and an embedding for an unprojected node is refused
  (`foreign_key_violation`). Population is one optional pass over the
  projection; the ranking pipeline (`kind_prior`/`distinctive_cut`, not a
  bare `pgvector` distance `ORDER BY`) and `ackplane-mcp`'s `recall` tool
  itself remain separate, later work.
