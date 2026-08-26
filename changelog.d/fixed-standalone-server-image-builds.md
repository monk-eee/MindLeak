### Fixed

- The standalone Ackplane server image builds again.

  `docker compose build ackplane` — the documented bring-up for the standalone
  federation service (ADR-0082, ADR-0088) — aborted with exit 101 before
  compiling anything:

  ```
  error: rustc 1.85.1 is not supported by the following packages:
    time@0.3.55 requires rustc 1.88.0
    time-core@0.1.9 requires rustc 1.88.0
  ```

  The Dockerfile pinned `rust:1.85-slim-bookworm` to match the workspace's
  declared `rust-version = "1.85"`, but the locked dependency graph had moved
  past that floor, so the declared MSRV was already false. Both are now 1.88,
  the version the lockfile actually requires.

  Nothing caught it because every CI job installs
  `dtolnay/rust-toolchain@stable` and none of them built the image, leaving the
  container as the only place the floor was ever exercised. A new
  `Ackplane server image` CI job now builds it on every run.
