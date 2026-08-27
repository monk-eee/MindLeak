### Added

- **Supervisor worker adapter contract (ADR-0116 decisions 5 and 9).**
  `ackplane-supervisor` gains a `WorkerAdapter` trait and a
  `ProcessWorkerAdapter` reference implementation: a small, runtime-neutral
  contract that starts a declared command and argument vector (never a shell
  string), observes and terminates it, and refuses every action against a
  worker id it did not itself register. Checkpoint, pause, and drain default
  to an honest, typed refusal rather than an approximated success, since a
  generic OS process cannot honestly support them.
