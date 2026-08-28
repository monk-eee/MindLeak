- **Fixed:** `ackplane-supervisor`'s daemon (`crates/ackplane-supervisor/src/daemon.rs`)
  no longer ends the whole process on an ordinary dropped connection during
  registration, session announcement, or directive-receipt submission. Those
  three call sites propagated a transport failure with `.map_err(Box::new)?`,
  which `run`'s `serve_once(config).await?` has no way to catch, so the daemon
  exited permanently instead of reconnecting â€” even though the very next call
  site, the heartbeat, already treated the identical failure as an ordinary,
  retriable disconnect. All four connection call sites now share one
  `disconnected_on_error` helper and behave consistently: any transport
  failure reconnects, matching the daemon's own stated design ("Serve,
  reconnecting on a dropped connection... until evidence stops adding up").
