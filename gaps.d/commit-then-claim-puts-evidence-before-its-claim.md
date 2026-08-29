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

  **The commit-time half now exists, 2026-08-29 — the exit is available while
  it is still an exit.** `scoped-commit.mjs` warns *before* creating the commit
  when no live claim of this session covers it, naming the remedy and saying
  plainly that claiming afterwards will not repair it (`uncoveredCommitNotice`).
  That is the piece the paragraph above identifies as missing: the publish-time
  warning is correct but arrives when the only remaining moves are
  `merge_evidence` or a human resolve, whereas at commit time stopping and
  claiming still costs one call and loses nothing.

  It is a warning and not a gate, and that is load-bearing rather than timid —
  ADR-0048's design note is that gating commits on a claim teaches agents to
  invent a task to get past the check, which is how six duplicate tasks reached
  the board. A warning costs a wasted commit at worst; a gate costs a fictional
  task that outlives it. An unreadable ledger is reported as unreadable rather
  than passed over, because "no claim" and "could not tell" must not look alike
  from here.

  **Still OPEN**, and the residual is what it always was: nothing makes claiming
  first *easier*, only more visible, and an agent that reads the warning and
  commits anyway reproduces the original outcome exactly. The underlying rule —
  evidence is bounded by the claim that authorised it — is ADR-0009's guarantee
  and is not a defect to patch.

  **Visibility half shipped, gap itself unchanged.** `canonical-push.mjs` now
  warns — advisory only, never blocking — when a branch being published
  carries a commit whose own timestamp predates every claim this session
  holds, naming the affected commits and this fragment. It does not make
  claiming first any easier, and it fires at publish time rather than commit
  time (deliberately: ADR-0048's own design note is that gating at commit time
  teaches people to invent tasks to get past the check, and a warning that can
  only be seen after the push offers no earlier exit anyway). The underlying
  limitation is exactly as open as before; what changed is that the breach is
  now reported instead of discovered later at `complete_task`.
