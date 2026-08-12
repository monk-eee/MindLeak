# ADR-0091: Ackplane builds and tests without a database

- Status: Accepted
- Date: 2026-08-13
- Deciders: MindLeak maintainers
- Accepted: 2026-08-13 by the repository owner — attributed human adoption after
  review.
- Depends on: [ADR-0082](0082-ackplane-is-a-standalone-federation-service.md)
  (Ackplane is a separately deployable service),
  [ADR-0086](0086-postgresql-is-the-ackplane-ledger-and-arbiter.md)
  (PostgreSQL is the ledger and arbiter),
  [ADR-0088](0088-the-ackplane-runs-in-containers-the-planes-do-not.md)
  (the planes need no container runtime)
- Related: [ADR-0083](0083-grpc-is-the-ackplane-node-protocol.md) (the
  deduplication key), [ADR-0087](0087-the-ackplane-graph-is-a-projection-not-an-authority.md)
  (projections read committed records)

## Context

Three accepted decisions meet at a point none of them resolves. ADR-0086 makes
PostgreSQL the sole durable write authority and puts idempotency in database
constraints rather than application memory. ADR-0088 clause 2 requires
`cargo build`, `cargo test`, and the extension suite to run on a machine with no
Docker, no PostgreSQL, and no network. ADR-0082 makes Ackplane a separately
deployable service rather than a mode of either plane.

Nobody can execute the ledger without deciding how those three hold together at
once, and two agents reached that same wall independently: the work was parked
with a question rather than guessed at, which is the behaviour this repository
asks for and the reason this decision exists rather than an improvisation.

The wall has a specific shape. There is nowhere to put the ledger:
`crates/ackplane-core` describes itself as the repository side of the boundary,
and the workspace holds no server crate. Every dependency is pinned in the root
`[workspace.dependencies]`, so a database client is a shared decision rather than
a local one. And the tests that ADR-0086 clauses 5 and 6 actually care about —
the duplicate retry and the conflicting retry — are precisely the ones that look
like they need a database.

## Decision

1. **The ledger lives in a new `crates/ackplane-server`, and that crate is
   ADR-0082's deliverable.** `ackplane-core` stays the repository side of the
   boundary and does not gain a database dependency. The server crate must exist
   before the ledger can be built, so the task that creates it gates the ledger
   task rather than the other way round.

2. **The append rule is a pure decision function, and SQL enforces it.** A
   function maps the deduplication key, the envelope digest, the stored receipt
   if any, and the current stream head to one of `append`, `duplicate`, or
   `conflict`. It holds no connection and performs no I/O, so both branches
   ADR-0086 clause 5 names are covered by ordinary unit tests on a bare machine.
   The database then enforces the same rule with a unique constraint and the
   locked stream head, because an application that merely agrees with the
   database is not an authority (ADR-0086 clauses 1 and 5).

3. **Database-dependent tests are `#[ignore]` and opt in through
   `ACKPLANE_DATABASE_URL`.** Plain `cargo test` passes with no Docker, no
   PostgreSQL, and no network, satisfying ADR-0088 clause 2. This is the pattern
   the repository already uses for the live model round-trip, and it is chosen
   for the same reason: an external dependency that cannot be assumed present
   must not be able to fail a build that never asked for it.

4. **The client is `tokio-postgres`, and migrations are numbered SQL files
   applied by the one-shot migrate service.** No query macro validates against a
   live database at compile time, and no ORM owns the schema. Both would move
   authority out of the database that ADR-0086 clause 6 puts it in, and the first
   would make `cargo build` depend on either a running server or a generated
   cache that can silently go stale. Applied migrations are recorded in a
   `schema_migrations` table so the migrate service is idempotent.

5. **A PostgreSQL-backed CI job is additive and sequenced, never a
   replacement.** It runs the ignored tests against a service container, and it
   is added only after the job proving the local planes need no container
   runtime has landed. That job is the guarantee; this one is coverage. If only
   one can exist, it is the guarantee.

## Consequences

- The SQL enforcement of clause 5 is unexercised on a developer machine, and is
  proved only by the additive CI job. The pure decision function narrows that
  gap but does not close it: a constraint that was never applied to a real
  database has not been tested, and this decision says so rather than implying
  otherwise.
- `crates/ackplane-server` is a new workspace member, so the root `Cargo.toml`
  changes. That file is bound to
  `goal:adr-0030-unique-per-process-agent-identity`, which has nothing to do with
  the workspace manifest, so every Ackplane crate task drifts at completion and
  no agent-reachable verb can rebind it (see
  `gaps.d/a-publication-can-report-an-unbound-file-no-agent-can-bind.md`). This
  is a standing tax on the whole build-out, recorded here so it is not
  rediscovered per task.
- `cargo build --locked --offline` is already failing in this workspace before
  any of this lands. Adding the first heavyweight dependency will be blamed for
  it unless that is fixed or understood first.
- The board's ordering is currently inverted and must be corrected for this to
  be actionable: the ledger task is open while the ADR-0082 task that would
  create the server crate sits `blocked` with no gate, which resolves only by
  `reopen`. Two of those blocked tasks were retired by this session on the
  reading that ADR-0082's coordination-mode slice had landed; that slice is the
  repository side, and the service itself was never built.

## Rejected alternatives

**Put the ledger in `ackplane-core`.** Rejected because that crate is the
repository side of the boundary by its own documentation and by ADR-0082 clause
1. A database client there would put server state inside the node.

**`sqlx` with compile-time query checking.** Rejected because it validates
queries against a live database at build time, or against a committed offline
cache that drifts from the schema without failing. Either outcome makes
`cargo build` depend on the database that ADR-0088 clause 2 says it must not.

**Testcontainers, or any harness that starts Docker from `cargo test`.**
Rejected because it makes the container runtime a build dependency of the whole
workspace, which is the exact property ADR-0088 clause 2 exists to prevent.

**An ORM or migration framework owning the schema.** Rejected because ADR-0086
clause 6 puts authority in the database — immutability and idempotency are
constraints and roles, not application conventions — and an ORM's schema
authority competes with that.

**Test the duplicate and conflict paths only against a real database.**
Rejected because it leaves the two rules ADR-0086 cares most about with no
coverage at all on a machine without PostgreSQL, which is every developer
machine this repository currently supports.
