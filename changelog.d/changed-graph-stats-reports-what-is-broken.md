- **`graph_stats` now reports what is silently broken, not just what exists.**
  Four capabilities were built correctly and left mute, and each cost hours: the
  publisher enforced claims but recorded no evidence, `AGENTS.md` never named the
  read tools it depended on, the embedding index existed but nothing refreshed
  it, and the build sha was reported but never compared. The common failure was
  not missing capability — it was capability that never speaks up.
  `graph_stats` is the call the fleet already makes constantly, so it is where a
  regression has to announce itself. It now also reports **nodes `recall` cannot
  see** (no embedding for the active model) and **nodes still carrying a split
  identity** (an absolute path in the id). Both rows appear only when the count
  is non-zero: a health row that is always present is one readers learn to skip,
  which is how these stayed invisible in the first place.
  The value was immediate. On first run against the live graph it reported 110
  unembedded nodes and **235 split identities that had reappeared since the
  repair**, because other agents' servers are still running binaries built before
  paths were made repo-relative. That regression was already underway and nothing
  would have said so. A missing embedding index reports every node as
  unrecallable rather than failing, because taking `graph_stats` down would
  remove the one health signal the fleet reads.
