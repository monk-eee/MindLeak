- **Projection catch-up preserves newly indexed vectors:** a delayed background
  scan now rechecks freshness after acquiring the repository's rebuild lock.
  If another worker already caught up, it preserves the vectors and projection
  timestamp instead of replaying again. Rebuilds of the same tenant/repository
  serialize; other repositories remain independent. Explicit rebuilds retain
  their full replay and derived-vector invalidation behavior.
