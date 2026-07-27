# ADR-0042: A design is retired by a person, never by a missing file

- Status: Accepted
- Date: 2026-07-27
- Deciders: MindLeak maintainers
- Related: [ADR-0019](0019-task-retention-and-board-hygiene.md) (archive, not
  delete), [ADR-0023](0023-design-board-accept-bridge.md) (design review
  workflow), [ADR-0038](0038-isolated-worktrees-shared-repository-state.md)
  (isolated worktrees, shared repository state)

## Context

`reconcile_designs` is upsert-only and keys on `adr_path`. Renaming an ADR
therefore registers a new design and orphans the old one, permanently. The
repository currently carries two such rows —
`design:0036-one-work-surface` and `design:0037-one-work-surface` — left behind
when one decision was renumbered twice on its way to ADR-0040. Neither path
exists on any branch. Every Design Board row is wired to
`mindleak.design.openAdr`, so clicking either throws.

There is no way to remove them. The design ledger has no retirement path at all:
a row, once registered, is forever.

The obvious fix is to retire any design whose ADR file is absent from the
working tree. **That fix is wrong, and dangerously so.** Under ADR-0038 several
worktrees on different branches share one `spec.db`. A branch that predates an
ADR, or a checkout of an older commit, legitimately lacks files that are very
much alive elsewhere. "Absent from this checkout" is a routine condition, not
evidence of anything. Auto-retiring on it would delete live decisions — turning
a cosmetic annoyance into exactly the loss the ledger exists to prevent, and
doing it silently, in the background, on someone else's branch.

The same reasoning rules out having the extension retire designs it cannot find
during `sync()`. The extension sees one set of working trees; the ledger spans
the repository.

## Decision

1. **Retirement is an explicit human act.** `retire_design(id, human, reason)`
   marks one design retired, attributed to the person and carrying a rationale.
   Nothing infers it, and no background process performs it.
2. **A missing file is never evidence.** No code path retires a design because
   its ADR is not on disk. `reconcile_designs` stays upsert-only and continues
   to leave unknown rows untouched.
3. **Retiring is not deleting** (ADR-0019). The row keeps its id, path, decision
   status, decider, and materialization history; it gains `retired_at`,
   `retired_by`, and `retired_reason`. A conformance record or materialization
   that referenced the design keeps naming something that still exists.
4. **Retirement is orthogonal to the decision status.** `proposed`/`accepted`/
   `rejected` records what a human decided *about the design*. Retirement
   records that the *record itself* is no longer a live entry — usually because
   its path was superseded. Encoding retirement as a fourth status would
   overwrite the decision and make "was this accepted?" unanswerable.
5. **Retired designs leave the working board and stay in history.**
   `list_designs` omits them by default and returns them under an explicit
   `include_retired`, so the board shows live decisions while the audit trail
   remains complete.

## Consequences

- The two ghost rows can be retired with an attributed reason naming the
  renumbering, and the Design Board stops offering unopenable rows.
- A renamed ADR still produces a new row plus a stale one. This decision makes
  the stale row *removable*, not impossible; detecting renames automatically
  would mean trusting file absence, which point 2 forbids.
- `retire_design` is a new authority: it can hide a design from the board. It is
  therefore attributed and reasoned like accept/reject, and reversible only by a
  deliberate follow-up — the row is never destroyed.
- Migration adds three nullable columns to `design_items`. Existing rows read as
  not retired, so no behaviour changes until someone retires something.
