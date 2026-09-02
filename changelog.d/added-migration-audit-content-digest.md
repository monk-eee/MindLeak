- `migration-audit` now reports a migration key that a live database applied under
  content the committed migration no longer carries, and audits `ackplane_test`
  alongside `ackplane`. `migrate_locked` already refuses such a key at runtime, but
  only once some store's `connect()` happens to reach it — surfacing as a wall of
  failures in whichever subsystem asked first, naming a diff that is usually
  innocent. Measured on the shared containers the moment it shipped: keys 60 and 61
  in `ackplane_test` both hold the pre-split bundled display-label migration that a
  later commit split in two, so every `ConstitutionStore::connect` refuses and a
  full `cargo test` reports 58 failures across five unrelated subsystems. The audit
  reported that database as clean beforehand for two independent reasons, both now
  closed: it compared keys but never content, and it only ever looked at `ackplane`
  — not `ackplane_test`, the database [ADR-0133](../docs/adr/0133-a-shared-test-database-is-not-a-test-database.md)
  gave every `cargo test` run. A row written before `migrate_locked` gained its
  digest column is reported as adopted rather than as a mismatch, since flagging
  those would fire on every long-lived database. The live findings stay non-fatal
  under `--check`, matching the existing live-only finding: a fresh CI database can
  never exhibit them.
