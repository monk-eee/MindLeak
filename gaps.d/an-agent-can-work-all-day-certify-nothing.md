- **A ledger-only task has no mutation evidence and therefore routes to human
  review - NARROWED 2026-08-25 by ADR-0110, one residual still OPEN.** The
  original gap covered normal code work whose agent forgot to call
  `ingest_commit`. That path is closed: the shared `post-commit` hook records
  commits, `scripts/hook-health.mjs` verifies the hook is installed before
  push, and canonical publication records the published head as a second
  deterministic path.

  The second cause -- a Lodestar-internal act that changes no repository
  artifact, so `evidence_for` is legitimately empty -- is now also closed for
  most acts. [ADR-0110](../docs/adr/0110-a-ledger-act-is-independently-verifiable-evidence.md)
  (Accepted 2026-08-21) made a ledger act first-class evidence in its own
  right: `Lodestar::ledger_act_evidence` (`facade/evidence.rs`) builds a
  bundle from one act, the `ledger_act_evidence` MCP tool exposes it to
  agents, and `facade/conformance/verdict.rs` counts `ledger_act_ids`
  alongside `changed_node_ids`, so a ledger-only completion no longer trips
  the `evidence contains no provenance-bearing mutation` refusal. The plane
  verifies deterministically -- with no MindLeak call -- that the named act
  exists, that its OWN recorded actor matches the resolved agent, and that
  its timestamp falls inside the live claim window.

  So the historical measurement in this fragment is no longer the expected
  outcome: registering a design (`task:680b14565a8f`, check 369
  `needs_human`) would today build real evidence and reach a real verdict.
  Do not read this fragment as advice to accept `needs_human` for a ledger
  act, and do not manufacture a file edit to clear the result -- that
  launders a ledger act as code evidence, and is now unnecessary as well as
  wrong.

  **Still OPEN -- goal supersession has no recorded actor.** `LedgerActKind`
  admits exactly four kinds: `design_registered`, `design_decided`,
  `waiver_granted`, `constitution_amended`. `GoalSuperseded` is deliberately
  absent because `supersede_goal` records only a free-form `reason` for the
  act, never an actor, so there is nothing to verify against the claiming
  agent. Human review remains the honest terminus for that one act until
  `supersede_goal` durably records who performed it; wiring it in before
  then would require fabricating an attribution. Adding any further variant
  is likewise an ADR amendment decision, not an escape hatch for "any
  Lodestar write".
