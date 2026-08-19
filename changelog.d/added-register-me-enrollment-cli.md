### Added

- `register-me` (`crates/ackplane-server/src/bin/register-me.rs`): a CLI for
  the ADR-0085 enrollment ceremony, as three explicit steps (`request`,
  `approve`, `activate`) mirroring the real actors — a node submits and
  activates unattended; approval is a documented local-dev database shortcut
  standing in for the administrative approval RPC/UI ADR-0085 does not build
  yet. `activate` opens one real `NodeSync` stream and sends a signed
  heartbeat event using the `signing_key_id` `EnrollmentActivationResult`
  returns directly, so no database access is needed for that step.
