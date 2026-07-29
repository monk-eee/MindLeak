- **Lodestar worktree sharing was path-based, then checkout-root based.** — The
  original server used `LODESTAR_DB` or the process CWD; the first fix resolved
  through Git's common directory but still privileged the checkout owning
  `.git`. — Low impact on correctness, high coupling to physical topology. —
  **Superseded Jul 2026 by ADR-0038:** both planes now resolve one random
  per-clone repository id from shared local Git config and use the same
  platform-local user store from every linked worktree. Explicit DB overrides
  still win; scratch use outside Git remains workspace-local.
