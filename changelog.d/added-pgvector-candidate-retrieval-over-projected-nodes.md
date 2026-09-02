- **pgvector-backed candidate retrieval over the graph projection (ADR-0140).**
  `Projector::similar_nodes` ranks a repository's `projected_nodes` by
  pgvector's `<=>` cosine distance, computed entirely inside PostgreSQL rather
  than pulled into application memory for a cosine loop over every stored
  vector. This is the read half of ADR-0140: the write half already shipped
  (`0055_projected_node_embeddings.sql` and the embedding pass that populates
  it), but nothing ever read those embeddings back, so a federated repository
  was computing and storing vectors that could not affect a single answer.

  This is stage one only — bounded candidate *retrieval* (ADR-0140
  decision 3). The `kind_prior`/`distinctive_cut`/floor pipeline that decides
  what is actually worth reporting is a separate slice, and until it lands a
  caller must not treat a returned candidate as a recall answer.

  A model nothing was embedded under returns an empty candidate set rather
  than an error (decision 1), so a deployment with no embeddings yet degrades
  to the existing recency/decay ranking (ADR-0080) instead of failing.
