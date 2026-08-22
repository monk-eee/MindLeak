### Added

- Bridge lists stranded (lease-expired, recoverable) claims:
  `FleetStore::stranded_claims` and `GET /api/v1/repositories/:repository_id/stranded-claims`,
  surfaced as a "Stranded claims" section on the repository detail view with
  a "Recover…" button per row, reusing the existing recovery form (ADR-0111).
  Closes the actual gap the recovery action left open: an operator could
  previously recover a claim only by already knowing its task id.
