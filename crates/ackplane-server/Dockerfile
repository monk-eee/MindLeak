# Ackplane server image (ADR-0088 clauses 1, 4): a multi-stage build from a
# pinned Rust toolchain onto a minimal, non-root runtime. Build only what
# Compose needs from this image — `ackplane-server` and `ackplane-migrate` —
# so neither `mindleak-mcp` nor `lodestar-mcp` is pulled into a container image
# by accident (ADR-0088 clause 2 keeps the local planes container-free).
#
# Build (from the repository root):
#   docker build -f crates/ackplane-server/Dockerfile \
#     --build-arg ACKPLANE_VERSION=$(git describe --tags --always) \
#     --build-arg ACKPLANE_COMMIT=$(git rev-parse HEAD) \
#     -t ackplane-server .

# Pinned by tag and digest (ADR-0088 clause 4). Matches the workspace's
# declared `rust-version` floor in the root Cargo.toml. That floor is 1.88
# because the locked dependency graph requires it - `time` and `time-core`
# both declare `rust-version = "1.88"` - not because this crate uses anything
# newer than Edition 2024. Raise both together: this image is the only place
# the declared floor is actually compiled against, since CI installs
# `dtolnay/rust-toolchain@stable`.
FROM rust:1.88-slim-bookworm@sha256:38bc5a86d998772d4aec2348656ed21438d20fcdce2795b56ca434cf21430d89 AS builder

WORKDIR /build
COPY . .
RUN cargo build --release --locked -p ackplane-server --bin ackplane-server --bin migrate

# Pinned by tag and digest (ADR-0088 clause 4). Debian slim rather than
# distroless/scratch: the binaries link the system's TLS and libc, and this
# keeps the runtime image's provenance auditable against the same base the
# builder stage used.
FROM debian:bookworm-slim@sha256:abd67ffcfa541b485a3dff59865ab629aa048a6c613e639d36e7456b0b229241 AS runtime

# The one runtime package this image carries beyond the pinned base (ADR-0088
# clause 4 still holds: minimal, not zero). Compose's healthcheck needs
# something that can prove the gRPC port accepts connections; netcat-openbsd is
# the smallest tool that does a bare TCP connect without pulling in curl's TLS
# stack or a shell the entrypoint does not otherwise need.
RUN apt-get update \
    && apt-get install --no-install-recommends -y netcat-openbsd \
    && rm -rf /var/lib/apt/lists/*

# Runs as an unprivileged, non-root user (ADR-0088 clause 4). No secrets or
# credentials are baked into the image; ACKPLANE_DATABASE_URL and friends are
# supplied at container start by the Compose topology's environment.
RUN groupadd --system --gid 10001 ackplane \
    && useradd --system --uid 10001 --gid ackplane --no-create-home --shell /usr/sbin/nologin ackplane

COPY --from=builder /build/target/release/ackplane-server /usr/local/bin/ackplane-server
COPY --from=builder /build/target/release/migrate /usr/local/bin/ackplane-migrate

# Carries the version and commit this image was built from (ADR-0088
# clause 4) as inspectable OCI labels (`docker inspect`), independent of
# `ServerConfig::banner()`'s own `CARGO_PKG_VERSION`, which is the crate
# version baked in at compile time rather than this specific build's inputs.
ARG ACKPLANE_VERSION=unknown
ARG ACKPLANE_COMMIT=unknown
LABEL org.opencontainers.image.version=${ACKPLANE_VERSION} \
      org.opencontainers.image.revision=${ACKPLANE_COMMIT} \
      org.opencontainers.image.source="https://github.com/monk-eee/MindLeak"

USER ackplane
ENTRYPOINT ["/usr/local/bin/ackplane-server"]
