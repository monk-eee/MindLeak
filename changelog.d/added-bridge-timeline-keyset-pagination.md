### Added

- The Bridge repository timeline gains keyset pagination (ADR-0112 decision
  1): `GET /api/v1/repositories/:id/timeline` accepts an optional `before`
  cursor (a `stream_position`), and the response carries `next_before` so a
  caller can page strictly older than what it already saw, without the
  skip/repeat drift `OFFSET` would risk against a continuously appended
  ledger. Previously the endpoint always returned only the newest 50 events
  with no way to see anything older. `claims` and `knowledge` still return
  a fixed 50-item page; ADR-0112 names the same keyset shape for each when
  next touched.
