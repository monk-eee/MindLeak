- **Ackplane retains immutable history for every published Constitution
  version (ADR-0121 decision 1).** A new `constitution_publications` table
  and `ConstitutionStore::record_publication`/`get_publication`/
  `list_publications` sit beside the existing mutable active-snapshot
  projection -- a byte-identical replay of the same `(tenant_id,
  repository_id, version_id)` is idempotent, and different content under the
  same identity is refused, never silently overwritten. The existing
  `publish`/`get_active` behavior is unchanged. This is a store-only slice;
  the compiler/authenticated request service and Bridge Design Board reads
  are follow-on work.
