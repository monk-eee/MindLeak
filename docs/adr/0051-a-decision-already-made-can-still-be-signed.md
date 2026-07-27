# ADR-0051: A decision already made can still be signed

- Status: Accepted
- Date: 2026-07-27
- Deciders: MindLeak maintainers
- Refines: [ADR-0047](0047-a-status-is-not-a-decision.md) (a status reflects a
  decision; only a decision records one)
- Related: [ADR-0019](0019-task-retention-and-board-hygiene.md)
  (archive, never delete), [ADR-0023](0023-design-board-accept-bridge.md) (the
  Design Board accept bridge), [ADR-0042](0042-designs-are-retired-by-a-person.md)
  (a design is retired by a person)

## Context

ADR-0047 named this failure precisely:

> because deciding is guarded on `proposed` the row is frozen: it asserts a
> decision that can never be attributed.

An imported ADR status lands at face value. The row says `accepted`, and
`decided_by` is empty, because reconciliation read a file rather than witnessing
a person. `decide_design_item` is guarded on `status = 'proposed'`, so the row
cannot be decided; it already claims to be.

ADR-0047 added `reopen_undecided_design` to unfreeze those rows — but guarded it
on `promotion_status = 'not_required'`, for a good reason: once promotion has
materialised tasks, that work descends from the acceptance, and reopening
underneath it would leave tasks pointing at a decision the ledger no longer
shows.

The consequence went unnoticed until the board was audited. Of 25 unattributed
designs, **18 had already materialised work** and were therefore beyond both
verbs: unreopenable *and* undecidable. Those 18 are ADR-0001 through ADR-0032 —
the decay engine, the intent plane, evidence-backed conformance. The
repository's founding decisions were the only ones the ledger could never
attribute to anybody, permanently, and the fix for exactly that complaint had
shipped four ADRs earlier.

The reason the gap survived is worth naming: ADR-0047 treated the problem as
*the row is in the wrong state*, and repaired it by moving the row back. That
framing only works while nothing is standing on the row. It is the wrong framing
for the common case, because the older and more load-bearing a decision is, the
more likely work descends from it — so the repair was least available exactly
where it mattered most.

## Decision

**Add `attribute_design_decision(id, human)`: record who made a decision the
ledger already asserts, without reopening it.**

The reframing is the whole of it. Attribution is not a decision and does not
pretend to be one. The decision is already there and work already descends from
it; what is missing is the name of the person behind it. So the verb writes
`decided_by` and nothing else — status, reason, and promotion state are all left
exactly as they stand.

Three guards define it:

1. **`decided_by IS NULL`.** Attribution fills an empty field and can never
   change a full one. Without this it would be a way to quietly replace one
   recorded human act with another, which is the erasure ADR-0019 and ADR-0047
   both refuse.
2. **`status <> 'proposed'`.** A proposed row asserts no decision, so there is
   nothing to attribute. It should be accepted or rejected — a real decision,
   made now.
3. **`promotion_status <> 'not_required'`.** This is the deliberate complement
   of `reopen_undecided_design`'s guard, and it is what keeps the two verbs from
   becoming two ways to do one thing. Between them every undecided row has
   exactly one route: if it can still be decided properly, reopening makes you
   decide it; only when that is closed off does attribution apply. Neither is a
   softer version of the other, and the error message for a wrongly-chosen verb
   names the right one.

## Consequences

- The 18 frozen foundational designs can be attributed. The ledger stops
  reporting that the decisions the whole repository rests on were made by
  nobody.
- Attribution is deliberately weaker evidence than a decision, and stays
  distinguishable from one: it is recorded after the fact by someone asserting
  what happened, where `accept_design` records someone deciding it in the
  moment. Nothing collapses the two, and `updated_at` moves while `created_at`
  does not.
- **A misspelled name is still permanent.** `decided_by` is free text, so
  `monk-ee` is as valid as `monk-eee`, and guard 1 means the typo can never be
  corrected — a wrong name is indistinguishable from a right one to every check
  in the system. This ADR does not fix that. Constraining the identity is a
  larger question about whether deciders are a closed set the ledger knows, and
  it wants its own decision rather than a validation rule smuggled in here.
- Nothing is retrospective. Existing rows keep their empty `decided_by` until
  somebody signs them, because inventing an attribution would be precisely the
  over-trusting import that caused this.
