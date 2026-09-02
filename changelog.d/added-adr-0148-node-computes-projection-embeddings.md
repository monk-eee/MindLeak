### Added

- ADR-0148 settles who runs ADR-0140's embedding pass: an **enrolled node**
  computes embeddings for its own repository's projected nodes and publishes
  them under its enrolled key, in a domain of its own. Ackplane stores and ranks
  them and computes none itself — it gains no HTTP client, no model
  configuration, and no inference cost, so model inference never moves inside
  the service holding the ledger (ADR-0082 clause 1). This follows the path
  knowledge embeddings already take, rather than inventing a second trust model.
  The pass stays optional: a repository that never runs it keeps today's
  recency/decay ranking, and every read surface must report
  `not_yet_embedded` distinctly from "asked, and nothing stood out" — under this
  decision the former is the normal steady state, and ADR-0053 makes the latter a
  real answer.
