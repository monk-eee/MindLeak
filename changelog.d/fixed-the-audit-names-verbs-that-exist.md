- The design audit's remediation advice now names verbs the server has, and the
  right one. For a row the ledger accepts but credits to nobody it said
  "reopen_undecided_design then accept_design": both names were retired when the
  design cluster collapsed to `design_register` / `design_decide` /
  `design_promote` / `design_query`, and the remedy was wrong as well. ADR-0051
  added `attribute` for exactly that row — it records the decider and leaves
  status, reason and promotion state untouched, and it takes precisely the rows
  `reopen` refuses. Following the old advice would have discarded six
  acceptances that already held and sent them back to proposed, which is a
  bigger act than the defect being repaired. Supersession advice and two stale
  `list_designs` references are corrected the same way.
  A test now scans the audit for every retired design verb, so advice naming a
  tool nobody can find fails the suite instead of misleading the next reader.
