- **The credential-facility-backed `ClaimSigner` has only ever been verified
  by hand, on Windows, once — OBSERVED 2026-08-18, left OPEN.**
  `CredentialFacilitySigner` (`crates/ackplane-client/src/auth.rs`, ADR-0100
  decision 5) is real, shipped code that stores/loads a signing seed through
  the `keyring` crate, which claims uniform support for Windows Credential
  Manager, macOS Keychain, and Linux Secret Service. Its own regression test,
  `a_stored_seed_round_trips_through_the_real_credential_facility`, is
  deliberately written to skip (not fail) when the facility is unavailable in
  the running environment — the same pattern this repo already uses for its
  Postgres-gated `ackplane-server` tests.

  The CI matrix (`.github/workflows/ci.yml`) runs only `ubuntu-latest` and
  `windows-latest`; there is no `macos-latest` job anywhere in this
  repository. GitHub's hosted `ubuntu-latest` runners have no D-Bus session
  bus / Secret Service daemon running by default, so on every CI run this
  test almost certainly hits its own skip branch there and never actually
  exercises the store/load round trip. The Windows CI job may or may not
  have a usable Credential Manager in that sandboxed context either --
  unconfirmed either way. The one time this test has demonstrably *not*
  skipped and produced a real pass was a manual, interactive run on this
  developer's own Windows machine, during the same session the feature was
  written.

  Impact: `resolve_identity` in `lodestar-mcp::federation` now *defaults* to
  `SignerSource::CredentialFacility` for any federated deployment that
  hasn't set `MINDLEAK_ACKPLANE_NODE_SIGNING_KEY_SEED` explicitly -- meaning
  the default, most-likely-to-be-used code path in this feature has never
  been proven to work in the project's own CI on any platform, and never at
  all on macOS. A regression in the keyring integration (a `keyring` crate
  upgrade changing its Linux backend selection, for instance) could land and
  merge with every check green, because the check that would catch it always
  reports "skipped" rather than "passed" or "failed" in this CI environment.

  **Guarded 2026-08-20 for legibility, not for real coverage:** the `rust` CI
  job now re-runs this one test with `--nocapture` on `ubuntu-latest`, and
  the test itself now prints "passed: round-tripped for real through the OS
  credential facility" on success as well as its existing "skipped: ..."
  line on failure to store -- closing option (b). This makes a
  permanently-skipping gate visible in the log; it does not make the gate
  real. Option (a) -- a CI job that installs/starts a real Secret Service
  provider so the test stops skipping on Linux at all -- remains open and
  unattempted.

  **Option (a) attempted 2026-08-20, outcome PENDING a real CI run --
  do not trust this paragraph over the actual log.** `ubuntu-latest` now
  installs `gnome-keyring` + `dbus-x11` before the credential-facility step
  (`continue-on-error: true`, so a failed install cannot turn the check red)
  and, when both `dbus-run-session` and `gnome-keyring-daemon` are present,
  wraps the test in a private D-Bus session with an empty-password login
  keyring primed via `gnome-keyring-daemon --unlock`. Falls back to the
  unwrapped command otherwise, so the existing soft skip remains the worst
  case. Validated locally only as much as a Windows machine allows: the YAML
  parses (PyYAML) and the embedded shell parses (`bash -n`) -- neither proves
  gnome-keyring actually unlocks a usable collection under GitHub's runner.
  This paragraph must be corrected (not just left standing) once the actual
  CI log for this step is read: either the test's own "passed: round-tripped
  for real..." line appears (closes option (a) for real) or it still prints
  "skipped: ..." (narrow this to name what was tried and why it still
  didn't work, the same way option (b) narrowed rather than closed).
