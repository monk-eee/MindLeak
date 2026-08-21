### Added

- Bridge exposes a Readiness rollup view (ADR-0105 decision 6's
  Workspace/Readiness row): `GET /api/v1/readiness` returns one page of
  per-repository health (`ReadinessStore::readiness`), composed entirely
  from Fleet/Claims/Signing-key state already exposed elsewhere — active
  node count, projection freshness, active claim count and soonest lease
  expiry, and signing-key health counts — rather than a new domain. Each
  repository gets a derived `ready`/`attention_needed`/`not_ready` status:
  `not_ready` when it has never produced a projection; `attention_needed`
  when the projection is lagging or any signing key is expired, revoked,
  unknown, or bound to a mismatched identity; `ready` otherwise. The Fleet
  page gained a "Readiness" section with a status badge per repository, so
  an operator sees which repositories need attention at a glance instead of
  opening each one's detail panel in turn.
