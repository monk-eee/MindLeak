- **A commit carried provenance in an environment with no post-commit hook
  installed, and that is still unexplained — OPEN.** Observed while closing the
  silent hook-install-drift gap: `ce99c35` *does* carry provenance, yet it was
  recorded in an environment whose shared hooks directory had no `post-commit`
  hook. The post-commit hook (`scripts/ingest-commit.mjs`) is the intended
  provenance path, so either something else can record provenance or the hook
  ran when we believe it could not. Impact: until this is understood, the
  presence of provenance on a commit is not proof the post-commit hook ran, so
  do not use it as evidence the hook is installed — use `scripts/hook-health.mjs`
  for that. Left for later; the new pre-push hook-health check closes the
  silent-drift half but not this.
