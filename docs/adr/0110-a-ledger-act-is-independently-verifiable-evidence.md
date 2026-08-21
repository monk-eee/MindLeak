# ADR-0110: A Lodestar ledger act is independently verifiable evidence

- Status: Accepted
- Date: 2026-08-20
- Deciders: MindLeak maintainers
- Accepted: 2026-08-21 by the repository owner, authorized directly in
  session — attributed human adoption after review.
- Depends on: [ADR-0009](0009-evidence-backed-conformance.md) (evidence-backed
  conformance), [ADR-0058](0058-work-that-shipped-must-leave-the-board.md)
  (work that shipped must leave the board)
- Related: [ADR-0025](0025-authoritative-checked-conformance.md) (authoritative
  checked conformance), [ADR-0060](0060-work-whose-product-is-not-code-must-still-conform.md)
  (work whose product is not code must still conform),
  `gaps.d/an-agent-can-work-all-day-certify-nothing.md`

## Context

ADR-0009's conformance gate refuses before it does anything else:

```rust
if evidence.changed_node_ids.is_empty() || evidence.provenance.is_empty() {
    findings.push("evidence contains no provenance-bearing mutation".to_string());
    return Ok(ConformanceResult { verdict: Verdict::NeedsHuman, findings });
}
```

(`crates/lodestar-core/src/facade/conformance/verdict.rs`,
`evaluate_base_conformance`.) This is correct for code: MindLeak's graph is
the only place that records what an agent actually changed, so evidence with
no MindLeak mutation is evidence of nothing.

It is not correct for every act Lodestar itself performs. `register_design`,
`decide_design_item`, `supersede_goal`, `grant_waiver`, and constitution
amendment all mutate Lodestar's own durable SQLite tables — `design_items`,
`goals`, `waivers`, `amendments` — and nothing else. They create no MindLeak
node, no execution, no commit. Not because the agent forgot to record them,
but because there is nothing for MindLeak to record: the act's entire effect
is a row in a store MindLeak has never seen and structurally cannot see
(ADR-0004's loose seam between the planes).

ADR-0060 already fixed a *different* branch of this same function: evidence
that exists (real commits, real `changed_node_ids`) but names an artefact
`link_goal_to_code` never bound, such as `DEVELOPERS.md`. That fix runs after
the gate quoted above. A pure ledger act never reaches it, because it never
gets past `changed_node_ids.is_empty()` in the first place — there is no
evidence to judge as code-bound or not, only its total absence.

`gaps.d/an-agent-can-work-all-day-certify-nothing.md` names this precisely,
having already had its original, different cause (a dropped `ingest_commit`
argument) fixed and removed in an earlier narrowing:

> What remains has the opposite cause. `design_register`, decision
> attribution, supersession, waiver grants, and task resolution mutate only
> Lodestar's durable ledger. They create no commit, execution, or changed
> MindLeak node, so `evidence_for` is empty because the act genuinely changed
> no repository artifact. Measured on `task:680b14565a8f`: registering
> ADR-0073 produced check 369 `needs_human` with "evidence contains no
> provenance-bearing mutation", and human resolution was the only honest
> terminus.
>
> Do not manufacture a file edit to clear this result; that launders a ledger
> act as code evidence. Closing the residual requires an explicit design for
> a first-class, attributable ledger-act evidence kind.

This is not a new category of problem. ADR-0058 already solved its sibling:
a git merge is not a MindLeak node mutation either, and that ADR made it
count as evidence anyway, because Lodestar can verify a merge deterministically
against `main` without asking MindLeak. A Lodestar ledger act is a *stronger*
case than a merge — Lodestar does not need to verify it against an external
system at all, because Lodestar's own store already recorded it, with an
actor and a timestamp, the moment it happened.

## Decision

**A Lodestar ledger act, from a closed and enumerated set, is conformance
evidence when its own recorded actor and timestamp match the current claim —
verified entirely inside Lodestar, with no MindLeak call.**

1. **The eligible set is closed and named, not "any write".** Exactly four
   act kinds qualify at adoption:

   | Kind | Table / actor field | Timestamp field |
   |---|---|---|
   | `design_registered` | `design_items.proposed_by` | `created_at` |
   | `design_decided` | `design_items.decided_by` | `updated_at` |
   | `goal_superseded` | `goals` supersession record | its recorded time |
   | `waiver_granted` | `waivers.approved_by` | `created_at` |
   | `constitution_amended` | `amendments.amended_by` | its recorded time |

   Adding a sixth kind is a future ADR amendment to this table, the same
   discipline ADR-0009 already applies to what counts as a `changed_node_id`
   source. This is deliberately not the generic conformance DSL ADR-0009
   rejected — it names five specific, already-existing, already-attributed
   mutations, not an open predicate over arbitrary store writes.

2. **A new deterministic builder, `ledger_act_evidence`, mirrors
   `merge_evidence`'s shape.** `ledger_act_evidence(task_id, act_ref,
   session_id)` takes a reference to one concrete row (a design item id, a
   waiver id, a goal supersession, an amendment id) and verifies, with no
   model and no MindLeak round trip:

   - the row exists and its kind is one of the five above;
   - its recorded actor matches the agent identity the current claim is
     registered under (the same identity check `complete_task` already runs
     for `agent_id`, ADR-0009's "Claim-bounded identity and time");
   - its recorded timestamp falls inside the live claim's window
     (`started_at >= claim_started_at`, the same rule every other evidence
     source already obeys).

   It returns a `ConformanceEvidence`-shaped bundle carrying a new
   `ledger_act_ids: Vec<String>` field alongside the existing
   `changed_node_ids`, with `provenance` entries naming the act's own kind and
   row id in place of a MindLeak node/relation pair — evidence about
   Lodestar's ledger is provenanced against Lodestar's ledger, not forced
   through a MindLeak shape it was never a mutation of.

3. **The base gate widens to admit either source, not to admit less.**
   `evaluate_base_conformance` refuses only when *both* are empty:
   `changed_node_ids.is_empty() && ledger_act_ids.is_empty()`. A task with
   neither still gets exactly today's `needs_human`. A task with a code
   mutation is unaffected. Only a task whose entire evidence is one eligible,
   attributed, in-window ledger act newly has something to be judged against.

4. **Conformance still judges afterward, exactly as ADR-0058 decision 3
   requires for a verified merge.** A ledger act counting as evidence answers
   "did something real and attributable happen", not "did it serve this
   task's goal". Goal-coverage checking, `forbid_change` locks, and drift
   detection run unchanged on whatever the act's kind implies it touched
   (e.g. a `goal_superseded` act is judged against the constitution-governance
   goal, not against an arbitrary task's own goal). A ledger act that does
   not serve the claiming task's goal is still `drift`, exactly as an
   unrelated commit is today.

5. **This does not change what `design_register`, `grant_waiver`,
   `supersede_goal`, or amendment already do.** No new argument, no new
   required call at the point the act happens. `ledger_act_evidence` reads a
   row that already exists; it does not need the act itself to change shape.

## Consequences

- The ten-and-more tasks parked purely because their entire product was a
  ledger act (registrations, decisions, supersessions, waivers) gain a real
  path to `aligned` instead of a permanent `needs_human`, the same relief
  ADR-0058 gave merged-but-unevidenced code work.
- `evidence_for`(MindLeak) is unchanged. This adds a second, Lodestar-internal
  evidence source beside it; it does not touch the graph query or its schema.
- The closed enumeration means this cannot silently expand into "every
  Lodestar write certifies its own task" — a new kind needs its own line in
  the table above and its own review, the same bar ADR-0009 set for adding a
  `changed_node_ids` source in the first place.
- Historical `needs_human` verdicts on tasks whose only work was an eligible
  ledger act remain historical records (ADR-0025: verdicts are not rewritten).
  They can be re-audited under this rule once adopted, or resolved by a human,
  the same disposition ADR-0060 left for its own ten stranded tasks.

## Rejected alternatives

- **Treat any Lodestar store write as evidence.** Rejected for the same
  reason ADR-0009 rejected a generic conformance DSL: it would make every
  read-adjacent internal mutation self-certifying, including ones nobody
  intends as durable, attributable, authority-bearing acts.
- **Route ledger acts through `evidence_for` by writing a matching MindLeak
  node for each one.** Rejected: this is exactly the gap fragment's own
  prohibition — manufacturing a MindLeak mutation to describe an act that
  never touched MindLeak launders provenance instead of recording it
  honestly, and doubles the audit surface for something Lodestar already
  knows first-hand.
- **Let a human resolve every ledger-only task by hand, permanently.**
  Already the status quo, and exactly what this ADR is proposing to narrow —
  correct for genuinely ambiguous authority-bearing acts, wrong as the only
  route for five specific, already-attributed, already-timestamped ones.
