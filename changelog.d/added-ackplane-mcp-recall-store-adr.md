### Added

- Added proposed ADR-0140, resolving ADR-0136's last open follow-up (all
  three named gaps are now resolved): a new `projected_node_embeddings`
  `pgvector` table, scoped to the ledger-derived `projected_nodes` projection
  rather than the curated, human-governed `knowledge`/`knowledge_embeddings`
  domain (ADR-0113). Critically, `ackplane-mcp`'s `recall` ranks candidates
  through the same discrimination pipeline `mindleak-core::embed::recall`
  already implements locally (kind-prior weighting, a per-query
  distinctive-field statistical cut, and the answer-nothing floor) rather
  than a bare `pgvector` distance `ORDER BY`, which measured evidence shows
  regresses below today's SQLite-backed behavior (a nonsense query scores
  above a naive floor because embedding spaces are anisotropic).
