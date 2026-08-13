- **Added:** `ackplane-server` gains a `projection` module implementing
  ADR-0087 clauses 1, 2, 3, 6, and 10: `projected_nodes`, `projected_edges`,
  and `projection_state` tables (idempotent migration), and
  `Projector::rebuild`, which replays a repository's committed
  `structural_fact` ledger records in stream order into those tables inside
  one transaction. Dropping and rebuilding a projection from the same ledger
  reproduces it exactly, and a rebuild-and-diff test proves it.
  `Projector::bounded_neighborhood` reads the projection back with an
  ordinary recursive-CTE traversal honouring the same bounded, best-first
  contract `mindleak-core`'s `GraphStore::bounded_neighborhood` already
  implements — seed set, max depth, per-node fanout limited to the strongest
  edges by effective weight, dangling edges dropped — with effective weight
  computed in the query, never stored, and the projection's ledger position
  and rebuild time returned alongside every answer so a stale projection is
  legible. Apache AGE and pgvector remain out of scope, matching ADR-0087
  clauses 4 and 5. As with the ledger schema, tests that need a real
  PostgreSQL connection are opt-in via `ACKPLANE_TEST_DATABASE_URL` and skip
  cleanly when it is unset (ADR-0088 clause 2).
