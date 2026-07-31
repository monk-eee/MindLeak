- **A rescue claim silently erases the path scope that certifies the rescued
  work.** `task_claim(step="claim")` treats an omitted `paths` argument as an
  instruction to set the scope to empty, not as "leave the existing scope
  alone". Rescuing a lapsed claim therefore destroys the declared scope the
  original owner set, and `merge_evidence` — the one tool that can certify work
  which already landed (ADR-0058) — then refuses with `task declared no path
  scope, so nothing ties <commit> to it`. Measured 2026-08-01 on
  `task:56b993a45db5` (ADR-0077, delivered by PR #330, merge
  `51e0f5f7a17fc6791795071604c1f084280efbc2`): the owner had declared 23 paths,
  the lease lapsed for roughly ten hours, and a rescue claim that omitted
  `paths` left scope `[]`. — Impact: the rescue path defeats itself. The act of
  taking abandoned work removes the linkage needed to prove that work landed,
  and the original scope is unrecoverable from the task afterwards — it survives
  only if somebody read it before claiming. The board keeps advertising finished
  work as in-progress, which is the state the rescue was meant to clear. —
  Worked around, not fixed: the scope was restored by re-claiming with the 23
  paths read from an earlier `task_query view=board`, after which
  `merge_evidence` verified the merge against 26 in-scope paths. A fix should
  either preserve the prior scope when `paths` is omitted, distinguish "not
  supplied" from "supplied empty", or refuse a scope-narrowing re-claim on a
  task that already declared one.
