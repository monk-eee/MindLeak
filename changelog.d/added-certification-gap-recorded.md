- **Known gaps now records that an agent can work all day and certify nothing.**
  48 of 101 `done` tasks rest on a `needs_human` receipt rather than an affirmed
  one, thirty-three claims sit lapsed, and an audit against `origin/main` found
  at least nine of those tasks already fully implemented in main — so the board
  cannot distinguish unfinished work from unclosed work, and an agent that
  trusts it re-implements what already shipped. The entry names the measurement,
  the impact, why `check_conformance`'s refusal is correct and must not be
  loosened, and the three candidate repairs. It also corrects the older
  "a lapsed claim can never certify the work it was claimed for" entry, which
  ADR-0048 has since made untrue: a same-owner re-claim keeps `claim_started_at`
  and records the hole, verified end to end on a task whose lease lapsed twice.
