- **The `evidence_for` false alarm did not recur.** The Known gaps entry
  recording it asked to be revisited if it were seen again; it was not. Four
  `evidence_for` calls across four tasks on 2026-07-29, in a session independent
  of the one that raised it, each returned the commits they should.
  The shape they shared is the useful part, and it is now written down: the
  commit ingested by an explicit `ingest_commit` carrying its true author
  timestamp, attributed to the session, and a window opened at
  `claim_started_at` *before* the commit existed. Every case the original entry
  was written about had the window opening *after* the work — which is a
  claim-ordering problem, not an evidence-query one, and points at a different
  fix from the one a reader of the original entry would have reached for.
  Docs only. Nothing is retracted: the disproof already on record stands, and
  this only settles the "until seen again" it left open.
