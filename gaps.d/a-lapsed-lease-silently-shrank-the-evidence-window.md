- **A lapsed lease silently shrank the evidence window a task can prove —
  FIXED (ADR-0048).** — Observed Jul 2026 closing ADR-0026 task 4. Building
  three commits took longer than the lease, and the only route back to a live
  claim is `claim_task`, which opened a **fresh** `claim_started_at`. The three
  implementation commits sat outside the new window and the receipt covered only
  the final ADR commit plus its validation run. Filed as "the proof
  under-reports", which undersold it: because the verdict is computed over
  whatever the evidence covers, the only way to get a lapsed task accepted was to
  narrow the interval until it was admitted — and the narrowed interval passed on
  the surviving sliver, returned `aligned`, and sent the task to `done` with
  every governed change made before the lapse never examined at all. Being slow
  could therefore stand down the drift check and produce a clean receipt over
  work nothing had read (the ADR-0015 false-safety shape). — High impact once
  understood: not a wrong verdict, but a confident verdict on a question never
  asked. — Fixed this run. A lapse now holes the window instead of moving it: a
  same-owner re-claim keeps `claim_started_at`, so earlier work stays provable,
  while the task log records the discontinuity — read back by `claim_window`
  (ADR-0064) — and caps conformance at `needs_human`. The cap follows the task,
  not the submitted interval, so shrinking the evidence no longer buys a pass. A
  different owner still opens a fresh window, so reach-back never crosses a
  period somebody else owned the task. `recover_claim` remains restricted to
  *legacy* pre-ADR-0030 owners; it was never the answer here.
