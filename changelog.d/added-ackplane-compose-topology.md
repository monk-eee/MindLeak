- **Added:** the Ackplane Compose topology (ADR-0088): `docker-compose.yml`
  brings up `postgres`, a one-shot `migrate` service, and the `ackplane`
  service, in that dependency order, using healthchecks rather than sleeps.
  Images are pinned by tag and digest; the server image
  (`crates/ackplane-server/Dockerfile`) is a multi-stage build from a pinned
  Rust toolchain onto a minimal, non-root Debian runtime, and carries its
  version and commit as OCI labels. `crates/ackplane-server/src/bin/migrate.rs`
  is a new thin binary that opens the ledger and projection connections
  (each already applies its own idempotent migration on connect) and exits.
  `node scripts/ackplane-compose.mjs` wraps `up`, `down`, `backup`, `restore`,
  and `reset` so a developer never types anything but `docker compose` or
  `node` (clause 3); `reset` refuses to delete the named
  `ackplane-postgres-data` volume without an explicit `--confirm` (clause 7).
  Compose reports `ACKPLANE_DURABILITY=single_node`, matching what one
  PostgreSQL container can actually promise (clause 6). The repository-local
  planes remain container-free; nothing in this change touches
  `mindleak-mcp`, `lodestar-mcp`, or their tests (clause 2).
