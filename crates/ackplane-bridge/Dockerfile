# Bridge image (ADR-0088 clauses 1, 4 style; ADR-0105 decision 3): a multi-stage
# build from the same pinned Rust toolchain onto a minimal, non-root runtime,
# mirroring crates/ackplane-server/Dockerfile so there is one provenance
# pattern to audit for the whole Industrial profile, not two. Bridge has no
# migration binary of its own (`crates/ackplane-server`'s `migrate` already
# owns the schema) -- this image builds and runs exactly one binary.
#
# Build (from the repository root):
#   docker build -f crates/ackplane-bridge/Dockerfile \
#     --build-arg ACKPLANE_VERSION=$(git describe --tags --always) \
#     --build-arg ACKPLANE_COMMIT=$(git rev-parse HEAD) \
#     -t ackplane-bridge .

# Pinned by tag and digest (ADR-0088 clause 4), identical to
# crates/ackplane-server/Dockerfile's builder stage: same workspace, same
# `rust-version` floor, one image to audit for both.
FROM rust:1.88-slim-bookworm@sha256:38bc5a86d998772d4aec2348656ed21438d20fcdce2795b56ca434cf21430d89 AS builder

WORKDIR /build
COPY . .
RUN cargo build --release --locked -p ackplane-bridge --bin ackplane-bridge

# Pinned by tag and digest (ADR-0088 clause 4), the same runtime base
# crates/ackplane-server/Dockerfile uses -- one provenance to audit, not a
# second base image for the same topology.
FROM debian:bookworm-slim@sha256:abd67ffcfa541b485a3dff59865ab629aa048a6c613e639d36e7456b0b229241 AS runtime

# Compose's healthcheck needs a real HTTP request against Bridge's `GET /`
# (the Fleet page -- static HTML, no store dependency, a genuine registered
# route rather than an invented one) since, unlike `ackplane`'s bare gRPC
# port, Bridge is an ordinary axum HTTP server with a trivial liveness route
# already on it. curl is the smallest tool that does that.
#
# socat relays the container's real network interface to Bridge's own
# loopback bind, since `BridgeConfig::resolve` refuses any other listen
# address (ADR-0094) and a loopback bind is otherwise unreachable through
# Docker's published-port mechanism -- see docker-entrypoint.sh for the full
# reasoning. It adds no privilege and no capability: the relay listens on an
# unprivileged port under the same non-root user Bridge itself already runs
# as.
#
# pg_dump/pg_restore (ADR-0119 decision 4 / ADR-0128's platform Snapshot,
# `ackplane_server::snapshot_provider`) are deliberately NOT bundled here yet:
# `SnapshotProviderConfig::resolve` reports the capability `unavailable`
# unless an operator sets `ACKPLANE_SNAPSHOT_DIR`, which this Compose profile
# does not (no snapshot storage volume is part of this topology yet), and
# Debian bookworm's default `postgresql-client` is major version 15 against
# this stack's pinned Postgres 16 server -- pulling in the PGDG apt
# repository to match versions is its own follow-on once Snapshot is wired
# into Compose, not a silent version mismatch shipped alongside a feature
# nothing here turns on.
RUN apt-get update \
    && apt-get install --no-install-recommends -y curl socat \
    && rm -rf /var/lib/apt/lists/*

# Runs as an unprivileged, non-root user (ADR-0088 clause 4), distinct from
# `ackplane-server`'s so the two images never share a uid by accident even
# though they never run in the same container. No secrets or credentials are
# baked into the image; ACKPLANE_DATABASE_URL and the salt volume are
# supplied at container start by the Compose topology.
RUN groupadd --system --gid 10002 bridge \
    && useradd --system --uid 10002 --gid bridge --no-create-home --shell /usr/sbin/nologin bridge

# Pre-created and owned by the unprivileged user before Compose mounts the
# `ackplane-bridge-salt` named volume here: Docker initializes a fresh named
# volume's ownership from whatever the image already has at that mount point,
# so `load_or_generate_salt` can write its first salt as `bridge`, not root.
RUN mkdir -p /var/lib/ackplane-bridge && chown bridge:bridge /var/lib/ackplane-bridge

COPY --from=builder /build/target/release/ackplane-bridge /usr/local/bin/ackplane-bridge
COPY --chmod=755 crates/ackplane-bridge/docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh

# Carries the version and commit this image was built from (ADR-0088
# clause 4) as inspectable OCI labels, independent of the crate version baked
# in at compile time.
ARG ACKPLANE_VERSION=unknown
ARG ACKPLANE_COMMIT=unknown
LABEL org.opencontainers.image.version=${ACKPLANE_VERSION} \
      org.opencontainers.image.revision=${ACKPLANE_COMMIT} \
      org.opencontainers.image.source="https://github.com/monk-eee/MindLeak"

USER bridge
ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
