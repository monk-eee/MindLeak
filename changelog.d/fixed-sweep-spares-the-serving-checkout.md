- **Fixed:** the continuous artefact sweep no longer deletes the build output
  the fleet host is actively serving from. `classifyArtefact` in
  `scripts/worktree-reclaim.mjs` already refused to sweep `target/release`
  there, but keyed the refusal on the checkout being **bare**. A fleet serving
  from an ordinary non-bare checkout on `main` passed every other predicate —
  clean, landed, unowned, idle — so the sweep took it, after which
  `worktree-reclaim` refused every worktree with "no lodestar-mcp binary found"
  and `canonical-push` could not publish at all. The same run took
  `editors/vscode/node_modules`, which the commit hooks shell out to prettier
  and eslint from, so the next commit in that checkout failed with
  `MODULE_NOT_FOUND` from a hook that modifies nothing. The refusal is now
  keyed on the worktree being **primary** — which a bare host always is, so the
  case the original guard named is still covered — and spans both directories
  as one rule, because what makes them load-bearing is the checkout they sit in
  rather than what they hold. Linked worktrees' landed `target/release` and
  `node_modules`, and the host's `target/debug`, are still swept, which is
  where the reclaimable disk actually sits.
