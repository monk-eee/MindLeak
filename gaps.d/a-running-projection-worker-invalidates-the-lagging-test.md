- **A test that asserts a projection is BEHIND is only valid while nothing else
  is catching it up, and the dev stack catches it up.**
  `readiness::tests::readiness_needs_attention_when_the_projection_is_lagging`
  ([`crates/ackplane-server/src/readiness.rs`](crates/ackplane-server/src/readiness.rs))
  appends a structural fact after a rebuild and asserts the repository reads
  `Lagging` / `AttentionNeeded`. That holds only if no projection worker drains
  the ledger in between. `docker compose up` starts one, pointed at the same
  `ACKPLANE_TEST_DATABASE_URL` database a developer runs the suite against, so
  with the standalone stack running the test observes `Fresh` / `Ready` and
  fails. Measured this run: 491 passed with the stack down, then 490 passed and
  this one failed at the pre-push hook with the stack up — same commit, same
  machine, minutes apart, and the failure names an assertion nowhere near
  anything the commit touched. PORTABLE: any test whose subject is *lag* —
  a queue depth, an unacked backlog, a stale cache, a checkpoint behind a log —
  asserts the absence of a consumer, and absence is not something a test fixture
  can hold still while a real consumer shares its storage. Left for later: the
  durable fix is for the test to use a database the dev stack cannot reach, or
  for the projection worker to refuse a database it did not migrate. Worked
  around this run by stopping `ackplane-ackplane-1` before pushing.
