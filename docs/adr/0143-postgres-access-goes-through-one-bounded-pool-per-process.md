# ADR-0143: Postgres access goes through one bounded pool per process

- Status: Accepted
- Date: 2026-08-29
- Deciders: MindLeak maintainers
- Accepted: 2026-08-29 by the repository owner, authorized directly in
  session — attributed human adoption after review.
- Depends on: [ADR-0086](0086-postgresql-is-the-ackplane-ledger-and-arbiter.md)
  (PostgreSQL is the Ackplane ledger and arbiter)
- Related:
  [gaps.d/the-postgres-connection-ceiling-fails-the-suite-misleadingly.md](../../gaps.d/the-postgres-connection-ceiling-fails-the-suite-misleadingly.md)

## Context

Every Ackplane store opens and keeps exactly one dedicated `tokio_postgres::Client`
for its entire lifetime. `ClaimStore`, to pick one representative example:

```rust
pub struct ClaimStore {
    client: Client,
}

impl ClaimStore {
    pub async fn connect(database_url: &str) -> Result<Self, tokio_postgres::Error> {
        let (mut client, connection) = tokio_postgres::connect(database_url, NoTls).await?;
        tokio::spawn(async move { /* drive the connection */ });
        crate::migration_lock::migrate_locked(&mut client, /* ... */).await?;
        Ok(Self { client })
    }
}
```

Twenty-three store types repeat this pattern verbatim across `ackplane-server`
and `ackplane-bridge` (`git grep -c "fn connect(" -- crates/ackplane-server/src
crates/ackplane-bridge/src` — 23), each opening its own raw connection with no
shared pool, no cap, and no backpressure. This is not a hypothetical concern;
it already produced a real, misdiagnosed outage
([gaps.d/the-postgres-connection-ceiling-fails-the-suite-misleadingly.md](../../gaps.d/the-postgres-connection-ceiling-fails-the-suite-misleadingly.md)):

- A running `ackplane-bridge` process holds 17 of these dedicated connections
  for its entire life (one per store its `main.rs` constructs); `ackplane-server`
  holds 11. Both counts are small and deterministic *per running process* — the
  gap is not there.
- `cargo test --all` is where demand is genuinely unbounded: every DB-gated
  test constructs its own store instances, each opening its own raw connection,
  with as many test binaries running concurrently as the test harness allows.
  One crate's DB-gated tests alone peaked at 73 concurrent connections;
  `cargo test --all` running several such crates at once exhausted Postgres's
  un-tuned default of 100 (`SqlState(E53300)` "sorry, too many clients already").
  The docker-compose ceiling was raised to `max_connections=500` as an
  immediate development-topology fix, but that only moves the exhaustion
  point — it does not bound demand. The gap fragment names the actual remedy
  explicitly: "the real remedy is bounding connections in code, and that is a
  decision rather than a patch... deliberately not attempted."
- The failure mode is actively misleading, independent of where the ceiling
  sits: the panicking test names never mention connections, only the panic
  body does, so it reads exactly like several unrelated subsystems broke at
  once, and an agent validating an unrelated diff goes looking for a cause
  inside it. This already produced one wrong diagnosis on this board (a
  now-deleted fragment blamed test isolation instead), which would have meant
  building tenant isolation that fixed nothing.

The reason nobody has just wired in a pooling crate is that several tests
*depend on* today's one-store-one-connection model. `delegation_store::
authorization::tests` polls `pg_stat_activity` for a session holding a
`SELECT ... FOR UPDATE` lock while a second, distinct connection blocks trying
to acquire the same row lock — that test needs two connections that are
identifiably separate for the duration of a held transaction, not two
requests that might be served by the same pooled connection out of a size-1
pool, and not a transaction that a naive wrapper silently returns to the pool
between statements. A pooling change that is not explicit about which
connection a held transaction owns for its duration will break this test's
premise, or worse, pass it by accident while quietly changing what it proves.

## Decision

**Each process gets exactly one bounded `deadpool-postgres` pool. Stores stop
holding a private `Client` for their lifetime and instead check out a pooled
connection per call (or per held transaction); nothing changes for a caller
that never held a transaction across an `.await` point it didn't need to.**

1. **One pool per process, not one pool per store.** Each binary's `main.rs`
   (`ackplane-server`, `ackplane-bridge`, `ackplane-mcp`) and each integration
   test's shared fixture builds exactly one `deadpool_postgres::Pool` from the
   process's `database_url`, sized by `ACKPLANE_DB_POOL_MAX_SIZE` (default:
   16 for a service binary, 8 for a test-fixture process — chosen so that
   today's `cargo test --all` demand, capped per test *process* rather than
   per test *store*, stays well under a development ceiling of 500 even at
   full parallelism, and so that a runaway single process cannot again exhaust
   a shared Postgres instance no matter how many stores or tasks it starts).
   Every store constructed in that process is handed a clone of the same pool
   handle (`deadpool_postgres::Pool` is cheaply `Clone`, an `Arc` internally) —
   this is dependency injection, matching this repository's existing
   `&GraphStore`-by-reference convention, not a `static`.

2. **`deadpool-postgres` is the chosen crate, not a hand-rolled pool or `bb8`.**
   It is purpose-built for `tokio-postgres` (the workspace's existing
   dependency, `Cargo.toml` line 52), so store code keeps using
   `tokio_postgres::Row`/`Client` method signatures unchanged — a checked-out
   `deadpool_postgres::Object` derefs to `tokio_postgres::Client`. `bb8` is
   more generic and asks for an adapter crate (`bb8-postgres`) with less
   activity; a hand-rolled semaphore-gated `Vec<Client>` freelist would
   duplicate the recycling, health-check-on-checkout, and idle-timeout
   behaviour `deadpool-postgres` already has tested, for no benefit this
   product needs. This is the "prefer ecosystem tools over manual changes"
   default, not a special case.

3. **Store construction keeps its migration-at-startup behaviour, using one
   checked-out connection.** `Store::connect(pool: &PgPool)` (replacing
   `Store::connect(database_url: &str)`) checks out one connection, runs that
   store's `migrate_locked` against it exactly as today, and returns it to the
   pool before completing construction. `migrate_locked`'s own advisory-lock
   serialization (`crate::migration_lock`) is unaffected: it already tolerates
   multiple callers racing to migrate the same key, and a pooled connection is
   no different from today's dedicated one for the duration of that one call.

4. **A held transaction owns one checked-out connection for its whole
   duration, never released mid-transaction.** Any store method that opens a
   `tokio_postgres::Transaction` (or otherwise depends on session-scoped state
   — an advisory lock, a `FOR UPDATE` row lock held across more than one
   statement) must check out its connection once at the top of that method and
   hold the same `deadpool_postgres::Object` for every statement in it,
   exactly as `delegation_store::authorization`'s lock-contention test
   requires: the connection identity a `FOR UPDATE` waiter blocks against must
   stay stable for as long as the holder's transaction is open. This decision
   exists specifically so that test keeps proving what it already proves — no
   store may be migrated to the pool by wrapping calls in a way that
   interleaves other pool traffic through the same logical operation.

5. **Pool exhaustion is a typed, fail-fast refusal, not an unbounded wait.**
   `deadpool_postgres::Pool::get()` is called with a bounded timeout
   (`ACKPLANE_DB_POOL_TIMEOUT_MS`, default 5000); a timeout maps to a new
   `StoreError::PoolExhausted` variant on every store's existing error enum
   (each already has a `#[from] tokio_postgres::Error` arm to sit beside),
   surfaced to callers the same way a connection failure is today. A process
   that cannot get a connection within the timeout reports why, rather than
   hanging a request indefinitely or silently retrying forever.

6. **Migration is per-store, not one all-at-once rewrite, and each store's
   commit is independently right-shaped.** Twenty-three stores share the exact
   same mechanical shape (swap the held `client: Client` field for
   `pool: PgPool`, swap every `self.client.query(...)` for
   `self.pool.get().await?.query(...)`, swap held-transaction methods per
   decision 4), so this is deliberately scoped as a sequence of small,
   individually complete PRs — one store (or one tightly related cluster of
   stores in the same module) per commit — rather than a single sprawling
   diff that is hard to review and impossible to bisect if one store's
   transaction handling regresses. There is no interim "some stores pooled,
   some not" shim to build: a store that has not yet migrated keeps its
   current `connect(database_url: &str)` exactly as it is today (a correct,
   already-shipped implementation, not a deprecated one) until its own turn
   in the sequence, at which point it moves to `connect(pool: &PgPool)` in
   full, in one commit, with its own tests updated in the same commit. No
   store may sit half-migrated.

7. **Test fixtures adopt the same pool, closing the actual unbounded demand.**
   Integration test modules that construct multiple stores against the same
   `database_url` (most of them, per `crates/ackplane-bridge/tests/
   administration_export_integration.rs`'s and similar files' repeated
   `Store::connect(database_url)` calls) build one pool per test binary and
   pass it to every store they construct, instead of one dedicated connection
   per store per test. This directly closes the demand side of the ceiling
   gap: a test binary's total connection footprint becomes bounded by its
   pool's `max_size`, not by how many stores or tests it happens to run
   concurrently. A test that connects directly with raw `tokio_postgres::
   connect` to *observe* server-side state out of band (the `pg_stat_activity`
   monitor connection in the lock-contention test, or the direct verification
   connections in the export/purge integration tests) is unaffected — those
   are deliberately not pool members, because they exist to independently
   confirm state the pool-backed connections created.

## Consequences

- The docker-compose `max_connections=500` development setting from the prior
  fix stays as-is; this ADR does not change it, and does not claim it as a
  production tuning value (the gap fragment was explicit that it is not one).
  It becomes generous headroom instead of the only defense.
- An implementing agent starts with one store (recommend `ClaimStore`, since
  it is the one the lock-contention test already exercises, making it the
  hardest and most informative case to migrate first) to prove out the
  `PgPool` type, the `connect(pool: &PgPool)` shape, and the
  `PoolExhausted` error arm, before repeating the same shape across the
  remaining 22.
- `PgPool` itself is a small new shared type — it belongs beside
  `migration_lock` in `ackplane-server`'s crate root (both are cross-store
  infrastructure, not any one store's concern), re-exported for
  `ackplane-bridge`'s `main.rs` to construct once and thread through its own
  store constructions.
- `Cargo.toml` gains `deadpool-postgres` as a workspace dependency (async,
  `tokio-postgres`-native, no `native-tls`/`openssl` transitively beyond what
  `tokio-postgres` already pulls for `NoTls`).
- No wire contract, RPC, or test assertion about *what* a store does changes;
  only how it obtains its connection. The lock-contention test in particular
  is expected to keep passing unmodified once `ClaimStore`'s delegated
  authorization path (or whichever store it targets) is migrated correctly
  under decision 4 — if it does not, that is a signal the migration broke the
  held-transaction invariant, not that the test needs to change.
- This does not touch `mindleak-core`/`lodestar-core`'s SQLite storage
  (bundled, single-process, no equivalent connection ceiling exists there);
  it is scoped to the Postgres-backed Ackplane stores named in ADR-0086.

## Rejected alternatives

- **Cap `--test-threads` instead.** Rejected in the gap fragment itself and
  reaffirmed here: it slows an already slow suite for every contributor and
  hides the coupling instead of removing it, and does nothing for a
  production process that legitimately wants to open many stores.
- **Raise `max_connections` further instead of bounding demand.** Only moves
  the exhaustion point; a large enough parallel run still exhausts whatever
  ceiling is set, and the failure keeps misleading the same way in the
  meantime.
- **A single process-wide `Mutex<Client>`** instead of a pool. Would remove
  concurrency entirely — the lock-contention test specifically needs two
  connections live at once, so a single shared connection cannot express the
  scenario the whole design has to preserve, and would serialize otherwise
  independent stores' otherwise-independent queries for no reason.
- **Migrate all 23 stores in one PR.** Rejected under this repository's own
  discipline: a right-shaped change costs what it costs, but a single
  sprawling diff across every store is exactly the shape that is hard to
  review and impossible to bisect if one store's transaction handling
  regresses; decision 6 scopes it as a sequence of individually correct
  commits instead.
- **`bb8`/hand-rolled pool.** See decision 2 — `deadpool-postgres` is the
  ecosystem-tool default for this exact dependency pair.
