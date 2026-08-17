- **`Projector::bounded_neighborhood`'s own regression test fails against a
  real Postgres database. MEASURED 2026-08-17, left OPEN — out of scope for
  the rotation-continuity task that surfaced it.**

  `bounded_neighborhood` binds `max_nodes: i32` directly as the `LIMIT $6`
  parameter (`crates/ackplane-server/src/projection.rs`, the final `SELECT` in
  the function). Postgres always describes a bare `LIMIT $n` parameter as
  `int8`, never `int4`, regardless of what it is compared against — the
  sibling `LIMIT $4` (bound from `max_fanout`) already casts to `i64` for
  exactly this reason, but `$6` does not. Every call fails before running:

  ```text
  thread 'projection::tests::bounded_neighborhood_admits_only_seeds_reachable_within_max_depth'
  panicked at crates\ackplane-server\src\projection.rs:636:14:
  bounded neighborhood: Database(Error { kind: ToSql(5), cause: Some(WrongType { postgres: Int8, rust: "i32" }) })
  ```

  Fix is narrow (bind `&(max_nodes as i64)` like the neighbouring parameter
  already does) but touches a file this task did not claim and serves a
  different clause than continuity proof. Filed rather than folded in.

  How this was found: fixing a neighbouring, unrelated defect
  (`EnrollmentStore::submit`'s timestamp binding — see the changelog) required
  running this crate's Postgres-gated tests against a real database for the
  first time to validate the fix. That run is what surfaced this one; nothing
  in CI exercises it, because ADR-0088 clause 2 deliberately keeps the default
  test run database-free.
