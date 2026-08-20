### Added

- Bridge repository detail now shows recorded knowledge for that repository:
  content, effective weight, source reference, and when it was confirmed,
  recency-ordered (ADR-0106). The new tenant-scoped
  `GET /api/v1/repositories/:repository_id/knowledge` read returns `404` for
  an unenrolled or cross-tenant repository, and reuses the already-shipped,
  already-tested `KnowledgeStore::recall` rather than a second query invented
  for the Bridge view -- closing the "Knowledge" row of ADR-0105's parity
  table, which previously had no Bridge implementation at all.
