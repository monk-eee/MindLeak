- **`fleet_view` lists a quiet-but-live session and an abandoned one as the
  same kind of peer.** — Observed 2026-08-17: a live call listed 4 declared
  sessions, 2 with `head_sha: null` and `staleness.state: "unknown"`, declared
  hours apart from the other two, with no expiry and no "last seen" marker
  distinguishing them from a session still genuinely coordinating. Where:
  `crates/lodestar-core/src/facade/fleet.rs` (`fleet_view`),
  `crates/mindleak-session/src/lib.rs` (session registry). Impact: an agent
  reading `fleet_view` to avoid a collision cannot tell "this peer is quietly
  working" from "this peer's process died an hour ago and nobody closed it
  out" — both render identically, so the caller either over-trusts a dead
  entry or under-trusts a live one. Left for later: needs a "last observed at"
  timestamp per session plus an explicit staleness threshold (distinct from
  the existing per-task lease/claim staleness ADR-0044/0048 already solve),
  not fixed this run.
