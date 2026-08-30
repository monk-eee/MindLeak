- `claim_window` now reports whether the current evidence window replaced an
  earlier one, and if so whose it was and whether the owner changed. A window
  opened because an agent's id changed underneath a single session — the
  identity-collapse incident where a live process running a superseded binary
  formatted its id differently — reset the counters and reported `lapses: 0`,
  identical to a first claim. Work committed under the earlier window then fell
  outside the current one and could not be certified, and nothing said why.
  Derived from the task log like the rest of `ClaimWindow`, never stored.
  Certification behaviour is unchanged: replacing a window is legitimate (a
  release and re-claim, a recorded handover), so this reports the fact rather
  than adding a refusal.
