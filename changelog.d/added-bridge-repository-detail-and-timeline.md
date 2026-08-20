### Added

- The Ackplane Bridge exposes two new tenant-scoped read endpoints (ADR-0095
  decision 4): `GET /api/v1/repositories/:repository_id` for one repository's
  coordination, ledger, and projection state (including a `freshness`
  classification of `never_projected`, `lagging`, or `fresh`), and
  `GET /api/v1/repositories/:repository_id/timeline` for its most recent
  accepted ledger records, newest first. A repository outside the caller's
  tenant, or never enrolled at all, reads as `404` either way — never as a
  cross-tenant peek. The timeline is capped at a fixed 50 events per request;
  ADR-0095 does not yet define a paging contract, so an unbounded limit would
  let a request pull an entire repository's ledger history through the
  Bridge.
