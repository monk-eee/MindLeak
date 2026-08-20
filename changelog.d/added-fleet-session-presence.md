- **Added:** `fleet_view` now reports a `presence` (`live`/`quiet`/`stale`) for
  every session, distinct from `staleness` (which is about a session's
  declared base, not whether the session itself still shows a pulse). A
  session is `live` while it holds any claim, `quiet` if it declared context
  within the last hour, and `stale` beyond that — the same "a live claim or a
  recent declaration counts as alive" rule the fleet view's stale-wait
  detection already applied per-wait, now surfaced per-session so a reader
  can tell a quiet-but-live peer apart from one that has gone silent
  (ADR-0035, ADR-0046).
