# Industrial Stabilization

Started 2026-09-10. Goal: `goal:stabilize-the-industrial-product`.
Baseline: `b6f79e8da3ccb860f3541ea442d1aa791efbde32` on `origin/main`.
Implementation starts on `agents/industrial-stabilization` in an isolated worktree.

## Scope and Exit Criteria

Stabilize Ackplane, the Bridge, enrolled supervisors and their clients as one
operable product. Preserve the local planes, existing public contracts and
deterministic ingestion. This is integration and hardening, not a rewrite.

The pilot must demonstrate the following workflow from packaged artifacts:

```text
install -> enroll -> claim -> obtain context -> execute a real worker
        -> record evidence -> check conformance -> complete -> restart/recover
```

A healthy container, a process exit or a passing unit suite alone cannot satisfy
this gate. Two enrolled nodes must execute separate work without crossing task,
repository, tenant or worker boundaries. Accepted evidence must survive restart;
replays must not duplicate execution. Missing data and unsupported capabilities
must be visible failures, not successful empty results.

Remote Bridge exposure is prohibited until STAB-06 passes. The existing
loopback-only development topology is not a production authentication system.
No task below authorizes modifying a live database or exposing a service.

## Delivery Rules

- One active implementation task and isolated branch per workstream. Claim and
  renew before editing; record tests and failures against the owning task.
- Review and integrate existing worker/context/setup work through normal PR
  merges. Do not copy a peer's uncommitted files or rebuild over their checkout.
- Freeze new capabilities outside this roadmap. Narrow a task into tested changes
  when needed; do not add compatibility shims just to get a green build.
- Before closing a task, run its acceptance tests, required lint/build checks and
  conformance. Keep failed or unverified requirements open. Code completion and
  deployment qualification are different milestones.
- The ledger orders these tasks as a single-writer handoff chain. Security can be
  reprioritized with an explicit plan update; it must never be bypassed for a
  remotely accessible pilot.

## Task Board

Estimates are engineering days for one experienced engineer familiar with this
repository. They are provisional, not deadlines; reassess after STAB-03.

| Task | Ledger ID | Status | Estimate | Acceptance Summary |
| --- | --- | --- | --- | --- |
| STAB-01: Reproducible build and database gate | `task:52eb3f6d82bd` | Merged, CI passed, required check enabled | 1-3 days | Fail closed without database/recovery prerequisites; migrate explicitly; compile every target; run all-feature workspace tests against disposable Postgres in CI and locally. |
| STAB-02: Installation and enrolled identity | `task:f60a0347a46d` | In progress: preserve enrollment keys | 3-5 days | Clean-machine TLS setup, tenant-consistent enrollment, persisted identity, actionable refusals and idempotent repeat setup. |
| STAB-03: Real worker execution and isolation | `task:df1e790eefca` | Queued after STAB-02 | 5-10 days | Addressed, authenticated work drives a real configured worker with bounded current context in its own worktree; two-node isolation and peer-impersonation refusals pass. |
| STAB-04: Completion and restart recovery | `task:a200ebd9ec16` | Queued after STAB-03 | 5-8 days | Attributed evidence and conformance govern completion; crashes, reconnects, duplicate messages, expired claims and lost outbox state cannot silently lose or repeat work. |
| STAB-05: Shared context and honest freshness | `task:8edce4b7d4a9` | Queued after STAB-04 | 4-7 days | Enrolled-node embedding production feeds shared recall; invalidation, cross-tenant refusal and explicit unembedded/stale/unavailable states are tested. Resolve the Work freshness design mismatch explicitly. |
| STAB-06: Production authentication | `task:9be81e654a23` | Queued after STAB-05 | 8-15 days | Real identity verification and per-operation tenant authorization cover reads, mutations and enrollment approval; browser request protections and negative security tests pass. |
| STAB-07: Operations and data recovery | `task:5c3861ce6973` | Queued after STAB-06 | 5-10 days | Upgrade and encrypted backup/restore drills reconcile data and evidence; database loss, pool saturation and write failures are bounded, visible and documented. |
| STAB-08: Artifact qualification and pilot | `task:f3efac2bfdeb` | Queued after STAB-07 | 3-5 days plus seven-day pilot | Versioned artifacts from one reviewed commit pass clean install, upgrade and real two-node workflows; a sustained pilot has no unresolved release blockers. |

This is roughly 7-13 engineering weeks plus pilot observation. The first useful
confined industrial demonstration targets STAB-03, not the completion of every
production requirement. A production deployment needs the entire chain and may
require more work after security and capacity measurement.

## First Gate

Requirements: Rust 1.88+, Node 20+, PostgreSQL 16 client tools (`pg_dump` and
`pg_restore`) on `PATH`, and a disposable pgvector/PostgreSQL 16 instance.
The role must be able to migrate schemas, install pgvector, and create/drop
scratch databases for recovery tests. Never use a production or shared live
deployment database.

Set both environment variables explicitly using the environment mechanism of
your terminal, task runner or CI system:

| Variable | Meaning |
| --- | --- |
| `ACKPLANE_TEST_DATABASE_URL` | Disposable database for ordinary integration fixtures. The runner migrates this database before executing tests. |
| `ACKPLANE_TEST_REHEARSAL_DATABASE_URL` | Maintenance connection on a disposable PostgreSQL instance, used to create and drop scratch databases for restore tests. It may be the same URL as the test database because the fixtures create separate databases. |

```bash
node scripts/industrial-test.mjs
```

`make industrial-test` is the same command. The runner refuses missing or invalid
URLs and missing client executables, overrides `ACKPLANE_DATABASE_URL` for its
children with the explicit test URL, then builds all workspace test targets,
runs the existing `migrate` binary, and executes the all-feature workspace suite
with two build jobs and four test threads. It stops at the first command failure
and preserves its exit status. It does not start, stop or reset any deployment.

The caller owns the disposable database lifecycle. URL validation is not proof
that a database is disposable; the runner cannot infer that from its name.
Existing ignored tests requiring external models or a platform credential service
retain their own prerequisites. An enabled database gate is not a claim that
those external integrations ran.

PR [#920](https://github.com/monk-eee/MindLeak/pull/920) merged as
`7991ba3a9aafe4d6a6012fd160b405856abb7b3c` after all applicable CI jobs passed.
With explicit user approval, **Industrial (database and recovery)** is now a
required check on `main`, preserving the six previous checks and strict
up-to-date enforcement. STAB-01 is complete with aligned conformance.

## Baseline Evidence

- Isolated `main`: all workspace targets compile with all features enabled.
- STAB-01 runner: six focused Node tests pass, covering missing configuration,
  credential-safe URL errors, ordering, live-URL replacement, command failure and
  terminated processes. The tests were introduced red before the runner existed.
- Full runner passes against fresh, migrated disposable PostgreSQL with both
  database gates enabled and PostgreSQL 16 recovery tools available. Repeating
  through `make industrial-test` passes 2,556 tests with zero failures and three
  existing ignored tests across 78 test targets. The disposable database was
  removed after validation.
- The active development checkout was changing during the initial audit; its
  transient compiler failures are not attributed to `main`.
- Remaining product gaps are tracked by STAB-02 through STAB-08. A green first
  gate does not mark those tasks complete.

The detailed acceptance text and ownership live in Lodestar. This document is a
portable roadmap, not a second task-state authority. Update evidence at each
reviewed checkpoint and retain the tested commit identity.

## Enrollment Identity Safety

The first STAB-02 change preserves the existing ignored key file at
`.mindleak/ackplane-node.key` (or the explicit `--key-path` /
`MINDLEAK_ACKPLANE_KEY_PATH`). No second copy of the private key belongs in a
dotenv file. A dotenv may hold non-secret configuration, but `register-me` does
not load dotenv files itself; its caller must supply the environment.

An unreadable or malformed key is an error, never permission to regenerate it.
If enrollment state exists but its key is gone, restore the original key from
protected backup. Activation only loads an existing key and verifies its public
fingerprint against the saved enrollment before connecting. Only a new request
with neither a key nor saved enrollment may create a key; concurrent creation
publishes one complete key without overwriting a winner.

Validation: all 18 enrollment CLI tests pass, including the two accidental
replacement cases first reproduced against the old implementation. Scoped
Clippy is warning-free. The full industrial gate passes 2,562 tests with three
existing ignored tests against a fresh disposable PostgreSQL instance.

This does not complete STAB-02: clean installation, activation-to-runtime
configuration, idempotent enrollment and the administrative approval surface
remain to be verified and completed.
