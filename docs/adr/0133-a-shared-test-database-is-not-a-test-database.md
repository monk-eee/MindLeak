# ADR-0133: A shared test database is not a test database

- Status: Accepted
- Date: 2026-08-26
- Deciders: MindLeak maintainers
- Related: [ADR-0088](0088-the-ackplane-runs-in-containers-the-planes-do-not.md)
  (Ackplane's supported Compose topology), [ADR-0132](0132-a-bind-address-is-not-a-reachable-interface.md)
  (a bind address is not a reachable interface — the same session's other
  Compose-topology finding), `gaps.d/a-running-projection-worker-invalidates-the-lagging-test.md`

## Context

`docker-compose.yml`'s local development topology and `cargo test`'s
`ACKPLANE_TEST_DATABASE_URL`-gated integration tests are documented to point
at the same Postgres database: the Compose stack's published
`127.0.0.1:${ACKPLANE_POSTGRES_PORT:-5432}/ackplane`. That is a deliberate
convenience — a developer who already ran `docker compose up` gets an
already-migrated database for free, rather than standing up a second Postgres
instance just to run tests locally. CI does not share this arrangement: its
own `ACKPLANE_TEST_DATABASE_URL` points at a GitHub Actions service container
that nothing else touches, so this ADR is about the local development loop
only.

That convenience has a real cost. `ackplane-server`'s test suite (46 files
under `crates/ackplane-server/src`) achieves test-to-test isolation entirely
through per-row uniqueness — `test_support::uuid_ish()`/`unique_id()` give
each test its own tenant, repository, and request identifiers so concurrently
running tests in the same binary never collide on a shared row. That
convention works between tests, because every test is equally bound by it.
It does not extend to a live `ackplane-server` process running outside the
test binary: the Compose stack's own `ackplane` service runs a real
projection worker, polling every repository whose ledger has moved past its
projection checkpoint (`ACKPLANE_PROJECTION_INTERVAL_SECS`, default 5s), with
no concept of "this repository belongs to a test and should be left alone."
A fresh, uniquely-named repository a test creates is, from that worker's
point of view, exactly as legitimate to drain as any other.

Measured 2026-08-26:
`readiness::tests::readiness_needs_attention_when_the_projection_is_lagging`
creates a repository, appends a structural fact, rebuilds the projection,
appends a second fact *without* rebuilding, and asserts the resulting state
reads `Lagging`/`AttentionNeeded`. That assertion is true only for as long as
nothing else rebuilds the projection in between. With the Compose stack up,
its projection worker can — and, given enough polling cycles, will — catch
the repository up before the test's own query runs. Same commit, same
machine, minutes apart: 491 passed with the stack down, 490 passed and this
one failed with the stack up, naming an assertion nowhere near anything the
commit touched.

Two shapes of fix were considered and rejected before this one:

1. **Make the projection worker refuse to touch a database it did not
   itself migrate.** This does not merely add friction, it is wrong on its
   own terms. ADR-0086 clause 1 requires every Ackplane instance to hold no
   authoritative local state and operate correctly against a ledger *any*
   instance already migrated — that is the entire point of the multi-instance
   deployment model a real Ackplane cluster depends on. A worker that
   refused a database it did not personally migrate would break every
   legitimate second instance joining an already-migrated cluster, to solve
   a problem that is purely local-development test hygiene. Rejected: it
   fixes the symptom by breaking the feature the symptom is a side effect of.
2. **Retry or widen the test's own assertion window**, the same shape of fix
   this session already shipped for a genuine OS-level timing race
   (`gaps.d/windows-migration-lock-retry-test-can-flake-with-permission-denied.md`).
   Rejected here for the opposite reason that fix was right there: that race
   needed the retry to *tolerate* a legitimate concurrent outcome (the lock
   really was available a moment later). This test's whole subject is
   *lag* — the absence of a consumer — and no amount of retrying converts
   "something drained it before I could observe the lag" into evidence that
   the code correctly reports lag. Retrying would not fix the test, it would
   hide the same non-determinism behind a coin flip that passes most of the
   time.

## Decision

**Local development's test database is a separate Postgres database on the
same Compose-managed Postgres server, not the same database the `ackplane`
service's projection worker operates against.**

1. Two new one-shot Compose services, following the exact pattern
   `tls-init` (ADR-0132) and `migrate` already established — idempotent, run
   to completion, ordered by `depends_on`/`service_completed_successfully`,
   never sleeps standing in for a real readiness check:
   - `test-db-init` creates an empty `ackplane_test` database if one does
     not already exist. Runs from the same pinned `pgvector/pgvector:pg16`
     image as `postgres` itself (one image's provenance to audit for this
     step, not a second base image, the same reasoning `tls-init` gave for
     reusing the server's own image rather than introducing another).
   - `migrate-test` runs the same `ackplane-migrate` binary the existing
     `migrate` service already runs, pointed at `ackplane_test` instead of
     `ackplane`.
2. **`ackplane_test` is reachable only through the same already-published
   Postgres port** (`127.0.0.1:${ACKPLANE_POSTGRES_PORT:-5432}`) as the dev
   stack's own database — no new port, no second Postgres container. A
   developer's `ACKPLANE_TEST_DATABASE_URL` changes only in the database name
   at the end of the connection string.
3. **Nothing in the Compose topology's own services is pointed at
   `ackplane_test`.** The `ackplane` service's `ACKPLANE_DATABASE_URL`
   is unchanged; its projection worker never learns `ackplane_test` exists,
   which is what removes the race rather than merely narrowing it.
4. **Documented, not enforced.** A developer can still misconfigure
   `ACKPLANE_TEST_DATABASE_URL` back onto the shared `ackplane` database, the
   same way nothing stops hand-editing any other documented environment
   variable. This is a convention fix, matching the convenience `.env`/
   `.env.example` already document rather than a new mechanical guard —
   proportional to a local-development-only race with a low-cost, already-
   documented workaround (stop the stack before testing), not a production
   correctness gap.

## Consequences

- The race in `readiness_needs_attention_when_the_projection_is_lagging` (and
  any future test in the same shape — asserting a lag, a backlog, a stale
  checkpoint, anything whose subject is "nothing has caught this up yet")
  stops being possible for a developer following the documented
  `ACKPLANE_TEST_DATABASE_URL`, because nothing with a live polling loop is
  ever pointed at the database that test touches.
- **Two more one-shot containers on every `docker compose up`.** Both are
  cheap (a `CREATE DATABASE` check and one already-fast migration binary
  invocation) and idempotent, so the steady-state cost after the first
  bring-up is negligible — the same trade-off already accepted for `migrate`
  and `tls-init`.
- **A developer who already exports `ACKPLANE_TEST_DATABASE_URL` pointing at
  `ackplane` keeps the old behaviour** until they update it to
  `ackplane_test`. This ADR documents the fix; it does not retroactively
  correct anyone's shell profile. The existing workaround (stop the stack
  before testing) remains valid for as long as anyone has not made that
  change.
- **This does not change CI**, which already uses an isolated service
  container nothing else touches, and does not change the per-row-uniqueness
  convention the other 46 files' tests already rely on for isolation from
  *each other* — this ADR only addresses isolation from a live *external*
  consumer, a different problem the row-level convention was never meant to
  solve.
