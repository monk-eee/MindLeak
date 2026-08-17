- MindLeak's SQLite open now gives itself 15s (was 5s) to finish enabling WAL,
  applying the schema, and migrating before reporting the database busy. The
  shorter budget was reliable on a dev machine but intermittently exhausted on
  Windows CI runners under disk contention, failing the maintenance runtime
  tests with a spurious `Busy` error even though nothing was actually
  deadlocked.
