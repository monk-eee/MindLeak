- **A file no longer splits into a separate node per worktree.** Node ids are
  repo-relative by contract, but `normalize_path` only settled separators — it
  never made an absolute path relative. Editor sensors report absolute paths, and
  every worktree of a repository shares one graph (ADR-0038), so a single file
  could occupy a different identity in each checkout. Measured on this
  repository: **871 of 6,144 nodes carried absolute ids across 7 worktrees, and
  590 files existed under two identities at once** — `AGENTS.md` and
  `.pre-commit-config.yaml` among them. The damage is quiet and broad: edits
  split across identities so genuine reinforcement decays like a one-off,
  `check_overlap` cannot see two agents editing the same file from different
  worktrees, governed bindings cover only one spelling of the path, and `recall`
  returns the same file twice. The process now declares the checkout it serves
  (`MindLeak::with_workspace_root`, wired from the resolved workspace), and paths
  inside it are made repo-relative at every entry point that accepts one —
  ingestion, deletion, reconciliation, and `check_overlap`. A path genuinely
  outside the checkout is left alone rather than forced into a relative form that
  would name a file that does not exist.
