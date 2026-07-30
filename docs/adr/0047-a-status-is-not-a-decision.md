# ADR-0047: A status reflects a decision; only a decision records one

- Status: Accepted
- Date: 2026-07-27
- Related: [ADR-0019](0019-task-retention-and-board-hygiene.md) (archive, not
  delete), [ADR-0023](0023-design-board-accept-bridge.md) (design items
  and human acceptance), [ADR-0042](0042-designs-are-retired-by-a-person.md)
  (a design is retired by a person, never by a missing file)

## Context

`reconcile_designs` imports a repository's ADRs into the design ledger, taking
each file's declared `Status:` at face value. That is deliberate and it is right:
importing thirty-five settled decisions as `proposed` would flood the Design
Board with work that was decided long ago, and would misrepresent the
repository's own record. Two tests already assert it.

But an imported status arrives with `decided_by` empty, because reconciliation
observes a file — it does not witness a decision. And `decide_design_item` is
guarded on `status = 'proposed'`, so the row is now frozen. It permanently
asserts a decision, and no verb can ever name who made it.

Observed directly. ADR-0045 was reconciled from a file declaring `Accepted`. A
reviewer then said, of that ADR, "I agree with it" — and there was no way to
write that down:

```
accept_design(design:0045-…, human=monk-eee)
→ invalid: already accepted; only a proposed item can be decided
```

The ledger's whole purpose is to be the authority a file is checked against. A
row that claims a decision nobody is recorded as making inverts that: the file
has successfully told the ledger something the ledger cannot verify or attribute.

## Decision

**Add `reopen_undecided_design(id)`: a design whose status has no `decided_by`
returns to `proposed`, so a human can decide it properly.**

Three guards define what this is:

1. **`decided_by IS NULL`.** A recorded human act is never reopened by this
   verb. Superseding a real decision is a new decision — rejection, or a later
   ADR — not the quiet erasure of the old one (ADR-0019).
2. **`promotion_status = 'not_required'`.** Once promotion has materialised
   tasks, that work rests on the acceptance. Reopening underneath it would leave
   tasks descending from a decision the ledger no longer shows.
3. **Not retired.** A retired row has left the board deliberately (ADR-0042);
   reopening it would drag it back.

Import behaviour is unchanged. The declared status still lands as written, and
the absence of a `decided_by` remains visible in every read of the row — the
signal that this status was reflected rather than decided.

## Rejected alternatives

**Import everything as `proposed`.** Tried first, and two existing tests
rejected it immediately: historical and terminal ADRs would arrive as pending
decisions, putting thirty-five settled items on the actionable board and
implying a repository has decided nothing. Trading a silent wrong for a loud one
is not an improvement.

**Backfill a decider during import.** There is nobody to name. Inventing an
approver is the falsified receipt this ADR exists to prevent, written by the
system instead of by accident.

**Let `accept_design` operate on any undecided row.** This would work, but it
makes acceptance's guard conditional and easy to misread — "only a proposed item
can be decided" is a rule worth keeping absolute. A separate verb states the
repair plainly and keeps the decision path unchanged.

**Edit the row directly.** Rejected on sight: mutating the ledger under its own
audit is exactly what the audit exists to detect.

## Consequences

- A status imported from a file can be turned into an attributed decision,
  instead of being permanently unattributable.
- `decided_by: null` becomes meaningful rather than incidental: it distinguishes
  *reflected* from *decided*, and now has a remedy.
- The verb is narrow by construction. It repairs rows nobody decided and refuses
  everything else, so it cannot become a general-purpose undo.
- It does not prevent the situation, only repair it. Imports still produce
  unattributed statuses; that is the accepted cost of importing history
  faithfully.

## Enforcement and test plan

Platform-agnostic (`cargo` / `npm` / `node` / `git` only):

1. **An imported status is reopenable.** A design reconciled as `accepted`
   cannot be decided, reopens to `proposed`, and can then be accepted by a
   person.
2. **A real decision is not.** Once `decided_by` is set, reopening is refused
   and the status and decider are unchanged.
3. **Materialised promotion refuses.** With `decided_by` absent but promotion
   armed, reopening is still refused — proving the promotion guard carries its
   own weight rather than riding on the decider check.
4. **Import behaviour is unchanged**, asserted by the two pre-existing
   reconciliation tests that this decision deliberately left passing.
