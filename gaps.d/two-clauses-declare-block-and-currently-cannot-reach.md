- **Two clauses declare `block` and currently cannot reach it, because an earlier
  amendment orphaned their controls — MEASURED, fix landed but not yet
  retroactive.** Measured across the live constitution: **30 active clauses, 13
  with a complete contract, and only 4 binding any control — 2 of them
  mechanical.** `clause_controls` reports
  `one-publishing-owner-per-task-branch` and
  `a-commit-stays-inside-its-declared-scope` as unguarded, though both still
  declare `block`. A clause copy takes a new id and controls stored the old one,
  so amending a rule silently disarmed it. Impact is narrower than it looks and
  worth stating precisely: the *mechanisms* never stopped working — the
  pre-commit hooks still exit non-zero and still refuse the commit — but the
  *ledger* cannot resolve those clauses above `advise`, so a conformance verdict
  will not report a violation of them. Do not read "no control" as "no
  enforcement", or "declares block" as "will block". Amendments now carry active
  controls across by slug, which re-adopts the stranded ones at the next
  amendment; until an amendment happens, the four above are the complete list of
  clauses that enforce anything.
