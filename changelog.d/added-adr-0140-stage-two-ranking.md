### Added

- Ackplane's pgvector recall now decides what is worth reporting, not just what
  is nearest. ADR-0140 decision 3's stage two runs over the candidate set
  `Projector::similar_nodes` retrieves: the `kind_prior` the graph already holds
  for a node's kind orders near-ties, the per-query `distinctive_cut` and the
  caller's floor together decide whether anything is reported at all, and the
  score a caller sees stays the raw cosine similarity rather than the internal
  weighted one (decision 4). An unanswerable query now returns nothing instead
  of the least-bad row PostgreSQL happened to order first (ADR-0053) — retrieval
  always returns something, so this is the part that can say no.
  `ackplane-server` takes the discrimination functions from the shared
  `mindleak-model` crate rather than reimplementing them, which is the whole
  reason ADR-0140 decision 5 put them there; the edge carries model types and
  pure functions only, never `mindleak-core` or `lodestar-core`, so Ackplane
  remains a separate deployable rather than a mode of either plane (ADR-0082
  clause 1).
