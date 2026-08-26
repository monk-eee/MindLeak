- Fleet and Readiness no longer report a healthy repository as permanently
  `Lagging` / `AttentionNeeded`. Both classified projection freshness by
  comparing the projection checkpoint against `stream_heads.position` — the
  head of *every* record in a repository's ledger — but a projection only ever
  consumes `structural_fact` records and checkpoints itself at the last one it
  projected. The projection worker's own staleness query filters to that same
  payload type, so once any evidence, knowledge, claim, directive, or
  delegation record landed after the last structural fact, the warning could
  never clear: the worker saw nothing to rebuild and the gap it was being
  judged against could never close. Since all of those records share the one
  ledger, this was the normal operating case rather than an edge case, and it
  also affected Fleet's server-side `freshness=lagging|fresh` filter, which
  re-derived the same comparison in SQL. Freshness is now classified against
  the head of the stream the projection actually consumes, so Fleet, Readiness,
  and the projection worker agree on "caught up" by construction. The
  whole-ledger head is still reported as `ledger_stream_position`; it is the
  honest answer to how many records a repository has published, and only the
  comparison changed.
