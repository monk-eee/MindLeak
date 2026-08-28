- **The enrolled supervisor is now a program you can actually run (ADR-0116
  slice 5).** `ackplane-supervisor` gains a binary that connects to Ackplane
  over authenticated gRPC, registers, opens a session, heartbeats, receives
  directives and durably receipts them, and on reconnect reconciles its
  position rather than resuming through a gap. Slices 1-4 built each of those
  as library code exercised only by tests; nothing an operator could start
  existed anywhere in the repository until now.
  It has no worker adapter yet, and its registration says so rather than
  discovering it at delivery time. It declares exactly one capability,
  `notify` — a notification is complete once durably recorded, so accepting one
  is truthful — and declares none of the worker-driving capabilities
  (`prompt`, `assign`, `steer`, `pause`, `resume`, `drain`, `terminate`).
  Ackplane therefore refuses to enqueue work this supervisor cannot do, and a
  directive that arrives anyway is receipted `refused` / `capability_missing`.
  An `accepted` receipt for work nothing performed is unreachable rather than
  merely unlikely (ADR-0116 decision 10).
  Configuration reuses the `MINDLEAK_ACKPLANE_*` variables `register-me` and
  the federated claim path already use, so an enrolled node is already
  configured; only the supervisor's own id is new. A missing variable is
  refused at startup with **every** missing one named at once. See
  `crates/ackplane-supervisor/README.md` to run it against the Compose
  topology.
