- **Enrollment cleanup can still block while a credential service is unresponsive.**
  On `93e1730b`, CI run `34821402520`, attempt 1, Industrial job `103903583199`
  reported `activation_survives_a_failed_sync_and_a_cli_restart_without_replacing_identity`
  as failed after Secret Service reactivated at 08:15:40 UTC. The daemon could
  not find its default control socket; its graphical unlock prompt could not
  open a display. The lost-activation-response and TLS companion-restart tests
  then exceeded 60 seconds until the job was cancelled after 30 minutes.
  `scripts/credential-test.mjs::runCredentialTest` used to start a detached
  daemon without monitoring its lifetime. Commit `0d861606` adds foreground
  supervision with bounded startup and cleanup; deliberately killing the owned
  Linux daemon now reports failure and stops its test process group, including
  descendants. That contains daemon loss. In
  `crates/ackplane-server/tests/register_me_enrollment/support.rs`,
  `TestIdentity::remove_credential` performs synchronous credential cleanup
  outside the ten-second CLI timeout. Cleanup can still block if the credential
  service stays alive but stops answering; bound that remaining call so it cannot
  conceal assertions behind a job timeout.
  The original job did not capture the daemon exit cause or blocked call stacks.
  Later CI run `34834477362` on `dfafd360` captured daemon `SIGTRAP`, and the
  unchanged tests reproduced it with Ubuntu's same GNOME Keyring package in
  `target/ubuntu-keyring-1789384227468/`. The matching upstream `OpenSession`
  race is recorded separately in
  [known limitations](../docs/KNOWN-LIMITATIONS.md#external-tools); its crash is
  not a repository-owned fix. The original CI log is retained as
  `target/ci-34821402520-industrial-103903583199.log` in the artifact-verification
  checkout. Earlier Debian repetitions and a single same-source CI retry passed,
  but do not resolve either observation. The alive-but-unresponsive cleanup
  risk remains unresolved this run; daemon supervision alone does not cover it.
