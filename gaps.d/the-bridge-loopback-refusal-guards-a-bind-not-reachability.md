- **The Bridge's non-loopback refusal guards a bind address, not reachability,
  and the shipped container puts a listener on every interface in front of it —
  MEASURED 2026-09-02 on `a156ec25`, OPEN.** `BridgeConfig::resolve`
  (`crates/ackplane-bridge/src/lib.rs`) refuses any listen address that is not
  loopback, and its error tells the operator to "configure a production
  authentication verifier before exposing it". That refusal is the only
  authentication-adjacent control the Bridge has: there is no verifier, and
  ADR-0098 decision 3 keeps the refusal deliberately.

  The refusal is a check on `listen.ip().is_loopback()` — a fact about the
  process's own bind. ADR-0132 ("a bind address is not a reachable interface")
  already established, for the `ackplane` server, that a process cannot observe
  its own reachability and that binding loopback is not the same claim as being
  unreachable. The Bridge's control has not absorbed that lesson, and its own
  shipped packaging demonstrates why: `crates/ackplane-bridge/docker-entrypoint.sh`
  runs `socat TCP-LISTEN:3001,fork,reuseaddr TCP:127.0.0.1:3000`, a relay
  listening on **every** interface in the container and forwarding into the
  loopback bind the refusal protects. `BridgeConfig` sees a loopback address and
  is satisfied; the process is reachable from the container network anyway.

  **What actually keeps this safe today is one line of `docker-compose.yml`,**
  not the refusal: `- "127.0.0.1:${ACKPLANE_BRIDGE_PORT:-3000}:3001"` publishes
  the relay to host loopback only. The authors reasoned about this explicitly
  and documented it at length in that file and in `docs/ARCHITECTURE.md`, so
  the default deployment is safe by construction and this is not a live
  vulnerability. The gap is that the safety rests on a port-publication detail
  an operator may reasonably edit — changing it to `0.0.0.0:` is the obvious
  move when you want to reach the Bridge from another machine — while
  `BridgeConfig`'s refusal continues to report success and its error message
  keeps implying that exposure is what it prevents.

  **Impact:** an operator who widens the published port exposes an
  unauthenticated Bridge (every read surface, plus ADR-0142's Work/Design/
  Constitution command routes, which now execute against a verified principal
  derived from the development tenant token) with no error, no warning banner,
  and the loopback refusal still fully intact and silent. The control that
  reads like the guard is not the guard.

  **What would close it:** ADR-0132 decision 4's answer to the same problem for
  the `ackplane` listener — "a plaintext endpoint is never quiet", a startup
  banner naming the fact and the claim. The Bridge makes no equivalent
  statement. Note that ADR-0132 decision 3 is explicit that its
  `ACKPLANE_LISTEN_CONFINED_BY` declaration "never affects authentication", so
  the fix here is emphatically *not* to let a confinement declaration unlock a
  non-loopback Bridge bind; it is to stop the loopback bind being reported as
  though it were proof of confinement, and to say so out loud at startup when a
  relay or published port can carry traffic into it.

  **Not fixed this run, and deliberately not fixed by widening anything.**
  Recorded because two separate investigations (`task:1032d9034467`, and this
  one) reached `BridgeConfig`'s refusal and read it as a working exposure
  control without noticing the relay in front of it.
