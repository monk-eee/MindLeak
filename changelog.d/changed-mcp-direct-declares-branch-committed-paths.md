### Changed

- `scripts/mcp-direct.mjs` now declares `branch_committed_paths` when a batch
  takes a claim, so ADR-0147's `branch_inherited` report is actually reachable
  from the documented direct-drive path. It runs `git diff --name-only
  <base>...HEAD` — measured from the `base` the batch's own `open_session`
  declared, defaulting to `origin/main` — and Lodestar answers with the clauses
  governing work already on the branch that the task's own scope does not
  cover, so they can be declared with `also_serves` before the work rather than
  discovered as `drift` at completion. Slices 1 and 2 added the parameter and
  the tool argument; until now nothing supplied either, so the field never
  appeared in practice.
- The declaration stays the caller's: a `branch_committed_paths` you supply is
  never overridden, including an explicit `[]`, and only `step: "claim"` is
  enriched — `renew`, `release`, and `recover` are untouched. If the diff
  cannot be computed the call is forwarded exactly as written rather than
  declaring an empty branch, because "unknown" and "carries nothing" are
  different answers and only one of them is safe to guess.
