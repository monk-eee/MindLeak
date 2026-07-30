- **Existing setups lack the post-commit ingest hook, so unobserved commits have
  no provenance — VERIFIED, OPEN.** `.pre-commit-config.yaml`
  declares `default_install_hook_types: [pre-commit, pre-push, post-commit]`,
  but the shared `.git/hooks` directory contains only `pre-commit` and
  `pre-push`. `default_install_hook_types` only takes effect when
  `pre-commit install` is re-run; an environment set up before that line was
  added keeps working and silently never installs the new type. Observed on
  `b4a9067` and `543e1c1`: `evidence_for` over the correct window returned
  nothing, and neither task could be certified until the commit was re-ingested
  by hand. Impact: an empty evidence bundle that looks exactly like an agent who
  forgot to ingest — which is the failure the hook was built to eliminate — so
  the diagnosis lands on the wrong cause. It cost this session two wrong
  theories before anyone checked whether the hook existed.
  Fixing it is `pre-commit install --install-hooks`, but note that this is the
  *shared* hooks directory: every worktree and every agent picks it up at once,
  and each commit then spawns an MCP server, so it is a fleet-wide load change
  rather than a local one.
  The hook now reports when it cannot record, and honours
  `MINDLEAK_INGEST_TIMEOUT_MS` — worth having, but it reports nothing while it
  is not installed at all, which is the actual gap.
  Unexplained: `ce99c35` *does* have provenance, recorded in the same
  environment with no post-commit hook installed. Do not assume the hook ran.
