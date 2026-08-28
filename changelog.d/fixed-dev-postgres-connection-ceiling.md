- **The development Postgres no longer runs out of connections part-way through
  `cargo test --all`.** `docker-compose.yml` never set `max_connections`, so it
  was the Postgres default of 100 — a value nothing had chosen for this
  workload. Every store opens its own connection, so the two long-running
  development containers hold 28 before a single test starts (`ackplane-bridge`
  17, `ackplane-server` 11), and those persist for the container's lifetime;
  56 were once measured idle for nearly 15 hours. One crate's database-gated
  tests then peaked at 73 connections, and running two crates concurrently —
  which `cargo test --all` does — exhausted the ceiling outright.
  The resulting failure was badly misleading rather than merely inconvenient:
  `SqlState(E53300)` "sorry, too many clients already" surfaced as a dozen
  unrelated subsystems failing at once, and because the failing *test names*
  never mention connections, an agent validating their own change would go
  looking for a cause inside it. The stack now starts Postgres with
  `max_connections=500`, verified by a full `cargo test --all` reporting 70
  suites ok, 0 failed and 0 `E53300` at a sampled peak of 80 connections. This
  is a development-topology value, not a production tuning claim, and it raises
  the ceiling rather than bounding connection use — the latter remains recorded
  as an open gap.
