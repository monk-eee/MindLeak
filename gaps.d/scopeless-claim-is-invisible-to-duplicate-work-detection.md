- **A claim that declares no scope is invisible to duplicate-work detection, in
  both directions — OBSERVED 2026-08-11, left OPEN.** Two sessions held the
  same task title at the same time: `task:523510b1663f` and
  `task:b0979f99d856`, both "Implement: ADR-0082: Ackplane is a standalone
  federation service", owned by `session:v1:5874...` and `session:v1:de43...`.
  Nothing told either of them.

  `task_query(view="drafts")` and `view="overlap"` both key entirely on the
  advisory path/symbol scope a claim declares, and declaring one is optional.
  `task:523510b1663f` declared ten paths; `task:b0979f99d856` declared
  `paths: []` and `symbols: []`. So `drafts` for the scope-less task returned
  `[]` — a task with no declared scope can never be told it collides with
  anyone — and `drafts` for the scoped task proposed a single question about
  `task:93473480a526` (a different ADR) on a `Cargo.toml` intersection, never
  mentioning the session working under its own title.

  The ledger already holds the answer. `view="existing_work"` exists precisely
  to say "has this already been done?", and identical title plus identical goal
  plus two live owners is the strongest duplicate signal available. Nothing
  consults it at claim time, so the check that would have caught this is one an
  agent has to think to run, about work it does not yet know exists.

  Impact is high and it is the product's own claim: preventing two agents from
  building the same thing is what the Intent Plane is for, and here it did not.
  Measured alongside this, three claims had lapsed for 5-7 hours (~19 hours of
  stalled work) across the same Ackplane effort, so the duplication had time to
  become real code on two branches — `feat/ackplane-node-protocol` and
  `feat/ackplane-node-protocol-isolated` both carry node-protocol work.

  Left for later, deliberately. Making scope mandatory, or widening the
  collision signal to same-title/same-goal, changes what ADR-0055 defined a
  draft to be and what ADR-0024 promised a scope would cost; that is reviewed
  design work, not a fix to slip in beside an observation. Observed while
  looking for claimable work after completing an unrelated task.
