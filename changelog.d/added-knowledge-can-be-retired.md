- **A lesson that is wrong or has been replaced can now be retired, instead of
  waiting out its half-life.** `prune_knowledge` only removes what decayed, so a
  superseded record kept being counted and kept competing for the capped
  goal-advisory slots until its clock ran out — which meant
  `scripts/silent-knowledge.mjs` reported a backlog that doing the work could
  not reduce, and `--check` could never gate on it. The new `retire_knowledge`
  tool records who ended a lesson and why, and optionally the record that
  replaced it. Retiring is not deleting: the statement and its provenance stay
  readable, but the record leaves the active set, so the conformance advisory
  stops carrying it and the audit stops counting it. Existing databases gain the
  columns by migration.
