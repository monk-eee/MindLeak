- **`claim_transfers` has never actually recorded a handoff in this fleet —
  DRILLED 2026-08-24, narrowed to a sharper residual.**
  Observed 2026-08-17: `task_query(view="claim_transfers")` is documented as
  "the append-only ownership recovery history," but `lodestar_stats` reports
  it empty for the entire session even though multiple concurrent agents
  repeatedly collided on the same files this session (exactly the scenario
  `task_claim(step="recover")` exists for) — every collision was avoided by
  `check_overlap`/live-claim checks before claiming, rather than resolved by
  taking over an existing claim. Where: `crates/lodestar-core/src/facade/...`
  (claim recovery path), `crates/lodestar-mcp/src/tools/executive.rs`
  (`task_claim` step `recover`).

  **Ran the deliberate drill this called for, against the live server, not a
  unit test.** A throwaway task was claimed under one session with a 5s
  lease, left to expire for real, then a second session attempted to take it
  over. `step="recover"` refused outright: *"claim recovery requires a
  compatible legacy owner and registered session identity"* — its own
  `open_session` `rescue_work` offer for the same task suggested a plain
  `step="claim"` instead, which succeeded, started a genuinely fresh
  `claim_started_at` (not a preserved window), and left `claim_transfers`
  empty afterward, confirmed by a direct `view="claim_transfers"` read.

  **The sharper finding: `recover` is not the mechanism a same-fleet, cross-
  session lapsed-lease handoff actually goes through.** The store-level
  mechanism (`crates/lodestar-core/src/store/claim_transfer.rs`) already has
  7+ unit tests and is not itself unexercised code — what has never fired is
  its *eligibility* for the ordinary "a different session claims a task whose
  lease genuinely expired" case, which this drill shows `recover` declines and
  a plain re-claim satisfies instead, silently, with no audit row. Whether
  `recover`'s "compatible legacy owner" gate is meant to be this narrow (ADR-
  0063 identity-migration transfers and paused-task grace only) or whether an
  ordinary cross-session lapsed-lease reclaim should also be recorded in
  `claim_transfers` rather than starting an unaudited fresh window is a design
  question, not a bug this drill can resolve by itself — recorded here rather
  than guessed at. Left for later: not fixed this run.
