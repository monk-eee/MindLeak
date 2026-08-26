### Fixed

- `docker compose up` now brings the standalone Ackplane server up healthy and
  serving TLS. It previously could not come up at all.

  The Compose topology sets `ACKPLANE_LISTEN=0.0.0.0:8443` — a container must
  bind every interface to be reachable through a published port at all — while
  the server refused any non-loopback bind without TLS material, so the
  container crash-restarted forever. The standalone federation service
  (ADR-0082) had no working documented bring-up.

  Compose now generates a self-signed development certificate into a named
  volume and serves it, so the default path satisfies ADR-0083 clause 8
  honestly. Verified on the wire: TLS 1.3, ALPN `h2`, `CN=ackplane` with
  `localhost` and `127.0.0.1` in the SAN, and a cleartext probe refused.

### Added

- `ACKPLANE_LISTEN_CONFINED_BY` lets a deployment that terminates TLS elsewhere
  — a service mesh, a load balancer, a container's published port — declare
  what confines its plaintext listener (ADR-0132).

  It takes a non-empty description rather than a boolean, because an operator
  who cannot name the mechanism does not have one. It permits exactly one thing,
  a non-loopback bind with no TLS material; it is ignored when a certificate is
  present, and it changes nothing about authentication, tenant binding,
  signatures, or evidence trust. The startup banner always names it:
  `serving PLAINTEXT outside loopback, confined by <what>`.
