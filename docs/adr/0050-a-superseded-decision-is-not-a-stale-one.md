# ADR-0050: A superseded decision is not a stale one

- Status: Accepted
- Date: 2026-07-27
- Related: [ADR-0023](0023-design-board-accept-bridge.md) (design items
  and human acceptance), [ADR-0042](0042-designs-are-retired-by-a-person.md)
  (a design is retired by a person, never by a missing file),
  [ADR-0047](0047-a-status-is-not-a-decision.md) (a status reflects a decision;
  only a decision records one)

## Context

`make design-audit` compares every ADR file against the design ledger. On its
first run, two of the forty-eight files could not be classified at all:

```
note  unrepresentable 0018  file says "Superseded by [ADR-0032](...)"
note  unrepresentable 0032  file says "Superseded by"
```

The ledger has three statuses: `proposed`, `accepted`, `rejected`. None of them
is "this was decided, it held, and it has since been replaced". So the audit had
to invent a fourth category to avoid lying in either direction — reporting these
as drift would be wrong, because neither side is stale. The file and the ledger
are not disagreeing; the ledger simply cannot hold what the file is saying.

The workaround available today is to leave the row `accepted`. That is how both
rows stand now, and it is actively misleading: ADR-0018's decision was replaced
by ADR-0032, but any ledger-driven view — the Design Board, promotion planning,
`governing_goals` — sees a live accepted design indistinguishable from one still
in force. A reader following the ledger rather than the file will act on a
decision that was withdrawn.

Deleting or retiring the row is worse. ADR-0042 already settled that a design is
retired by a person and never by absence, and retirement means "this record
should not have existed" — not "this decision was superseded by a better one".
Superseding is part of the record, and losing the link loses why the replacement
exists.

**The goal model already solved this.** `goal` carries `superseded_by` and
`supersede_goal` writes it, so an objective that has been replaced points at its
replacement and stops being treated as live. Designs, which are the same kind of
durable, human-decided record, have no equivalent. That asymmetry is the gap —
not a missing enum variant.

## Decision

**Give a design the same supersession the goal model already has.**

1. `design_items` gains `superseded_by TEXT` and `superseded_at INTEGER`,
   mirroring `goal.superseded_by`.
2. `supersede_design(id, superseded_by, human)` records that an accepted design
   has been replaced. It is guarded on `status = 'accepted' AND decided_by IS
   NOT NULL`: superseding is a statement about a decision that was actually
   made, so a row that never carried one cannot be superseded — it should be
   reopened and decided (ADR-0047), or retired (ADR-0042).
3. The replacement must already be a registered design. A dangling reference
   would leave a reader with a superseded decision and nowhere to go.
4. `status` is unchanged. The design stays `accepted`, because it was accepted —
   supersession is a separate fact about a decision, not a different decision.
   Views filter on `superseded_by IS NULL` to mean "live".
5. `reconcile_designs` does not infer this from the file. `Superseded by <ref>`
   in an ADR is prose, and inferring a link from it would repeat exactly the
   mistake ADR-0047 documents: a file successfully telling the ledger something
   no one is recorded as deciding. A human runs `supersede_design`, and
   `make design-audit` reports the mismatch until they do.

## Consequences

The audit's `unrepresentable` category shrinks to a genuine finding: a file
claiming supersession that the ledger has not been told about. That is drift,
and it will be reported as drift once this lands.

Anything reading the ledger for live designs must filter on `superseded_by IS
NULL`. Missing that filter is a new way to be wrong — but it is the same shape
as the goal model's, so there is one rule to learn rather than two.

Two rows need the new verb applied once it exists: ADR-0018, superseded by
ADR-0032, and ADR-0032's own declaration, which currently reads `Superseded by`
with no reference at all and needs a human to say what replaced it.

## Alternatives considered

**Add a `superseded` status.** Simpler to write and wrong: it discards the fact
that the design *was* accepted, and it cannot say by what. It also diverges from
the goal model for no reason, leaving two vocabularies for one idea.

**Retire the row (ADR-0042).** Retirement means the record should not have
existed. A superseded decision should have existed; it was correct and then it
was replaced. Conflating the two loses the history the ledger exists to hold.

**Leave it as prose in the file.** The status quo. It keeps the ledger quietly
overstating what is in force, which is the one property it exists to be trusted
about.
