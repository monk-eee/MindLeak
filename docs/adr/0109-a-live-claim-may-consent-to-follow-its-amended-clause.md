# ADR-0109: A live claim may consent to follow its amended clause

- Status: Accepted
- Date: 2026-08-20
- Deciders: MindLeak maintainers
- Accepted: 2026-08-20 by the repository owner, authorized directly in
  session - attributed human adoption after review.
- Refines: [ADR-0068](0068-an-amendment-carries-the-work-it-renames.md) (an
  amendment carries the work it renames) — narrows decision 5's "a live claim
  does not move either" to add one explicit, owner-consented exception
- Depends on: [ADR-0063](0063-a-migration-may-tidy-the-past-never-the-present.md)
  (a migration may tidy the past, never the present),
  [ADR-0048](0048-a-lapsed-lease-holes-the-window-it-does-not-move-it.md) (a
  lapsed lease holes the window, it does not move it)
- Related: `gaps.d/a-task-claimed-across-a-constitution-amendment-cannot.md`

## Context

ADR-0068 fixed a real defect: an amendment renames every clause it carries
forward, so a claim, binding, or task that still names the outgoing id becomes
silently ungoverned. Its decision 5 is deliberately absolute: *"a live claim
does not move either — including in the repair migration... There is no tool
to retarget a task's goal, and there should not be one: the only legitimate
reason a task changes clause is that the clause it served was renamed by an
amendment."* The reasoning is sound and this ADR does not dispute it: moving a
task's governing clause **without its holder's knowledge** changes the rule an
agent's evidence is judged against mid-flight, which is the same harm ADR-0063
already named for `tasks.owner`.

That absolute rule has a measured cost with no available remedy. On
`task:7b6154f1d69a` (governed by `goal:durable-intent-plane-for-multi-agent-coordinatio`,
amended to `constitution:v2` mid-claim), the holding agent had exactly two
options for the rest of its claim window, and both were wrong in a different
way:

- **Keep working under the stranded clause.** `governing_for_task` returns
  `[]`, `advise` returns `review`, and `check_conformance` returns `drift` —
  "governed code changed without a covering task", naming the very goal the
  task serves — for every commit, for the entire remainder of the claim. There
  is no action available to the holder that reaches `aligned`.
- **Release the claim** so the ADR-0068 repair (or the next amendment) can
  move it. This holes the evidence window (ADR-0048), which caps the eventual
  verdict at `needs_human` regardless of what the work actually was.

The only route to completion was a human overruling a `drift` verdict at
`complete_task` (`resolved_conformance_id: 182`). Nothing was broken and
nothing was laundered — this is the system correctly refusing to certify
something it cannot verify — but the *load* is real: every claim spanning an
amendment becomes human review work, and that load scales with amendment
frequency, not with any actual risk in the work done. Amendments are
infrequent by design (ADR-0043), so this is a small, recurring tax rather than
a crisis; it is nonetheless a real gap with a plausible, narrow close.

The distinction ADR-0068 protects against is **who initiates the move and
whether the holder knows about it**, not whether a move happens at all after a
claim exists. Decision 5's own wording is precise: the harm is a clause moving
*"under someone doing the work"* — invisibly, from their point of view. A
holder who explicitly asks to be moved onto their goal's current clause is not
that harm; they are the one person for whom "under them" and "with their
consent" are the same event.

## Decision

**The current owner of a claimed, live-leased task may explicitly request
that their task's `goal_id` move from a superseded clause to its active
same-slug successor. Nobody else may request this on their behalf, and it
changes nothing about `reconnect_superseded_clauses` or any future amendment's
own automatic carry-forward.**

1. **Only the task's own current owner may ask, and only for their own live
   claim.** Not an administrator, not a peer agent, not an automatic process.
   The request is meaningless — and refused — for a task that is not
   currently claimed by the caller with an unexpired lease. This is the entire
   distinction from decision 5's prohibited case: a move nobody but the
   holder can trigger cannot be a move made *behind* the holder.

2. **Eligibility is narrow and mechanical, matching `reconnect_superseded_clauses`
   exactly.** The task's current `goal_id` must name a clause with
   `status = 'superseded'` and exactly one active clause sharing its `slug`
   (the same "stable identity across versions" `reconnect_superseded_clauses`
   already uses — no new matching rule, no slug-rename inference, which
   ADR-0068 already rejected as invented mapping between differently-named
   clauses). If the outgoing clause has zero or more than one active same-slug
   successor, the request is refused and says why; it never guesses.

3. **The move is one attributed, append-only act on the task's own thread.**
   It records the old `goal_id`, the new `goal_id`, the requesting owner, and
   the timestamp — the same durability standard ADR-0068 already applies to
   the amendment's own carry-forward, just triggered by the holder instead of
   by the constitution write. This is not a second, parallel task-retargeting
   verb of the kind ADR-0068 rejected in its Alternatives section (a script
   using an imagined `link_goal_to_code`-style tool, with no attribution and
   no gate): it is gated to exactly one caller, exactly one clause transition,
   and it leaves a record shaped like the one the amendment itself would have
   left had the claim not been live.

4. **Reconnection never touches history.** It does not re-audit, delete, or
   relabel any `conformance_records` row already written under the old
   clause — ADR-0025's rule that we do not rewrite recorded verdicts applies
   unchanged. It changes only which clause governs the task's evidence
   **from the moment of reconnection forward**. A `drift` verdict already
   recorded while stranded stays exactly as recorded; the next
   `check_conformance` call after reconnection is judged against the new
   clause.

5. **The surface is an argument on an existing verb, not a new one.**
   `task_claim`'s `step="renew"` already touches the live claim under the
   caller's own identity and already requires no new session/ownership
   plumbing; this ADR's implementation should add an opt-in there (e.g. a
   boolean requesting reconnection) rather than invent a sibling tool, unless
   review finds a concrete reason `renew`'s existing contract cannot carry it
   cleanly. The exact shape (argument name, response field) is left to
   implementation; the constraint that matters is *reuse the existing
   claim-scoped verb set*, matching ADR-0068's own preference for putting a
   capability at its one correct home rather than a new door beside it.

6. **A refusal names the reason.** "Not superseded" (nothing to reconnect —
   most claims, most of the time), "ambiguous successor" (more than one
   active same-slug clause, which should not happen but must be reported
   rather than guessed at), and "not the current owner" are distinct,
   legible outcomes, not a single generic error.

## Consequences

- Closes the measured gap for the one case it actually affects: a task whose
  holder is present, still working, and would simply like their own claim to
  keep meaning something. It does nothing for a task whose holder has already
  moved on — that case is unaffected, and correctly stays on the existing
  release-then-let-the-migration-or-next-amendment-move-it path.
- `reconnect_superseded_clauses` and every future amendment's carry-forward
  are unchanged. This ADR adds one owner-initiated door beside the
  amendment's own; it does not reopen the door ADR-0068 closed for anyone
  else.
- The human-review load ADR-0068's own gap named should shrink for held
  claims specifically, without weakening the guarantee that a task never
  changes governing clause without its current holder's own action.
- Implementation (the store-level same-slug active-successor lookup and
  thread record, and the chosen call surface) is separate, larger work gated
  on this ADR's acceptance — not included in this change.

## Rejected alternatives

**Do nothing; the existing human-override path is sufficient.** Rejected as
the status quo, not a fix — it is the option this ADR exists to reconsider.
The load is real even though it is not urgent, and a narrow, safe close is
available.

**Let the ADR-0068 repair migration retry stranded tasks once their lease
expires, without requiring consent.** Rejected because a lapsed lease does not
mean the holder agreed to anything; it usually means the holder has moved on,
in which case the existing release-and-let-the-next-amendment-or-migration
path already applies. Retrying automatically on expiry also cannot distinguish
"abandoned" from "about to renew", and ADR-0048 already treats a lapse as
holing the window rather than as license to act on the task.

**Let any agent (not just the current owner) request reconnection for a task
it does not hold.** Rejected outright — that is exactly the "behind the
holder" harm ADR-0068 names, just requested by a third party instead of
triggered automatically. Restricting the request to the current owner, acting
on their own live claim, is the entire mechanism that makes this safe.

**Make reconnection also re-run conformance retroactively over the stranded
window.** Rejected because ADR-0025 already settled that recorded verdicts are
not rewritten; a stranded window's `drift` findings are a true historical
record of what the constitution looked like at that moment, and only a human
review (unchanged) revises what a `done` task's history means.

**Give reconnection its own dedicated MCP tool instead of extending `task_claim`.**
Not rejected outright — left as an implementation choice — but disfavoured
here because ADR-0059 treats every new tool as a cost against the default
session vocabulary, and this capability is a narrow argument on an act
(renewing your own live claim) an owning agent already performs routinely,
not a new category of action.
