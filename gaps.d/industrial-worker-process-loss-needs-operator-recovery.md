- **Automatic reconciliation after supervisor process loss is not implemented.**
  In `crates/ackplane-supervisor/src/daemon/runtime.rs` and `daemon/mod.rs`,
  a durable worker-run marker prevents a new process from silently reusing a
  workspace whose old worker or pending receipts are unaccounted for. Ordinary
  connection reconnects resend the existing outbox; process loss instead stops
  for an operator to inspect the named session, process tree and queues. This
  avoids duplicate execution but requires manual recovery before that slot can
  run again. Fixed this run: fail-closed slot reuse, tested by
  `an_unaccounted_worker_run_refuses_startup_before_reusing_its_workspace`.
  Left for later: an explicit recovery operation that reconciles old receipts
  and establishes worker termination without attaching to unrelated processes.
