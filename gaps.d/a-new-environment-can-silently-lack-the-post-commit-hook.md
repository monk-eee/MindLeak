- **A new environment can silently lack the post-commit ingest hook, and one
  commit's provenance is still unexplained — NARROWED 2026-07-30, OPEN.**
  `.pre-commit-config.yaml` declares
  `default_install_hook_types: [pre-commit, pre-push, post-commit]`, and the
  shared hooks directory now contains all three, so the original measurement —
  that `post-commit` was missing and commits landed with no provenance at all —
  is no longer true here and has been removed.

  What remains is the mechanism that produced it. `default_install_hook_types`
  only takes effect when `pre-commit install` is re-run, so any environment set
  up before a hook type was added keeps working and silently never installs the
  new one. Nothing reports the difference: the hook that would announce it is
  the hook that is not installed. The failure it produces is an empty evidence
  bundle, which looks exactly like an agent who forgot to ingest — the very
  failure the hook exists to eliminate — so the diagnosis lands on the wrong
  cause. It cost one session two wrong theories before anyone checked whether
  the hook existed.

  Also still unexplained: `ce99c35` *does* carry provenance, recorded in an
  environment with no post-commit hook installed. Until that is understood, do
  not treat the presence of provenance as proof the hook ran.

  Worth noting for anyone fixing the install-drift half: the hooks directory is
  *shared* across every worktree, so `pre-commit install --install-hooks`
  changes the commit path for every agent at once, and each commit then spawns
  an MCP server. That is a fleet-wide load change, not a local one.
