- **Added:** The Bridge now comes up alongside Ackplane in the supported
  `docker compose up` topology (ADR-0105 decision 3). `crates/ackplane-bridge/Dockerfile`
  packages it onto the same pinned toolchain/runtime pattern
  `crates/ackplane-server/Dockerfile` already uses, and `docker-compose.yml`
  gains a `bridge` service, depending only on `migrate` (Bridge is a direct
  Postgres client of the same database Ackplane migrates, never a gRPC client
  of `ackplane` itself), publishing its port to loopback only, and following
  the existing healthcheck/`restart: unless-stopped` conventions.
  `BridgeConfig::resolve` refuses any non-loopback listen address until a
  production authentication verifier exists (ADR-0094), so the process always
  binds `127.0.0.1` inside its container â€” but Docker's published-port
  mechanism forwards host traffic to a container's real network interface,
  never to that container's own loopback, so a port published straight from
  that bind is silently unreachable from the host even though the container
  reports healthy. `docker-entrypoint.sh` resolves this with a small in-container
  `socat` relay rather than loosening `BridgeConfig`'s own validation: the
  relay listens on every interface on a second port and forwards to Bridge's
  loopback bind, and the published port targets the relay instead.
