- **The shared Postgres connection ceiling failed `cargo test --all` in a
  pattern that read exactly like a semantic code break — ROOT-CAUSED and the
  development ceiling RAISED 2026-08-29; a residual is left OPEN.**

  Originally measured 2026-08-28 while validating an unrelated client-side
  change: 16-17 tests failed across five unrelated `ackplane-server` modules at
  once — `projection`, `signing_keys`, `supervisor_store`, `telemetry_service`,
  `readiness`, `knowledge_store`, `service::directive_receipt`. Every one was
  `SqlState(E53300)` "sorry, too many clients already" raised at a `connect`
  call site. Re-running the identical suite with `--test-threads=6` gave
  564 passed, 0 failed.

  **Why the ceiling was reached, measured 2026-08-29.** `docker-compose.yml`
  never set `max_connections`, so it was the Postgres default of 100 — never a
  considered value for this workload. Every store opens its own connection, so
  the two long-running development containers hold 28 before a single test
  starts: `ackplane-bridge` 17 and `ackplane-server` 11, confirmed two
  independent ways — counting `::connect(` call sites in each `main.rs` (17 and
  11), and `pg_stat_activity` grouped by `client_addr` (`172.19.0.2 → 17`,
  `172.19.0.5 → 11`). Those connections live for the container's lifetime: on
  the morning of 2026-08-29, 56 were idle for 14h54m. One crate's DB-gated
  tests then peaked at 73 connections total, and running two crates
  concurrently — which `cargo test --all` does — exhausted 100 outright.

  A restart of the Postgres container was itself a diagnostic: clearing the 56
  stale idle connections made the previously-failing
  `cargo test -p ackplane-bridge --bin ackplane-bridge` pass 73/73 unchanged.

  **Fixed for the development topology.** `docker-compose.yml` now starts
  Postgres with `max_connections=500`. Verified after the change: full
  `cargo test --all` reports 70 suites ok, 0 failed, 0 `E53300`, with a sampled
  peak of 80 concurrent connections — so the headroom is roughly six times
  measured demand rather than a guess. Capping test threads was rejected as the
  fix: it slows an already slow suite for everyone and hides the coupling
  instead of removing it.

  **Residual, OPEN.** Raising the ceiling removes the symptom at the measured
  workload; it does not make connection use *bounded*. Nothing limits how many
  connections the test suite or a service opens, so a large enough parallel run
  still exhausts whatever ceiling is set, and the failure will mislead the same
  way — the failing test *names* never mention connections, only the panic body
  does, so the visible signal is "several unrelated subsystems just broke" and
  an agent validating their own diff goes looking for a cause inside it. The
  real remedy is bounding connections in code, and that is a decision rather
  than a patch: several tests assert connection-level behaviour, so introducing
  a pool changes what they exercise. Deliberately not attempted here. This
  value is also a development-stack choice only, not a production tuning claim.

  **A correction worth keeping, because the wrong diagnosis was expensive to
  hold.** A separate fragment
  (`bridge-bin-tests-interfere-in-the-shared-test-database.md`, since deleted)
  recorded the same failures with a *different* stated cause: that concurrent
  enrolment was falsifying the absence-asserting
  `..._is_404_for_an_unenrolled_repository` tests, with per-test schema or
  tenant isolation proposed as the likely fix. That was a plausible inference
  from the failing test names, written as fact and never checked against a
  panic body. It was wrong. Capturing the actual panics showed every failure
  was `E53300` at a `connect` call. The `404` tests were over-represented for a
  mundane reason — those handlers connect to several stores each
  (`handlers/repository/mod.rs` opens Fleet, Readiness and Work), so they are
  simply the most likely to be holding a connection request when the ceiling is
  hit. Acting on that fragment would have meant building tenant isolation that
  fixed nothing. The transferable rule: a gap fragment should record the
  symptom that was *observed* and mark any inferred cause as inferred, because
  a plausible cause written as fact does not merely under-describe the defect —
  it can point the next reader at entirely the wrong repair.

  One claim from that fragment does survive re-measurement and is kept here:
  the failure is local-only in practice. These suites are green on every recent
  pull request, so CI's scheduling does not reach the ceiling, which makes this
  a trap CI will not catch on your behalf.
