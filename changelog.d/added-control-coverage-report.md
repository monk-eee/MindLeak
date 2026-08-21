### Added

- `scripts/control-coverage.mjs` (`make control-coverage`) reports every
  active constraint/invariant clause whose active controls cannot reach its
  declared consequence -- either because it has no active control at all, or
  because every control it has caps below what it declares (ADR-0034's
  ceiling rule). Neither gap is visible from reading the constitution alone;
  this reads the clause and its bound controls together. Report-only, like
  `board-health.mjs` and `binding-audit.mjs`: it binds no control and drafts
  no amendment, because deciding a clause's consequence or its mechanism is
  the judgement ADR-0034 reserves for a human.
