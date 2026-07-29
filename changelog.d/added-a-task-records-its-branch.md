- A task now records the branch its evidence window is being done on (ADR-0057).
  The value is joined at claim time from what the claiming session already
  declared to `open_session`, so nobody is asked to declare anything twice, and
  a session that declared no branch records nothing rather than a guess — the
  server never inspects Git (ADR-0044). It follows the window rather than the
  agent: a same-owner re-claim keeps it, so an agent that has since moved on
  cannot silently rename the branch its earlier commits were made on, while a
  claim by a different owner opens a fresh window and re-reads it. Existing
  databases gain the column as NULL, which is the honest record of a branch that
  was never captured. This is the fact a verified merge will be checked against
  (ADR-0058).
