### Added

- Bridge exposes `recover` as a tenant-scoped administrative claim mutation
  (ADR-0111): `POST /api/v1/repositories/:repository_id/tasks/:task_id/recover`
  calls `ClaimStore::recover` directly, requiring a non-empty `reason` and
  resolving the claim's current owner itself (via the new
  `FleetStore::claim_owner`) rather than trusting a caller-supplied value.
  `delegate`, `release`, and `renew` remain node-signed-only and are not
  exposed. The Fleet UI gains a recovery form per repository, since an
  already-expired claim no longer appears in the existing active-work list.
