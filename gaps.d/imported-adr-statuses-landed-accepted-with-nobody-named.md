- **Imported ADR statuses landed `accepted` with nobody named as the decider —
  FIXED (ADR-0051), ledger fully signed.** — Found Jul 2026 by the first
  run of `make design-audit`. `reconcile_designs` imports the status out of the
  ADR file, so a large part of the ADR history landed as `accepted` with
  `decided_by` null. They read as decided and are not: nobody approved them
  through Lodestar.
  **Two earlier versions of this entry were wrong, in opposite directions, and
  both are worth keeping written down.** The first said the repair was
  mechanical — reopen, then accept — and attempting it on all of them is how we
  found out otherwise: the stuck rows were all `promotion_status = materialized`,
  and `reopen_undecided_design` deliberately refuses a row whose promotion has
  left `not_required` (ADR-0047), because materialized work rests on that
  acceptance. The second concluded from that the rows were unrepairable, which
  only held while the missing verb did not exist.
  [ADR-0051](docs/adr/0051-a-decision-already-made-can-still-be-signed.md) adds
  it: `attribute_design_decision` records who made a decision the ledger already
  asserts, changing status, reason and promotion state not at all. It takes
  precisely the rows `reopen_undecided_design` refuses, so the two verbs
  partition the undecided rows rather than overlapping, and no guard had to be
  softened to let the repair through.
  **All 51 design rows now carry a decider — `list_designs` reports zero
  unattributed.** Seven rows that had materialized nothing were reopened and
  decided properly (0015, 0017, 0036, 0037, 0039, 0040, 0046); the rest,
  including the founding ADR-0001..0014, 0016, 0019, 0025 and 0032, were signed
  with the new verb.
  One residue remains, and it is permanent: the decider name is free text and
  nothing normalises it, so the same person appears as `monk-eee` (49 rows),
  `monk-ee` (1) and `Lyndon Swan` (1). A typo there cannot be corrected —
  `attribute_design_decision` refuses a row that already has a name, by design,
  so a wrong name cannot be quietly rewritten into a different one. Correcting
  one is superseding a real decision and needs its own verb.
  This is not only inherited history. `DesignBoardController.sync()` calls
  `reconcile_designs` over the whole ADR directory, so **every ADR merged with
  `Status: Accepted` already written in its file becomes another undecided row
  on the next sync.** Stopping the inflow is a convention question: an ADR
  authored here would land as `Status: Proposed` and be accepted through the
  Design Board, so the file follows the decision instead of asserting it.
  Earlier this session the ledger was described as "fully remediated" after the
  `proposed` rows were cleared; that was wrong, and only checking a second
  property caught it.
