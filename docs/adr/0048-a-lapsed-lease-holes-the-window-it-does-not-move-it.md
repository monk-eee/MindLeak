# ADR-0048: A lapsed lease holes the evidence window, it does not move it

- Status: Accepted
- Date: 2026-07-27
- Deciders: MindLeak maintainers
- Refines: [ADR-0009](0009-evidence-backed-conformance.md) (evidence-backed
  conformance), [ADR-0025](0025-authoritative-checked-conformance.md)
  (authoritative checked conformance)
- Related: [ADR-0015](0015-advisory-symbol-leases.md) (false safety is worse
  than none), [ADR-0020](0020-task-lifecycle-states.md) (task lifecycle states),
  [ADR-0034](0034-typed-controls-and-enforcement-ceilings.md) (typed controls
  and enforcement ceilings), [ADR-0041](0041-cross-cutting-work-is-declared.md)
  (declared breadth cannot self-certify)

## Context

A claim carries an evidence window. `claim_started_at` marks its start, the
lease expiry marks its end, and `validate_claim_evidence` refuses any evidence
interval reaching outside it. The bound is the right idea: it stops an agent
proving work it did not do under a claim it did not hold.

A lease is a heartbeat, and heartbeats stop. `renew_lease` refuses a lapsed
lease, so the owner must re-claim — and re-claiming reset `claim_started_at` to
the moment of the re-claim:

```sql
claim_started_at = CASE
    WHEN status = 'claimed' AND owner = ?2 AND lease_expires_at >= ?4
    THEN claim_started_at ELSE ?4 END
```

The window did not merely lose its start. It *moved*. Everything the agent had
done before the lapse fell outside the interval it was allowed to submit, and
the only error it saw was `evidence interval falls outside the live claim`.

The obvious reading of that is "the receipt under-reports", which sounds like an
accounting nuisance. It is worse than that, because the verdict is computed over
the nodes the evidence covers. An agent that lapsed had exactly one way to get
its work accepted: narrow the interval until it was admitted. The narrowed
interval passes conformance on the surviving sliver and returns `aligned`,
`aligned` transitions the task straight to `done`, and every governed change
made before the lapse is never examined by anything.

So the mechanism that exists to catch drift could be stood down by an agent
simply being slow, and the resulting receipt asserted a clean bill of health
over work nothing had read. That is the ADR-0015 false-safety shape: a check
that reports success on a question it never asked.

## Decision

**A lapse punches a hole in the evidence window. It does not move the window.**

1. **The window survives a lapse.** A re-claim by the same owner keeps
   `claim_started_at` where it was, so work done before the lapse stays
   provable. A claim by a *different* owner still opens a fresh window: reach-back
   must never cross a period somebody else owned the task, and letting the window
   survive only same-owner re-claims enforces that structurally rather than by
   comparing intervals.

2. **The hole is counted, on the task.** `claim_lapses` records how many times
   the lease lapsed inside the current window and `unleased_seconds` records how
   much of that window was held under no lease. Both are set in the same guarded
   compare-and-swap that performs the claim, so a claim and the record of its
   discontinuity cannot disagree. Both reset when a fresh window opens, because
   they describe the current window, not the task's whole history — the durable
   record of an ownership change remains `task_claim_transfers`.

3. **A discontinuous window cannot certify itself.** If `claim_lapses > 0`,
   conformance caps the verdict at `needs_human` and the finding names the lapse
   count and the un-leased seconds. This is the ADR-0034 ceiling rule: a lapse
   means there was a stretch in which the agent held no lease, and nothing in
   the system can tell whether work fell into it. That is an unknown, and an
   unknown gets a human — it is not a pass, and it is not a failure either.

4. **The cap follows the task, not the submitted interval.** It would be more
   precise to cap only when a hole falls inside the interval actually submitted.
   It would also be gameable in exactly the way this ADR exists to close: an
   agent could shrink the interval until the hole fell outside it and recover
   the clean pass. Precision that can be dodged is not precision.

5. **The lapse count joins the conformance token basis.** The window start no
   longer moves on a same-owner re-claim, so without this a lease could lapse
   between `check_conformance` and `complete_task` and the token would still
   match, letting a verdict issued over a continuous window certify a window
   that had since acquired a hole.

## Consequences

- Work done before a lapse is provable again. The common case — an agent that
  went quiet, came back, and finished the job — no longer has to choose between
  discarding its evidence and shrinking it.
- Conformance now reads the whole span, so governed changes made before a lapse
  are examined instead of skipped. This is the point of the change; expect it to
  surface drift that previously went unseen.
- **Lapsed work lands in `in_review` rather than `done`.** A fleet whose leases
  lapse routinely will feel this immediately, and that is the intended pressure:
  the remedy is to renew the lease, which is cheap, or to accept the review,
  which is honest. There is deliberately no threshold below which a lapse is
  forgiven — a tolerance is a knob, and a knob is where this guarantee would
  quietly rot.
- The two counters are additive columns with defaults, so existing databases
  migrate without backfill. Windows already open when the migration runs are
  treated as continuous, because nothing recorded whether they were.
- `unleased_seconds` measures the total hole, not where it is. Locating it would
  need an append-only interval ledger per claim. That is a bigger change than
  this problem justifies, and the ceiling already forces a human to look, so the
  cheaper counter is enough for now.
