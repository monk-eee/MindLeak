- **A lapsed claim can never certify the work it was claimed for — ROOT CAUSE,
  OPEN.** The four traps below are real, but they are symptoms. Underneath them
  is a rule that no amount of care gets past: `check_conformance` requires
  `evidence.started_at >= task.claim_started_at`, and *every* route back to a
  live claim sets `claim_started_at` to now. `claim_task` does.
  `recover_claim` does (`SET status = 'claimed', ..., claim_started_at = ?4`
  with `now`). `renew_lease` refuses outright — a lapsed lease cannot be
  renewed. So the evidence window can only ever begin after the recovery, and
  the work happened before it. There is no ordering of these calls that works.
  — Reproduced end to end on `task:36fa0badd713`, whose commit
  `64fb56b3` is on `main`: the commit was ingested with its true timestamp, the
  window was bounded to the commit itself, and the bundle came back exactly
  right — one commit, three changed nodes, no contamination. `check_conformance`
  answered `invalid: evidence interval falls outside the live claim`. — A second
  edge makes it worse: that task was committed at 05:49:36 and claimed
  **fourteen seconds later**, so even its *original* claim window excludes its
  own commit. Commit-then-claim-then-push is the normal shape of the work, which
  means the evidence for a task routinely predates the claim that authorises it,
  and the 300-second default lease is far shorter than the work. — Impact: an
  agent cannot close a stranded claim at all, however carefully. The only exits
  are a human `resolve_task` or abandonment, and the board accumulates claims
  that look like abandoned work but are finished, shipped, merged work. Thirty-two
  such claims are on the board today. — Status: not fixed, and deliberately not
  worked around here. The honest fix needs the task's claim history rather than
  the two scalar aggregates that replaced it, which is exactly what ADR-0064
  (the log is the ledger) is for: with a real transition log, "evidence that
  falls inside a *prior* claim by the same agent" becomes a question the store
  can answer, and completing shipped work stops requiring a human. Anyone
  implementing ADR-0064 should treat this as a requirement of it.
  — **UPDATE, 29 Jul 2026: the absolute claim above is no longer true, and the
  headline overstates what remains.** ADR-0048 landed after this was written: a
  re-claim *by the same owner* now keeps `claim_started_at` and records the hole
  in `claim_lapses` / `unleased_seconds` instead of moving the window
  ([`store/coordination.rs`](crates/lodestar-core/src/store/coordination.rs),
  the `claim_started_at = CASE WHEN status = 'claimed' AND owner = ?2 THEN
  claim_started_at ELSE ?4 END` arm). Verified end to end on
  `task:219184500419`: its lease lapsed twice mid-task, it was re-claimed by the
  same owner each time, and evidence beginning at the *original* claim was still
  accepted by both `check_conformance` and `complete_task`. So a lapse alone no
  longer strands the work. What remains true is narrower: a **different** owner
  still opens a fresh window, and commit-then-claim still puts the evidence
  before the claim that authorises it. Treat the two scalars as the interim
  mechanism, not the absence of one.
