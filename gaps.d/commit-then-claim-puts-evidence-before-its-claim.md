- **Commit-then-claim puts the evidence before the claim that authorises it —
  MEASURED, OPEN by design, but the cost is real.** `check_conformance` bounds
  evidence by a claim, so work committed before the task was claimed can never
  be certified: the moment it happened was not authorised by anything. Measured
  on `task:36fa0badd713`, committed at 05:49:36 and claimed **fourteen seconds
  later**, so even its original window excluded its own first commit.

  This replaces the earlier "a lapsed claim can never certify the work" entry,
  whose other two causes are now closed. ADR-0048 made a same-owner re-claim keep
  `claim_started_at` and record the hole, so a lapse alone no longer strands
  work; ADR-0076 made conformance judge evidence against the window that
  authorised it, walking the audited recovery chain, so a **different** owner
  recovering a stranded claim no longer strands it either. Verified end to end on
  `task:219184500419` and by a red probe on the recovery path respectively.

  What is left is not a defect to patch. Admitting evidence from before any
  claim would mean accepting unclaimed time, which is the guarantee ADR-0009
  exists to provide rather than an inconvenience around it — an agent could
  claim a task after the fact and certify whatever preceded it. The remedy is
  the ordering rule that already exists: claim before the first commit.

  It stays recorded because the ordering is easy to breach by accident and
  expensive when breached. Commit-then-claim-then-push is the natural shape of
  the work, the 300-second default lease is far shorter than most tasks, and the
  failure is discovered at the end — after the work is done, when the only exits
  are a human `resolve_task` or abandoning provable work. If that keeps
  happening, the honest fix is to make claiming first easier or to make the
  breach visible at commit time, not to loosen what evidence means.
