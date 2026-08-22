- **Fixed concurrent Ackplane schema migrations that could deadlock on related
  tables.** Every migration now acquires one global schema lock before its
  per-file advisory lock and records its applied key, so parallel store startup
  cannot interleave Evidence and other DDL transactions or repeat warm-schema
  `ALTER TABLE` work into a PostgreSQL deadlock.
