# ADR-0077: A crowded board is not a decision

- Status: Accepted
- Date: 2026-07-31
- Deciders: monk-eee
- Related: [ADR-0023](0023-design-board-accept-bridge.md) (design items and human
  acceptance), [ADR-0027](0027-extension-led-progressive-disclosure.md)
  (extension-led progressive disclosure), [ADR-0042](0042-designs-are-retired-by-a-person.md)
  (a design is retired by a person, never by a missing file),
  [ADR-0047](0047-a-status-is-not-a-decision.md) (a status reflects a decision;
  only a decision records one), [ADR-0050](0050-a-superseded-decision-is-not-a-stale-one.md)
  (a superseded decision is not a stale one),
  [ADR-0059](0059-the-tool-surface-is-a-vocabulary.md) (the tool surface is a
  vocabulary)

## Context

The Design Board currently renders seventy-seven `proposed` design items. That
is not a rendering artefact — every one of those rows is a real ADR that a human
has not yet accepted or rejected, and `actionable_design_items` is correct to
surface them: by ADR-0023 a proposed design *is* actionable, because acceptance
is the human gate the whole board exists to present. The board is honest. It is
also unusable, because seventy-seven equally-weighted "decide me" rows is past
the number a person can forage, so in practice none of them get decided and the
backlog only grows.

The tempting fix is an `archived` status (or a "hide terminal ADRs" filter that
defaults on) so the board shows only a handful. **That fix is the exact mistake
this codebase has now rejected three times.** ADR-0047 established that a status
records a decision and nothing else; ADR-0042 refused a fourth `retired` status
because it would overwrite "was this accepted?"; ADR-0050 refused a fourth
`superseded` status for the same reason and made supersession an orthogonal fact.
An `archived` status would be a fourth decision-shaped thing that is not a
decision — and worse than the two we already declined, because it would default
to *hiding undecided work*. A proposed design hidden by default is a decision no
one made, rotting silently, which is the one property ADR-0050 says the ledger
must never have: a view that quietly understates what is live.

So the crowding has two distinct causes, and conflating them is what produces the
bad fix:

- **Some of the seventy-seven are genuinely stale** — a proposal the maintainers
  have looked at and do not intend to pursue *now*, but are not ready to reject
  outright. Today there is no honest verb for "a real proposal, parked, not now".
  `reject` overstates it (a durable no), `retire` is wrong (retirement means the
  record should not have existed, ADR-0042), and leaving it `proposed` keeps it
  shouting on the board. This is a genuine gap in the vocabulary.
- **Most of them are simply many** — live proposals that all legitimately await a
  decision. Nothing about them is stale; there are just more than fit a glance.
  This is not a ledger problem at all. It is a view problem, and ADR-0027 already
  says the view's job is progressive disclosure.

## Decision

**Thin the board by a reader's view or a person's explicit act — never by a
silent status.** Split the fix along the two causes above.

1. **No `archived` status, and no default hiding of undecided work.** `proposed`,
   `accepted`, and `rejected` remain the only decision statuses. A `proposed`
   design is never removed from the default board by anything other than a human
   act recorded against it. This clause is the point of the ADR; the rest is how
   we honour the two real needs without breaking it.

2. **Add a `defer` act for "a real proposal, parked, not now".** Mirroring
   retirement (ADR-0042) and supersession (ADR-0050), deferral is an orthogonal
   fact, not a status. `design_items` gains nullable `deferred_at INTEGER`,
   `deferred_by TEXT`, and `deferred_reason TEXT`. `design_decide` gains a
   `defer` decision, guarded on `status = 'proposed'`: only an undecided row can
   be parked, because a decided one is superseded, retired, or reopened, not
   deferred. Deferral is attributed to a person and carries a rationale, exactly
   like accept/reject/retire — nothing infers it and no background process
   performs it.

3. **Deferral is reversible, and its inverse is explicit.** `design_decide` gains
   a `resume` decision that clears the three `deferred_*` columns and returns the
   row to the working board. The row's `status` never changed (it stayed
   `proposed` throughout), so `resume` is not `reopen` (ADR-0047, which repairs a
   *decided* row back to undecided) — it is deferral's own undo. A deferred design
   is fully live in the ledger, counts everywhere counts are taken, and is one
   `resume` from the board.

4. **The board omits deferred rows by default and exposes them on demand.**
   `actionable_design_items` adds `deferred_at IS NULL` to its filter, alongside
   the existing `retired_at IS NULL` and `superseded_by IS NULL`. `design_query`
   gains an `include_deferred` flag on the `ledger` view, mirroring the existing
   `include_retired`, so the audit trail stays complete. "Deferred" is thus
   reported the same way retirement and supersession already are: live for the
   ledger, quiet on the board, never lost.

5. **A backlog is cleared in one attributed act, not seventy-seven.**
   `design_decide` accepts an optional `ids: [..]` in place of a single `id` for
   the `defer`, `resume`, `reject`, and `retire` decisions, applying one shared
   `human` and `reason` across the batch and logging each row individually. One
   maintainer parking or rejecting a swathe of stale proposals records one honest
   act with one rationale, rather than being forced into seventy-seven clicks or
   into a silent status to avoid them.

6. **The Design Board view discloses progressively (ADR-0027).** Independently of
   the ledger, the extension stops rendering the actionable set as one flat,
   equally-weighted list. It shows a header count ("N awaiting decision"), groups
   `proposed` above pending-promotion rows, caps the rendered tail with an
   expand-to-see-all affordance, and offers a "show deferred" toggle that calls
   `design_query view:ledger include_deferred:true`. This is a pure view change:
   it moves no rows, decides nothing, and holds no state the ledger does not.

## Consequences

- The board becomes forageable without lying. A maintainer parks the genuinely
  stale proposals with `defer` (in bulk, with a reason), and progressive
  disclosure handles the still-live remainder. Neither path hides a decision no
  one made.
- `defer`/`resume` is a new authority: it can remove a proposed design from the
  working board. It is therefore attributed and reasoned like accept/reject/
  retire, reversible only by a deliberate `resume`, and never destroys the row.
- Anything reading the ledger for "on the working board" must now filter on
  `deferred_at IS NULL` as well as `retired_at IS NULL` and `superseded_by IS
  NULL`. That is a third orthogonal liveness fact — but it is the same shape as
  the two already there, so it is one more instance of a known rule, not a new
  vocabulary (ADR-0059).
- Migration adds three nullable columns to `design_items` and an append-only
  `design_actions` audit with one attributed row per affected design. Existing
  rows read as not deferred, so nothing changes on the board until someone
  defers something; a later `resume` clears the working projection without
  erasing who resumed it or why.
- The batch `ids` form on `design_decide` is additive; the single-`id` form is
  unchanged, and mixing both in one call is rejected rather than silently
  preferring one.
- This ADR is code — `lodestar-core` (schema + `design_decide`/`design_query`),
  `lodestar-mcp` (the two tool definitions), and the extension's Design Board
  view — plus a `changelog.d/` fragment and `docs/TOOLS.md` rows. The design is
  accepted on the board; the implementation is a pending promotion, so nothing
  here changes behaviour until that work is planned, built, and merged.

## Alternatives considered

**An `archived` status the board hides by default.** The naive request, and the
reason this ADR exists. It repeats ADR-0047/0042/0050 for a fourth time: it
overwrites "was this decided?", and by defaulting to hidden it makes the ledger
understate what is live — parking undecided work where no one will ever return to
decide it. Rejected on principle, not on cost.

**Retire the stale proposals (ADR-0042).** Retirement means the record should not
have existed. A parked proposal *should* exist; it is a real design the
maintainers may yet pursue. Retiring it to clear the board loses that distinction
and misattributes a renumbering-grade "this was a mistake" to work that was not
one. `defer` exists precisely to be the honest verb retirement is not.

**Reject the stale proposals.** A durable no. Some of the seventy-seven deserve
it and should get it — but forcing *every* not-now proposal through `reject` to
clear the board manufactures decisions that were not made, the same lie as the
hidden status wearing the opposite mask.

**View-only: progressive disclosure and nothing else.** Cheaper, and it handles
the "simply many" half well. But it leaves no honest home for the genuinely
stale proposal: without `defer`, the only ways to quiet a parked row are to
reject it (overstates), retire it (wrong), or hide the whole class by default
(the rejected status). Progressive disclosure is necessary and is adopted here —
it is just not sufficient on its own.

**A per-user "hidden" flag in the extension, not the ledger.** Keeps the ledger
clean, but a decision to park a design is a repository fact, not a per-checkout
preference: another maintainer opening the board should see that a proposal was
parked, by whom, and why. Storing it only in the extension would make the same
board show different live sets to different people, which is the multi-worktree
divergence ADR-0042 was careful to avoid.
