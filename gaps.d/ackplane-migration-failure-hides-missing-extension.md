- **Ackplane migration diagnostics hide a missing PostgreSQL extension -- OPEN.**
  On 2026-09-11, migrating a disposable native PostgreSQL 16 database without
  pgvector printed `projection schema failed: projection database error: db error`
  from `crates/ackplane-server/src/schema_migration.rs`. Only the PostgreSQL
  server log identified the absent `vector.control` file. The operator cannot
  diagnose the prerequisite from the migration command itself. Using the
  repository's pinned PostgreSQL/pgvector image allowed migration and tests to
  proceed. Left for later: expose a bounded, credential-safe cause or prerequisite
  diagnostic without dumping connection strings or SQL parameters.
