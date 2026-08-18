- **The `ackplane` Compose service now matches what the binary actually does.**
  Its topology still described the pre-ADR-0083 build: no gRPC service, so the
  container started, printed its banner, and exited 0 — hence `restart: "no"`
  and no healthcheck. `NodeSyncService` has served real traffic since PR #439,
  with connection authentication since PR #501, so the container is long-running
  now and the old comment was actively misleading about what a failed or
  restarting `ackplane` container means. `restart: "no"` is now
  `unless-stopped`, and a `healthcheck:` proves the gRPC port accepts
  connections with a bare TCP check (`nc -z 127.0.0.1 8443`) rather than a full
  `grpc.health.v1.Health` probe, which the service does not implement yet — the
  runtime image gains `netcat-openbsd` (the smallest tool for that check) as
  its one dependency beyond the pinned Debian base. Compose's own dependency
  ordering (clause 5) can now use this healthcheck instead of assuming the
  container is ready the instant it starts.
