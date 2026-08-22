### Added

- Bridge exposes the Context Graph (ADR-0105 decision 6): `GET
  /api/v1/repositories/:repository_id/graph` wires up
  `Projector::bounded_neighborhood` (ADR-0087) — the same relevance-first,
  depth/fanout-bounded traversal the projection worker already implements —
  to the Bridge for the first time. Optional `seeds` (comma-separated node
  ids); an absent value falls back to a new `Projector::sample_nodes`, the
  most recently touched nodes, so a repository is browsable without already
  knowing a node id. `depth` (clamped 1-4), `max_nodes` (clamped 1-300), and
  `max_fanout` (clamped 1-30) round out the contract. Each edge's
  `effective_weight` is computed at response time from
  `base_weight`/`half_life_hours`/`updated_at` (now carried on
  `ProjectedEdge`), mirroring `mindleak_core::decay::effective_weight`
  exactly — never stored. The repository detail panel gained a "Context
  graph" section listing the neighbourhood's nodes and edges.
