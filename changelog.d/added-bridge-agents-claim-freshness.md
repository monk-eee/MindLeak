### Added

- The Bridge's Agents view and repository claims API now surface how long
  each delegated claim has been held and how many times its lease has
  lapsed (`claim_started_at_seconds`, `claim_lapses`). The standalone
  `/agents` dashboard shows a new "Started" column with a human-readable
  age (e.g. "2h ago") and a "· recovered N×" suffix once a claim has
  lapsed. Both values were already tracked by the `delegated_claims`
  table (ADR-0052: a lease is a heartbeat, not a deadline) but were not
  previously exposed through the Fleet read models or the Bridge API.
