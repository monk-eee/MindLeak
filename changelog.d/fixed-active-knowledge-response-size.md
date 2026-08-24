- **`active_knowledge` no longer returns every active lesson in one reply.**
  An unfiltered call on a long-lived repository had no bound at all: every
  active statement's full text, once per record, in a single reply. Measured
  on this repository's own ledger, that produced multi-hundred-kilobyte to
  megabyte-scale results — which a chat client's session store then persists
  (and has been observed to embed more than once per turn), turning ordinary
  agent recall into a real contributor to extension-host and renderer memory
  pressure. The `knowledge` array returned is now capped at the strongest 50
  entries (already the return order: weight, semantic rank, or substring
  match); a new `knowledge_truncated` field says whether more exist. `count`,
  `never_surfaces`, and `reaches_by_goal_only` are computed before the cut and
  still describe the full matching set, so repo-health tooling reading those
  fields is unaffected.
