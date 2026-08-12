- **The coordination mode is declared per process, not per repository.** —
  `resolve_coordination_mode` in `crates/ackplane-core/src/lib.rs` reads
  `MINDLEAK_COORDINATION_MODE` from the environment, so the two planes are
  configured independently and nothing stops one process being started `local`
  while another is started `federated`. ADR-0082 decision 3 makes the mode a
  property of the *repository*, which a repository-scoped declaration — the
  existing `.mindleak.toml`, or Git config beside `mindleak.repositoryId` —
  would actually enforce. — Impact is bounded today because every build refuses
  `federated`, so the only reachable disagreement is between two `local`
  processes, which agree anyway; it becomes load-bearing the moment an Ackplane
  client exists. — Left for later: resolution is one function, so the channel
  can change without moving the invariant.
