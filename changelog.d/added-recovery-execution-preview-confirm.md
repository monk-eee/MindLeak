- Implemented ADR-0145 decisions 4-5: `ackplane-bridge` gains a production
  recovery-execution preview/confirmation workflow
  (`POST /api/v1/administration/recovery-executions`,
  `.../:request_id/confirm`, `GET .../:request_id`), reusing ADR-0134's
  dual-signing-key Lifecycle-purge pattern verbatim through a distinct
  domain separator (`RecoveryExecutionOperation`,
  `RECOVERY_EXECUTION_DOMAIN`) so a purge-confirming key's signature can
  never verify for a recovery-execution operation or the reverse. The
  preview's explicit impact plan names the artifact and its digest
  (cross-checked against the artifact's own Snapshot receipt), the passing
  rehearsal report relied on, and triggers a fresh platform Snapshot as its
  own pre-restore safety point as part of preview construction -- that
  Snapshot's failure fails the preview outright, before any
  recovery-execution request row exists. Confirming here never runs
  `pg_restore`: it only records that a second, distinct enrolled key
  authorized the request (`confirmed`/`refused`/`expired`); production
  execution itself is separate, later work (ADR-0145 slice 4) this ADR names
  but does not implement yet. Always platform-scoped (never per-tenant or
  per-repository), though disclosure of a request/confirmation stays
  bounded to the Bridge tenant that made it.
