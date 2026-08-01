- **A partly-warmed knowledge index no longer claims to be fully semantic, and
  the index warms itself.** `active_knowledge`'s semantic `query` (ADR-0080)
  shipped with an index that only filled on write, so a repository that had
  already learned anything searched by substring forever, and once a single
  lesson was embedded the reply called itself `semantic` while every unembedded
  lesson was silently pinned last. Now a search backfills missing embeddings in
  one bounded batch, ranking covers only what was actually embedded, and the
  reply reports `ranked_by_meaning` with a note whenever part of the list is
  still in weight order. The embeddings client also validates the model's
  response — vector count, per-input `index`, dimension consistency and
  non-finite components are refused rather than quietly reshaped into a vector
  that scores against everything.
