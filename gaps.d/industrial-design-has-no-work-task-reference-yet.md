- **What**: `industrial_designs` (ADR-0121 decision 3) has no `work_task_id`
  column/foreign key, even though the ADR's own decision 3 text lists Work
  as one of the three reference domains ("immutable references to
  Constitution publications and Work/Evidence records").
- **Where**: `crates/ackplane-server/migrations/0027_industrial_designs.sql`,
  `crates/ackplane-server/src/design_store.rs`.
- **Impact**: a design record cannot yet link to the Industrial Work task it
  originated from or resulted in. Low impact today (nothing reads or writes
  that link yet), but the Bridge Design Board (decision 7) will eventually
  want to navigate from a design to its linked Work.
- **Why deferred, not a bug**: the Work domain's own schema
  (`work_tasks` and friends) is not yet merged to `origin/main` -- confirmed
  via `git grep` finding no `CREATE TABLE ... work_tasks` in any committed
  migration. An earlier version of this migration DID add the FK, and it
  passed every LOCAL test, because the long-lived shared dev Postgres
  container already had `work_tasks` from a different, still-unmerged
  session's local testing. It only failed in CI, against a genuinely fresh
  database, with `relation "work_tasks" does not exist`. Add the column and
  its FK once the Work domain's own migration lands on `main` -- a small,
  additive follow-up migration, not a rewrite of this one.
