- **Coverage uses the isolated `ackplane_test` database name.** Its PostgreSQL
  service, readiness probe, and test/rehearsal URLs now agree with the browser
  integration safety guard. The guard still refuses the live-service database
  name; tests are not skipped or relaxed to make coverage pass.
