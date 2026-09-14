- **Linux credential-service loss failed enrollment coverage and stalled CI.**
  On `93e1730b`, CI run `34821402520`, attempt 1, Industrial job `103903583199`
  reported `activation_survives_a_failed_sync_and_a_cli_restart_without_replacing_identity`
  as failed after Secret Service reactivated at 08:15:40 UTC. The daemon could
  not find its default control socket; its graphical unlock prompt could not
  open a display. The lost-activation-response and TLS companion-restart tests
  then exceeded 60 seconds until the job was cancelled after 30 minutes.
  `scripts/credential-test.mjs::runCredentialTest` starts a detached daemon
  without monitoring its lifetime. In
  `crates/ackplane-server/tests/register_me_enrollment/support.rs`,
  `TestIdentity::remove_credential` performs synchronous credential cleanup
  outside the ten-second CLI timeout. Capture daemon loss and bound test cleanup
  so a failed credential facility cannot conceal assertions behind a job timeout.
  The original daemon exit cause and blocked call stacks were not captured.
  A separate Debian Linux probe passed 768 synthetic Secret Service operations
  with four concurrent clients and an unchanged service owner; it did not
  reproduce the enrollment failure. The original CI log is retained as
  `target/ci-34821402520-industrial-103903583199.log` in the artifact-verification
  checkout. A single same-source job retry was requested; it is not a fix.
  Left unresolved this run, including if the retry passes.
