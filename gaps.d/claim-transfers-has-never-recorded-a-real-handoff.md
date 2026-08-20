- **`claim_transfers` has never actually recorded a handoff in this fleet.** —
  Observed 2026-08-17: `task_query(view="claim_transfers")` is documented as
  "the append-only ownership recovery history," but `lodestar_stats` reports
  it empty for the entire session even though multiple concurrent agents
  repeatedly collided on the same files this session (exactly the scenario
  `task_claim(step="recover")` exists for) — every collision was avoided by
  `check_overlap`/live-claim checks before claiming, rather than resolved by
  taking over an existing claim. Where: `crates/lodestar-core/src/facade/...`
  (claim recovery path), `crates/lodestar-mcp/src/tools/executive.rs`
  (`task_claim` step `recover`). Impact: a code path that exists specifically
  to safely resolve real collisions has no production exercise behind it in
  this repository's own fleet, so latent bugs in it would only surface the
  first time it is actually needed — likely under pressure, not calmly. Left
  for later: not fixed this run; worth a deliberate drill (two sessions
  intentionally racing a claim) rather than waiting for a real incident to be
  the first exercise.
