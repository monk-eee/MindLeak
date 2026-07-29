- **The design ledger could not say `Superseded` — FIXED; one row still needs a
  decider first.** — ADR-0018 and ADR-0032 declare `Superseded by <ref>` while
  the ledger had only `proposed`, `accepted`, `rejected`, so both sat `accepted`
  and every ledger-driven view showed a withdrawn decision as live.
  [ADR-0050](docs/adr/0050-a-superseded-decision-is-not-a-stale-one.md) gives a
  design the `superseded_by` link the goal model already has, and
  `make design-audit` now reports an unrecorded supersession as drift instead of
  as an unrepresentable note. ADR-0018 → ADR-0032 is recorded.
  ADR-0032 → ADR-0038 cannot be: `supersede_design` requires a recorded
  `decided_by` and ADR-0032 is one of the unattributed rows above, so it needs
  the signing verb first. **Its successor was never actually unknown** — see the
  parser gap below.
