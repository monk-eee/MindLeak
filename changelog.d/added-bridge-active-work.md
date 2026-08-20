### Added

- Bridge repository detail now shows active Ackplane-delegated work from the
  PostgreSQL claim authority: task, agent owner, branch, lease expiry, and
  declared path/symbol scope. The new tenant-scoped
  `GET /api/v1/repositories/:repository_id/claims` read returns `404` for an
  unenrolled or cross-tenant repository, excludes expired claims, and remains
  bounded to 50 current items.
