- **Shared Industrial recall:** `ackplane-mcp` now offers `recall` through the
  enrolled companion. It reuses server-side pgvector candidate retrieval and
  kind-prior/distinctive-field ranking, reporting raw similarity alongside
  explicit empty, unprojected, stale, unembedded, partial or current state.
  Hits and metadata share one read-only database snapshot; a concurrent rebuild
  cannot mix them. Status probes avoid model calls for unsearchable data, and
  model or RPC failures remain errors rather than no-match results. Query
  inference stays on the node, response sizes are bounded, and local recall is
  unchanged.
