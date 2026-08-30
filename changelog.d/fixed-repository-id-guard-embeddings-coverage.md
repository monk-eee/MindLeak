### Fixed

- `ackplane-bridge/tests/repository_id_guard.rs`'s tenant-scope coverage for
  `Projector` silently dropped `projection/embeddings.rs` after that file was
  added: `PROJECTION_SOURCES` still named only `mod.rs`/`rebuild.rs`/
  `neighborhood.rs`, so `every_projector_query_requires_an_explicit_tenant_id`
  never scanned `nodes_missing_embedding`/`upsert_embedding` at all. Both
  already take `tenant_id: &str` explicitly and pass now that the guard
  actually looks at them.
