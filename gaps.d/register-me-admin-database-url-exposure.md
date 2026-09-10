- **`register-me approve` can expose database credentials in error messages - OPEN.**
  Observed while stabilizing enrollment on `7991ba3a`:
  `crates/ackplane-server/src/bin/register-me/commands.rs::run_approve` inserts
  the complete `database_url` into both pool-construction and connection-failure
  messages. `main` prints those errors to stderr, so a URL containing a password
  reaches terminal output and may then enter captured execution evidence.
  Replace the URI with an operation label, preserve useful non-secret failure
  context, and add malformed-URL and connection-refusal tests that assert a
  sentinel password never appears. Left for the next STAB-02 change; the
  key-preservation fix does not alter this separate approval path.
