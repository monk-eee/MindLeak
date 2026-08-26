- **The Compose stack cannot satisfy the server's own TLS bind guard, so the
  standalone federation service has no working documented bring-up — OPEN,
  found 2026-08-26.** With the image build repaired, `docker compose up -d`
  brings `ackplane-ackplane-1` up into a permanent crash-restart loop. Every
  start emits the same refusal and exits:

  ```
  ackplane-server: ACKPLANE_LISTEN is not loopback but neither
  ACKPLANE_TLS_CERTIFICATE_PATH nor ACKPLANE_TLS_KEY_PATH is set;
  ADR-0083 clause 8 requires TLS outside loopback
  ```

  `docker-compose.yml`'s `ackplane` service sets `ACKPLANE_LISTEN: 0.0.0.0:8443`,
  and the server refuses any non-loopback bind without a TLS certificate and key.

  Both sides are individually right, which is what makes this a design gap
  rather than a typo. A container must bind `0.0.0.0` to be reachable at all:
  Docker's published port forwards to the container's bridge address, so a
  process bound to the container's own loopback is unreachable from the host.
  The Compose file says exactly this — "the container listens on every
  interface internally, but the published port below is what actually decides
  reachability from the host, and that stays 127.0.0.1". Meanwhile ADR-0083
  clause 8 is a deliberate security invariant, and the server cannot observe
  that Docker has confined the published port to loopback; from inside the
  container, `0.0.0.0` is `0.0.0.0`.

  Impact: the stack starts, `migrate` succeeds, and the server itself never
  stays up, so nothing can enrol or sync against a Compose deployment. The
  end-to-end enrolment ceremony verified on 2026-08-24 ran against a
  hand-started server binary rather than the Compose topology, which is why
  this was invisible then.

  Left for later deliberately: the candidate resolutions are a real decision,
  not a patch. Either (a) the Compose stack generates and mounts a self-signed
  certificate, honouring clause 8 as written and moving the dev topology closer
  to a real deployment, at the cost of every client needing to trust it; or
  (b) the server gains an explicit, narrowly-named acknowledgement that
  something else terminates TLS or confines the port, which is a security
  escape hatch and should not be added on a hunch; or (c) clause 8 is amended
  to distinguish a bind address from a reachable interface. That wants an ADR,
  not a same-session guess.

  The separate defect that hid this one — the image not building at all under
  its pinned Rust 1.85 — is fixed, and a CI job now builds the image so the
  standalone deployment path cannot rot invisibly again.
