- **Ackplane treated `lease_expires_at == now` as both live and stranded --
  VERIFIED 2026-08-27, repair in progress.** Claim transitions correctly match
  Lodestar's inclusive live boundary, but active-work, Fleet, readiness,
  publication, and release queries used strict comparisons. At the exact
  expiry instant the server could refuse recovery while omitting the same claim
  from active coordination views, making a legitimate lease invisible and
  producing contradictory operator guidance.
