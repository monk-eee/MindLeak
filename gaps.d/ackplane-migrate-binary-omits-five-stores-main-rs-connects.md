- **What I observed:** `crates/ackplane-server/src/bin/migrate.rs`'s own doc
  comment says it "applies every table this deployment needs" so the
  `ackplane` service "can start only after migrations have finished rather
  than racing them at boot" -- but its `migrate()` function only connects
  `LedgerStore`, `Projector`, `EvidenceStore`, `DelegationStore`, and
  `SupervisorStore` (5 stores). `crates/ackplane-server/src/main.rs`'s own
  startup additionally connects `EnrollmentStore`, `ClaimStore`,
  `KnowledgeStore`, `ConstitutionStore`, and `TelemetryStore` (9 stores
  total) -- each of those 5 missing stores still runs its own
  `CREATE TABLE IF NOT EXISTS` migration as a side effect of `main.rs`'s own
  `connect()` call, so the dedicated pre-migration step this binary exists
  for (see its cited regression: "14 of 92 tests failed this way on a fresh
  `docker compose down -v` container") is not actually covering those 5
  stores. Discovered while adding `EnrollmentStore`'s sixth migration
  (`enrollment_status_authentication_nonces`, ADR-0122) and checking whether
  `migrate.rs` needed a matching update -- it turned out `EnrollmentStore`
  was never in `migrate.rs`'s list at all, before or after my change.
- **Where:** `crates/ackplane-server/src/bin/migrate.rs` (`migrate()`
  function, lines ~44-60) vs. `crates/ackplane-server/src/main.rs` (lines
  ~50-122).
- **Impact:** On a genuinely cold database, the Compose `migrate` service
  can report success while `EnrollmentStore`, `ClaimStore`,
  `KnowledgeStore`, `ConstitutionStore`, or `TelemetryStore`'s schema has
  not been created yet, reintroducing the exact concurrent-`CREATE TABLE`
  race this binary was built to close for those five stores if the
  `ackplane` service's own boot-time `connect()` calls run concurrently
  across replicas.
- **Fixed this run:** No -- out of scope for the enrollment-status-check
  feature this session is implementing, and touching a shared migration
  entrypoint deserves its own focused change and test rather than riding
  along inside an unrelated feature's diff. Left for a follow-up task.
