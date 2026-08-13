- **A constitutional amendment cannot be attributed to a human, so the record
  cannot tell adoption from an agent acting alone — OBSERVED 2026-08-13, left
  OPEN.** `amend_constitution` promotes a draft and stores `amended_by`, and
  every route to that field ends at the calling agent:

  | Layer | What it does with attribution |
  |---|---|
  | Advertised schema (`LODESTAR_TOOL_PROFILE=full`) | `draft_id`, `rationale`, `session_id` — no `human` |
  | `apply_session_contract`, [`crates/lodestar-mcp/src/tools/mod.rs`](../crates/lodestar-mcp/src/tools/mod.rs) | inserts `agent` from the resolved session, overwriting any caller value |
  | `amend_constitution`, [`crates/lodestar-mcp/src/tools/amendments.rs`](../crates/lodestar-mcp/src/tools/amendments.rs) | passes that `agent` as `amended_by` |
  | `Lodestar::amend_constitution`, [`crates/lodestar-core/src/facade/amendments.rs`](../crates/lodestar-core/src/facade/amendments.rs) | forwards to the store, no guard |

  So a human label cannot be supplied even out of band: the injection runs after
  the caller's arguments and replaces `agent` outright.

  What makes this a gap rather than a preference is the contrast inside the same
  plane. `task_transition to="resolve"` takes an explicit `human`, records it in
  `resolved_by`, and refuses when it equals the agent under review. Closing one
  task therefore demands an attribution that amending the entire constitution
  does not, and the constitution is the larger act.

  **This is not privilege escalation, and should not be read as one.** The
  server is local, stdio-only and unauthenticated by design (ADR-0004), and
  ADR-0071 is explicit that a `human` label is an attributable declaration
  rather than authentication. Nothing here could be enforced by a check anyway.
  The defect is narrower and entirely about the record: ADR-0043 makes adoption
  into the active constitution an *attributed* amendment, and ADR-0026 places
  constitutional authority with a human — but `amendments`, the audit history
  those decisions rely on, can only ever name the agent that made the call. A
  reader a year later cannot distinguish a human's adoption from an agent
  amending policy on its own initiative.

  Left open because the repair is a constitutional question, not a mechanical
  one: whether `amend_constitution` should take a `human` distinct from the
  calling agent, and whether it should refuse without one, is exactly the kind
  of decision ADR-0026 reserves for a person. An agent surfacing it is right;
  an agent deciding it is not.

  Answers the open question raised in
  [`the-constitution-verbs-were-reachable-all-along.md`](the-constitution-verbs-were-reachable-all-along.md)
  ("may not be expressible yet") — it is not expressible, and this is the
  mechanism. That fragment covers the separate advertisement problem and closes
  independently of this one.
