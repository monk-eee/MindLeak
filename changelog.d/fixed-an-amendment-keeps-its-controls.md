- **Amending the constitution no longer disarms the controls enforcing it.** A
  clause copy takes a new id (`goal:{slug}@{version}`), and controls store the
  clause id they were registered against, so every amendment used to leave its
  controls pointing at a row that had just been superseded. Nothing refused
  that. The orphaned control went on accepting observations and went on
  answering `pass` and `fail` — only the effective consequence quietly
  collapsed to `advise`, and `clause_controls` reported the live clause as
  unguarded. A control that has stopped enforcing reads exactly like one that
  works, and it happened at the moment somebody was strengthening a rule, which
  is the worst possible time to silently stop enforcing it. Active controls are
  now carried across with the clause inside the amendment transaction, matched
  on slug rather than on the outgoing version's ids — so a control stranded by
  an earlier amendment is adopted by the next one rather than staying orphaned
  forever. Retired controls are deliberately left where they are: they are a
  record of what once enforced a rule, and moving them would rewrite that record
  onto a clause they never guarded.
