# ADR-0118: The Coverage gate provisions Postgres for Ackplane's tests

- Status: Accepted
- Date: 2026-08-21
- Deciders: MindLeak maintainers
- Accepted: 2026-08-22 by the repository owner, authorized directly in
  session — attributed human adoption after review.
- Depends on: [ADR-0091](0091-ackplane-builds-and-tests-without-a-database.md)
  (database-gated tests are opt-in via `ACKPLANE_TEST_DATABASE_URL`),
  [ADR-0088](0088-the-ackplane-runs-in-containers-the-planes-do-not.md)
  (Ackplane's supported container topology; the local planes stay
  container-free)
- Related: [ADR-0086](0086-postgresql-is-the-ackplane-ledger-and-arbiter.md)
  (PostgreSQL is the ledger and arbiter)

## Context

`gaps.d/the-coverage-gate-penalizes-postgres-gated-ackplane-tests.md` names a
structural problem in `.github/workflows/ci.yml`'s `coverage` job (`cargo
llvm-cov report --fail-under-lines 80`, via the Makefile's `coverage` target):
it provisions no `services:` block and sets no `ACKPLANE_TEST_DATABASE_URL`.
Every test in `ackplane-server` gated behind
`let Ok(url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else { skip }`
therefore hollow-skips in that job specifically — it prints `skipped` and
passes with zero assertions executed, while the exact same test runs for real
on a developer machine with `docker compose up -d postgres` and the variable
set.

This is by design for the plain `rust` test job and for `cargo test` on a bare
developer machine: ADR-0091 decision 3 makes that skip the mechanism by which
`cargo test` stays runnable with "no Docker, no PostgreSQL, and no network".
But the `coverage` job's purpose is different from the `rust` job's. `rust`
proves the code *builds and passes*; `coverage` measures *how much of it is
exercised*. A file that hollow-skips its own tests in that measurement counts
as written-but-untested, indistinguishable from a file that genuinely ships
without tests.

The consequence, observed directly on two PRs in the same week: `ackplane-server`
now holds a large and growing share of the workspace's line count
(`claim_service`, `claim_store`, `enrollment_store`, `fleet`, `evidence_store`,
`knowledge_store`, `signing_keys`, and more — all `tokio_postgres`-backed).
Every one of those files' real tests is invisible to `coverage`. The aggregate
line percentage has been pushed to a ~79-81% band that sits right on the 80%
gate:

- `origin/main`'s own last successful run (immediately after
  [task:22f9d48b8414](../../gaps.d) / PR #590 fixed `worktree_roots`'s
  unrelated 0%-covered lines): 80.29% — barely over.
- The very next PR touching `ackplane-server` (`feat/bridge-knowledge-lifecycle`,
  PR #589): 79.23%, then 79.15% on an unmodified re-run — barely under.
- The same commit, built and measured locally on a machine with
  `docker compose up -d postgres` and `ACKPLANE_TEST_DATABASE_URL` set:
  90.65% lines — because every Postgres-gated test actually executed.

The percentages before and after crossing the 80% line differ by whether
Postgres happened to be reachable when `cargo llvm-cov` ran, not by whether
the changed code was tested. A gate that measures environment availability
instead of test coverage is not doing its job, and it fails or passes PRs for
reasons that have nothing to do with their diffs.

ADR-0091 decision 5 already anticipated exactly this gap and left it
unresolved on purpose, sequenced behind other work landing first:

> A PostgreSQL-backed CI job is additive and sequenced, never a replacement.
> It runs the ignored tests against a service container, and it is added only
> after the job proving the local planes need no container runtime has
> landed. That job is the guarantee; this one is coverage. If only one can
> exist, it is the guarantee.

The `local-planes` job (the guarantee) has been live in `ci.yml` for some
time. The Postgres-backed wiring for the coverage measurement was simply never
implemented. This ADR is that implementation decision, not a new idea:
ADR-0091 already decided a Postgres-backed CI job should exist; what remained
undecided, and is genuinely consequential enough to warrant sign-off rather
than a silent CI edit, is *which* of two shapes it takes and what it costs.

## Decision

**Provision a real PostgreSQL service container for the existing `coverage`
job, and nothing else.**

1. **Only the `coverage` job gains a Postgres dependency.** The `rust` job (on
   both `ubuntu-latest` and `windows-latest`) and the `local-planes` job keep
   ADR-0091 decision 3's and ADR-0088 decision 2's zero-container guarantee
   exactly as accepted — `cargo build`/`cargo test --all` on a bare machine,
   with no Docker, no PostgreSQL, and no network, remains proven by a job that
   has none of those available. This ADR changes what one measurement job
   provisions, not what the workspace requires to build or to pass its
   ordinary test suite.

2. **The service container is the same pinned image and credentials
   `docker-compose.yml` already uses**, not a second, independently-maintained
   definition:
   `pgvector/pgvector:pg16@sha256:ccc6e83d6e35e931dc7c5def2022729d5a6c370318d099181995567ff1fb4d6b`
   (pgvector is required, not optional — the knowledge domain's `ivfflat`
   index needs the extension the plain `postgres` image lacks), user/password/
   database `ackplane` / `ackplane-development-only-not-for-production` /
   `ackplane`, with the same `pg_isready` health check gating the job's first
   test step. Ackplane's own `docker-compose.yml` is already the accepted,
   reviewed topology for exactly this database (ADR-0088 decision 1); reusing
   its pin is copying established, reviewed infrastructure, not inventing new
   infrastructure. A future rotation of that image must update both places in
   the same commit, same as any other pinned-dependency bump; this is a known,
   ordinary maintenance cost, not a new class of risk.

3. **`ACKPLANE_TEST_DATABASE_URL` is set for the coverage job's test step**,
   pointed at the service container's `localhost` port. No separate migration
   step is added: every `ackplane-server` store already applies its own
   embedded SQL idempotently on `connect()` (`migration_lock.rs`'s
   Postgres-advisory-lock-guarded apply), so a fresh, empty database is
   sufficient and each store migrates itself the first time a test opens it —
   exactly the same as a developer's local `docker compose up -d postgres`
   workflow already described in
   `gaps.d`/the repository's own Postgres-gated-tests-were-rotting lesson.

4. **This measures, it does not lower the bar.** `--fail-under-lines 80` stays
   80; the fix is making the number the gate reads be an honest one. No
   `--ignore-filename-regex` exclusion is introduced.

## Rejected alternatives

**Exclude Ackplane's Postgres-gated store files from `--fail-under-lines` via
`cargo llvm-cov`'s `--ignore-filename-regex`.** This was the gap fragment's
other named option, and it is real, cheap, and does not touch CI's dependency
footprint at all. It is rejected for three reasons taken together:

- It does not fix the actual defect (Postgres-gated code is never proven to
  run in CI); it only stops that gap from being *counted*, which is a
  narrower and weaker claim than "this file is well tested."
- An excluded file's future regressions become invisible to the one
  mechanism meant to catch them. A coverage drop inside
  `enrollment_store.rs` after this ADR would be caught; the same drop inside
  a file matched by an exclusion pattern would not be, silently.
- The exclusion pattern needs to grow every time a new Postgres-backed file
  is added, which is exactly the kind of ungoverned, ever-widening allowlist
  this repository's own conventions warn against elsewhere (a second
  `_v2`-shaped escape hatch beside the real mechanism). It also invites a
  future PR to dodge writing tests by adding its file to the exemption list
  rather than testing it, which the current honest-if-strict gate cannot be
  argued into doing.

**Run the Postgres-gated tests in a brand new, separate job, leaving
`coverage` as-is.** This satisfies ADR-0091 decision 5's literal text most
narrowly (only the `coverage` job's own claim ever needed a database is
untouched) but does not fix the reported symptom: `coverage`'s own
`--fail-under-lines 80` would keep measuring the same undercounted
percentage regardless of what a sibling job proves. A job that proves the
Postgres-gated tests pass, without ever feeding that execution into the
coverage tool, leaves the actual gate exactly as broken as it is today.

**Lower `--fail-under-lines` below 80.** Rejected outright: it does not
change what is measured, only how much of an already-wrong number the gate
tolerates, and it silently accepts every already-landed Postgres-gated file
staying permanently uncovered in CI's eyes.

## Consequences

- `coverage` becomes a job whose success depends on a container image pull
  and a health check succeeding, which the `rust`/`local-planes` jobs
  deliberately do not. A transient registry or network failure can now fail
  `coverage` for a reason unrelated to the diff — the same category of
  flakiness any service-container CI job accepts, mitigated by the health
  check and by this being one job, not the required build/test gate.
- The coverage percentage will jump substantially once this lands (comparable
  to the ~90% measured locally with Postgres available for the same commit),
  which will read as a large, one-time improvement in the CI dashboard rather
  than incremental progress. That jump is the fix becoming visible, not new
  work landing.
- `docker-compose.yml`'s pinned Postgres image/digest and this workflow's copy
  of the same pin can drift apart if only one is updated. No automated check
  enforces they match; a future hardening could add one, but is not required
  to accept this decision.
- This does not change anything about `ACKPLANE_TEST_DATABASE_URL`'s opt-in
  behavior for `cargo test` on a bare developer machine or in the `rust` CI
  job — ADR-0091's guarantee is untouched.
