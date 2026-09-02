# Bounding the Postgres Connection Budget in Production

This is the connection-budget slice of the production deployment story
[ADR-0088](adr/0088-the-ackplane-runs-in-containers-the-planes-do-not.md)
names as still owed — not the whole story. `docker-compose.yml`'s
`max_connections=500` is a **development-topology** value, measured against
`cargo test --all`'s demand (the closed
`gaps.d/the-postgres-connection-ceiling-fails-the-suite-misleadingly.md`),
never a production tuning claim. This note is the production answer for
Ackplane's Postgres connection budget: how many connections
`ACKPLANE_DB_POOL_MAX_SIZE` should actually allow per process, for real
topologies, and what happens when that ceiling is reached.

## The mechanism this builds on

[ADR-0143](adr/0143-postgres-access-goes-through-one-bounded-pool-per-process.md)
gives every Ackplane process exactly one bounded `deadpool-postgres` pool
(`crates/ackplane-server/src/db_pool.rs`), capped at `ACKPLANE_DB_POOL_MAX_SIZE`
connections (default `SERVICE_POOL_MAX_SIZE = 16`) and bounding the wait for
one at `ACKPLANE_DB_POOL_TIMEOUT_MS` (default 5000ms). Every store's own
`connection()` helper checks a connection out through `db_pool::checkout`,
which now refuses **immediately** once as many callers are already queued as
the pool has slots — the identical typed `PoolExhausted` a caller joining
that queue would eventually get anyway, just without first paying the full
configured timeout. That is the explicit backpressure this note assumes: a
process under sustained overload fails fast and predictably rather than
letting its own wait queue, and every caller's tail latency, grow without
bound.

## Sizing `ACKPLANE_DB_POOL_MAX_SIZE` for a real topology

The budget to divide is Postgres's own `max_connections`, minus headroom
every deployment needs regardless of Ackplane:

```
available = max_connections
          - superuser_reserved_connections   (Postgres default: 3)
          - operational headroom             (psql/monitoring/backup tooling;
                                                recommend >= 5)
          - one-shot job headroom            (migrate / migrate-test; each
                                                needs exactly 1 connection for
                                                the short window it runs in)
```

Only two Ackplane binaries hold a long-running pool for their whole process
lifetime — `ackplane` (`crates/ackplane-server`, the gRPC service) and
`ackplane-bridge` (the HTTP service); `migrate` and `tls-init` run to
completion and exit. Divide the remaining budget across every long-running
replica of both:

```
ACKPLANE_DB_POOL_MAX_SIZE = floor(available / (replicas_of_ackplane + replicas_of_ackplane_bridge))
```

Both binaries read the **same** environment variable, so this is one number
per deployment, not one per binary — set it identically wherever either
binary runs, sized for the total replica count across both.

### Worked example

A single-primary managed Postgres instance with `max_connections=100` (a
common managed-Postgres default), running 2 replicas of `ackplane` and 2 of
`ackplane-bridge`:

```
available = 100 - 3 (superuser reserved) - 5 (operational headroom) - 1 (migrate) = 91
ACKPLANE_DB_POOL_MAX_SIZE = floor(91 / (2 + 2)) = 22
```

Set `ACKPLANE_DB_POOL_MAX_SIZE=22` for every replica of both binaries. Total
worst-case demand is `4 * 22 = 88`, comfortably under the 91 available, with
the remaining 3 connections of slack absorbed by the fact that not every
replica hits its own ceiling at the same instant.

A smaller single-instance deployment (1 `ackplane`, 1 `ackplane-bridge`,
`max_connections=100`) can afford the built-in default unchanged:
`available = 91`, `floor(91 / 2) = 45`, well above `SERVICE_POOL_MAX_SIZE`'s
default of 16 — the default is already conservative here and does not need
raising.

## Why a slightly generous cap is safe, and an exact one is not required

Because `checkout` now fails fast under real contention, getting this number
*approximately* right is enough — demand beyond the configured cap produces
an immediate, typed `PoolExhausted` (`SERVICE_UNAVAILABLE` in Bridge,
`Status::unavailable` over gRPC) that a caller can retry, rather than a raw
Postgres `SqlState(E53300)` that can starve *every* process sharing that
Postgres instance, including ones this deployment does not control. Getting
it exactly right only matters for latency under load, not for correctness:
prefer sizing conservatively (rounding down, keeping real headroom) over
raising the number until tests stop complaining, which is the outcome this
whole exercise exists to avoid — see the gap fragment linked above for what
that looked like the one time it happened by accident.

## What this note does not cover

A hard per-topology cap validated under an actual multi-process soak test,
and horizontal-scaling guidance beyond connection budgeting (replica count
itself, load balancing, managed-Postgres failover) remain open, tracked
separately. This note is scoped to the connection budget alone.
