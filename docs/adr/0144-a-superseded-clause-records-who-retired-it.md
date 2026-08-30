# ADR-0144: A superseded clause records who retired it

- Status: Accepted
- Date: 2026-08-29
- Deciders: MindLeak maintainers
- Amends: [ADR-0110](0110-a-ledger-act-is-independently-verifiable-evidence.md)
  (a ledger act is independently verifiable evidence — this supplies the one
  prerequisite it named and adds the fifth variant it anticipated)
- Related: [ADR-0009](0009-evidence-backed-conformance.md)
  (evidence is bounded by the claim that authorised it),
  [ADR-0050](0050-a-superseded-decision-is-not-a-stale-one.md)
  (a superseded decision is not a stale one)

## Context

ADR-0110 made a Lodestar-internal act first-class conformance evidence: an agent
whose work changed no repository artifact can complete a task by naming an act
the ledger already recorded, and the plane verifies — with no MindLeak call —
that the act exists, that **its own recorded actor** matches the resolved agent,
and that its timestamp falls inside the live claim window.

It admitted four kinds, and named the reason a fifth was excluded:

> `GoalSuperseded` is not a variant here: `supersede_goal` records no actor for
> the act itself (only a free-form `reason`), so there is nothing to verify
> against the claiming agent yet. Wiring it in needs that prerequisite fixed
> first, not a fabricated attribution.

That exclusion was correct and the prerequisite was never met, so superseding a
clause — one of the most consequential acts in the system, since it is the only
way governing intent changes — remained the one act that could not be proved.
An agent whose entire task was to retire a clause had no honest completion path
and routed to human review every time.

Two places in the code claimed otherwise. `LodestarStore::supersede_goal`'s doc
comment read *"Intent changes only through this explicit, attributed step"*, and
the MCP tool description read *"The only way intent changes — explicit and
attributed"*. Neither was true: the write recorded `superseded_by` (which
version replaced it) and a free-form `reason`, and no actor at all. A comment
asserting a property the code does not have is worse than silence, because it
stops the next reader checking.

## Decision

**A supersession records who performed it and when, on the clause it retires.**

`goals` gains two nullable columns, `superseded_at` and `superseded_by_agent`,
written by `supersede_goal` alongside the existing `superseded_by`. The retiring
agent is resolved through the same session path every other identity-bearing
call uses, so a caller cannot name someone else as the one who did it.

**The actor goes on the old row, not the new one.** The two rows answer
different questions: the new version records *why it was written* (`reason`),
the retired one records *who retired it and when*. Putting the actor on the
replacement would attribute the act to the clause that resulted from it.

**`LedgerActKind::GoalSuperseded` is added**, and `ledger_act_evidence` verifies
it exactly as the other four are verified — same actor match, same claim-window
bound, same absence of any MindLeak call. Its `act_id` is the **retired**
clause's id, because that is the row the act is recorded on.

**Pre-existing supersessions stay unattributed and are refused.** The columns
backfill as NULL. A clause retired before this change has no recorded actor, and
`ledger_act_evidence` returns an error naming that reason rather than accepting
the claiming agent as a stand-in. This is the same discipline ADR-0110 applied
when it declined to add the variant at all: an attribution that was never
recorded cannot be reconstructed, and inventing one would make the evidence
chain assert something false about the past.

## Consequences

- A ledger-only task that retires a clause can now certify itself. That was the
  last remaining case in the known-gap fragment
  `an-agent-can-work-all-day-certify-nothing.md`, which is closed and deleted by
  this decision. (ADR-0110 cites the same fragment; that citation is history —
  it describes what was true when ADR-0110 was written, and is left as written
  rather than rewritten to match the present.)
- The closed set grows from four kinds to five. It remains closed: adding a
  sixth is another ADR amendment, and this one does not weaken that rule — it
  satisfies the exact prerequisite the rule demanded, which is what the rule
  existed to force.
- **`supersede_goal` now requires a session id.** The facade signature gains an
  `agent` parameter and the MCP tool gains a required `session_id`. This is a
  breaking change to both surfaces, taken rather than adding an optional
  parameter that would leave the act unattributable whenever it was omitted —
  an optional attribution is an absent one on exactly the calls that matter.
- Two false claims in the codebase become true. The doc comment and the tool
  description that already said "attributed" now describe what the code does.
- Nothing is retrofitted. The migration adds columns and touches no existing
  row, so the audit trail gains new facts and loses none.

## Alternatives considered

**Record the actor in the free-form `reason` string.** Rejected: a verifiable
attribution cannot live in prose. `ledger_act_evidence` compares the recorded
actor to the resolved agent, and parsing an identity back out of a
human-authored sentence would make the comparison depend on formatting.

**Backfill existing supersessions with the current constitution's amender.**
Rejected outright, and it is the tempting option. Those acts happened; someone
performed them; nothing recorded who. Writing a plausible name in is precisely
the fabricated attribution ADR-0110 refused, and it would corrupt the one
property that makes ledger-act evidence worth anything.

**Leave it and accept `needs_human` forever.** Rejected: it makes the honest
completion path unavailable for a real class of work, and an unavailable honest
path is how agents learn to manufacture a file edit to clear the result — which
launders a ledger act as code evidence and is worse than the problem.
