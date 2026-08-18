- **`ackplane-server`'s Postgres-gated tests race on schema creation against a
  genuinely empty (freshly `docker compose down -v`'d) database. MEASURED
  2026-08-17, left OPEN — out of scope for the `ReleaseClaim` task that
  surfaced it.**

  Every store's `connect()` runs its own `CREATE TABLE IF NOT EXISTS` /
  `CREATE TYPE` migration DDL, and dozens of `#[tokio::test]` functions each
  call `connect()` independently. `cargo test` runs them concurrently by
  default, so against a completely fresh container every test's connection
  races every other test's connection to create the same tables/types.
  Postgres's catalog (`pg_type`) is not immune to this: several stores hit

  ```text
  duplicate key value violates unique constraint "pg_type_typname_nsp_index"
  detail: Key (typname, typnamespace)=(delegated_claims, 2200) already exists.
  ```

  14 of 92 tests failed this way on the first run against a fresh volume;
  running the identical suite a second time (schema now present) failed with
  ZERO of these, proving it is a cold-start race and not a logic bug.

  How this was found: validating the `ClaimDelegationService.ReleaseClaim`
  RPC (task:32d76e33a3bd) required a `docker compose down -v` reset to get a
  known-clean baseline for the also-being-fixed `signing_keys` nonce
  collision (see the sibling gap on non-isolated runs). That reset is what
  exposed this second, independent race — it never appears on an
  already-migrated container, which is what every prior session in this repo
  had been running against.

  Fix direction (not attempted here): either serialise migrations with a
  Postgres advisory lock (`pg_advisory_lock` around the DDL block) so
  concurrent `connect()` calls queue instead of racing, or have the test
  harness run one explicit migration pass before spawning the parallel test
  threads (a `#[ctor]`-style once-only setup, or `cargo test -- --test-threads=1`
  documented as required for a cold container). The advisory-lock approach
  also protects any future caller of `connect()` outside tests, not just this
  suite.
