- Fixed `Projector::bounded_neighborhood` (`crates/ackplane-server`) failing
  against a real Postgres database: its final `LIMIT` bound `max_nodes: i32`
  directly, and Postgres always types a bare `LIMIT $n` parameter as `int8`
  regardless of context. Cast to `i64`, matching the neighbouring `max_fanout`
  parameter's existing pattern. Verified against a live database via
  `ACKPLANE_TEST_DATABASE_URL`.
