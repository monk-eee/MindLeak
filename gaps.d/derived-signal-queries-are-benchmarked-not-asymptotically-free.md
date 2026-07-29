- **Derived signal queries are benchmarked, not asymptotically free.** — Evidence
  is computed per edge from graph state; a 200-edge snapshot measured 16.757 ms
  p95, but much larger dense graphs may need batched SQL/materialized raw
  provenance. — Low current impact. — Left as a measured scaling boundary.
