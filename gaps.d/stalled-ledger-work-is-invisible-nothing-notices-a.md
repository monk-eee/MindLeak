- **Stalled ledger work is invisible: nothing notices a lapsed lease or a
  shipped change with no receipt — OPEN.** — Found Jul 2026 auditing why three
  tasks sat unfinished. They stalled for three *different* reasons and the board
  reported none of them:
  1. **A lapsed lease produces no signal.** `task:c3ef672e0ae3` (fleet view) was
     built and opened as a pull request, but `check_conformance` was never
     called and the lease simply expired. Its only conformance record is the one
     written during a later audit. The work exists in Git and does not exist in
     the ledger, and nothing anywhere says so.
  2. **Work that ships outside a claim window can never be certified.**
     `task:92778f8ad0f5` was delivered under an earlier pull request, so every
     honest evidence window for it is empty and the verdict is necessarily
     `needs_human`. This is the evidence contract behaving correctly — it
     refuses to certify what it cannot bound — but the task then waits on a
     human with nothing prompting one.
  3. **Cross-cutting work reads as `drift`, and by design cannot be repaired
     afterwards.** `task:05dade200195` ran the full loop with real evidence (2
     commits, 11 artifacts, complete provenance) and still resolved `drift`:
     *"governed code changed without a covering task"*, naming two goals other
     than its own. ADR-0041's `also_serves` is the answer, but it is fixed at
     creation with no later mutator — deliberately, because coverage added once
     conformance has complained is a rationalisation. So the only exit is human
     judgement.
  Blocked work then queues behind these silently: `task:0bcbb4220bcc` waited 78
  hours on (2), with zero conformance records of its own, and the fleet-overlap
  chain waited on (1). — Medium-to-high impact: no state is wrong and nothing is
  lost, but the board looks idle while three finished pieces of work sit
  uncertified, and the only way to find out is to go looking. It is the same
  shape as the ADR-loss problem — silent, and caught by accident. — Left for
  later; the fix is a read-only stall report (lapsed leases, `in_review` older
  than a threshold, tasks blocked by something already terminal or `in_review`)
  rather than any change to the evidence contract, which is behaving correctly
  in all three cases. Note that (1) is the only one that is purely mechanical;
  (2) and (3) are rules working as intended and want a human, not a fix.
