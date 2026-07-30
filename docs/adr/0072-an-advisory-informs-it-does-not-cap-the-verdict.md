# ADR-0072: An advisory informs; it does not cap the verdict

- Status: Proposed
- Date: 2026-07-30
- Amends: [ADR-0022](0022-learned-knowledge-loop.md) §4
  (consolidated knowledge advises conformance)
- Related: [ADR-0060](0060-work-whose-product-is-not-code-must-still-conform.md) (a finding is not a
  verdict), [ADR-0048](0048-a-lapsed-lease-holes-the-window-it-does-not-move-it.md) (a holed
  window cannot certify itself), [ADR-0009](0009-evidence-backed-conformance.md)
  (evidence-backed conformance)

## Context

ADR-0022 §4 let consolidated knowledge nudge an otherwise-`Aligned` verdict to
`NeedsHuman`. The rule was deliberately bounded — knowledge may never emit
`Violation` or harden a verdict — and the reasoning was sound: a proven
regularity touching the code you just changed is worth a second look.

The trigger, however, is that any active knowledge node **references** one of
the evidence's changed nodes. Knowledge only accumulates. The set of referenced
nodes therefore only grows, and the nudge has become unconditional in practice.

Measured against the live board on 2026-07-30, of 190 `done` tasks only 58
(31%) carry a receipt that affirms the work. The shape is not accumulated debt;
it is a collapse with a date on it:

| Closed on | affirmed | `needs_human` | `drift` |
|---|---|---|---|
| 07-23 | 28 | 0 | 0 |
| 07-24 | 14 | 5 | 4 |
| 07-27 | 3 | 19 | 2 |
| 07-28 | 3 | 30 | 1 |
| 07-29 | 9 | 38 | 21 |
| 07-30 | 1 | 8 | 4 |

The fleet affirmed 28 of 28 completions on 23 July and 1 of 13 on 30 July. That
single survivor earned it for one reason: `advise` reported *"no active clause
governs this change"*, so no knowledge bound to a changed node could fire. **The
only reliable way to obtain an affirming receipt had become changing code that
nothing governs** — the exact inverse of what conformance is for.

The cost is not cosmetic.

- A task that lands `in_review` does not reach `done` on its own, and a
  successor declared with `blocked_by` opens only on an **aligned** completion.
  A permanent cap therefore freezes dependent work; `task:9d66c8997336` sat
  behind exactly this.
- A receipt whose only substantive finding is positive, capped anyway, teaches
  every reader that the verdict does not track the work. A gate that always says
  the same thing stops being read, and then it is not a gate.

ADR-0060 item 2 already states the principle this violates: only a positive
signal of a **problem** may downgrade a verdict. Topical overlap between a
changed node and a recorded lesson is *relevance*. It is precisely the case for
showing the agent the lesson, and precisely not the case for doubting the work.

## Decision

**The knowledge pass attaches advisory findings and does not alter the verdict.**

1. Every matching knowledge node still contributes its `advisory: learned
   knowledge <id> — <statement>` finding. Surfacing the lesson at the moment of
   change is the whole value ADR-0022 was reaching for, and it is preserved
   intact.
2. The pass may not move `Aligned` to `NeedsHuman`, and — as before — may never
   harden a verdict or emit `Violation`. Verdicts belong to the base pass: goals,
   the Constitution, and the evidence-window rules.
3. ADR-0022 §4 is amended accordingly. The bound it placed on knowledge ("advise,
   never hard-fail") stands; what is withdrawn is the residual power to downgrade.

## Consequences

- Receipts discriminate again. An `aligned` verdict means the base pass found
  nothing wrong, rather than meaning nobody had yet written a note near the file.
- Blocked successors open on merit. The `blocked_by` handoff becomes usable
  again, because an ordinary good change can reach `aligned`.
- **A second look is no longer forced.** This is the real trade. If a class of
  knowledge genuinely should stop a completion, it needs to say so as a problem
  signal — a constraint or invariant clause with a declared consequence, which is
  the machinery that already exists for exactly that and which carries an
  attributable human decision. Relevance alone will not do it, and should not.
- The historical receipts are unchanged. Verdicts already recorded stay as they
  were; this alters only checks made from here.

## Alternatives considered

- **Keep the nudge and prune knowledge harder.** Treats the symptom. The trigger
  is reference, not staleness, so any knowledge base large enough to be useful
  reproduces the problem — and pruning useful lessons to restore a verdict is a
  bad trade in both directions.
- **Nudge only on knowledge above a confidence or recency threshold.** Invents a
  number nobody agreed, and still downgrades on relevance rather than on a
  problem. It would have delayed this collapse rather than prevented it.
- **Leave it and rely on human resolve.** This is the status quo, and it is what
  the measurement is describing: 58 human resolutions on 29–30 July alone, most
  of them rubber-stamping work whose only substantive finding was positive. That
  is not review, it is toil that trains people to click through.
