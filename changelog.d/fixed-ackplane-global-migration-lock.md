- **Fixed concurrent Ackplane schema migrations that could deadlock on related
  tables.** Every migration now acquires one global schema lock before its
  per-file advisory lock, so parallel store startup cannot interleave Evidence
  and other DDL transactions into a PostgreSQL deadlock.
