### Added

- Bridge now has a tenant-scoped `/api/v1/live` SSE sub-router with durable
  cursor replay, typed resynchronization on replay gaps, and bounded
  operational event metadata for future visible operations views.
