- **Recovery of live or unproven workers after supervisor loss remains unimplemented.**
  In `crates/ackplane-supervisor/src/daemon/runtime.rs` and `daemon/mod.rs`,
  a durable worker-run marker prevents a new process from silently reusing a
  workspace whose old worker or pending receipts are unaccounted for. Ordinary
  connection reconnects resend the existing outbox. Fixed this run:
  `recovery::inspect` and `recovery::confirm` finish cleanup for versioned runs
  with positive adapter-stop provenance, exact receipt replay, original-owner
  release and durable completion before marker removal. The real two-worker
  `stopped_run_recovery_survives_supervisor_crash_lost_replies_and_completion_write_failure`
  test covers supervisor/recovery crashes and fresh authorized execution.
  Left for later: a trustworthy external process-ownership mechanism for live
  or unproven workers, old markers and missing stop records. PID absence, lease
  expiry and lifecycle labels do not prove termination. The live-worker
  recovery test confirms refusal without signalling or releasing those workers;
  automatic slot reuse and inconsistent-history repair remain intentionally blocked.
