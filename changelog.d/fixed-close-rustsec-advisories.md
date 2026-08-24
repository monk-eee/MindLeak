### Fixed

- Closed three dated RustSec advisories by updating transitive dependency
  pins that were unintentionally exact-pinned (`= "x.y.z"`) rather than
  using a normal semver range, which had silently prevented `cargo update`
  from ever picking up a patched release:
  - `h2` 0.4.15 -> 0.4.18 (RUSTSEC-2026-0258: unbounded empty DATA frames).
  - `time` 0.3.36 -> 0.3.55 (RUSTSEC-2026-0009: denial of service via stack
    exhaustion, medium severity).
  - `idna` 0.5.0 -> 1.1.0, via `url` 2.5.0 -> 2.5.8 (RUSTSEC-2024-0421:
    accepts Punycode labels that decode to no non-ASCII characters).
