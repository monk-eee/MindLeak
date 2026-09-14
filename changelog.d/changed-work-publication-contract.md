- **Industrial Work keeps synchronous transactional updates:** the approved
  ADR-0120 amendment removes the unreachable Work `lagging` state and defines
  `current`, `claims_only`, `not_published`, and `unavailable` in terms of scoped
  consistency and absence. Per-task consistency checks and unavailable reporting
  remain tracked implementation work; this contract amendment does not claim
  they are already shipped. Structural graph freshness is unchanged.
