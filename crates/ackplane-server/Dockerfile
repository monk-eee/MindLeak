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
# declared `rust-version = "1.75"` floor in the root Cargo.toml.
FROM rust:1.75-slim-bookworm@sha256:70c2a016184099262fd7cee46f3d35fec3568c45c62f87e37f7f665f766b1f74 AS builder

WORKDIR /build
COPY . .
RUN cargo build --release --locked -p ackplane-server --bin ackplane-server --bin ackplane-migrate

# Pinned by tag and digest (ADR-0088 clause 4). Debian slim rather than
# distroless/scratch: the binaries link the system's TLS and libc, and this
# keeps the runtime image's provenance auditable against the same base the
# builder stage used.
FROM debian:bookworm-slim@sha256:abd67ffcfa541b485a3dff59865ab629aa048a6c613e639d36e7456b0b229241 AS runtime

# Runs as an unprivileged, non-root user (ADR-0088 clause 4). No secrets or
# credentials are baked into the image; ACKPLANE_DATABASE_URL and friends are
# supplied at container start by the Compose topology's environment.
RUN groupadd --system --gid 10001 ackplane \
    && useradd --system --uid 10001 --gid ackplane --no-create-home --shell /usr/sbin/nologin ackplane

COPY --from=builder /build/target/release/ackplane-server /usr/local/bin/ackplane-server
COPY --from=builder /build/target/release/ackplane-migrate /usr/local/bin/ackplane-migrate

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
