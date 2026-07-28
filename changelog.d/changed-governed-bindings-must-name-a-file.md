- **The conformance gate now reports governed bindings that name no file
  (ADR-0031).** Splitting, renaming, or deleting a governed file moves the code
  and leaves the binding pointing at a path that no longer exists. Nothing
  failed when that happened: the constitution simply stopped governing the code,
  `advise` found no clauses for the new paths, and the loss was invisible —
  an orphaned binding looks exactly like code that was never governed. Measured
  on this repository after a refactor campaign: **7 governed ids named files
  that no longer existed**, including `graph/query.rs` and `graph/signal.rs`,
  split hours earlier by the very campaign that unbound them. The gate cannot
  catch this by watching diffs, because an orphaned id never appears in one; it
  now checks every governed binding against the working tree and reports the
  ones that resolve to nothing. Advisory alongside the existing receipt check,
  and failing under `--strict`.
