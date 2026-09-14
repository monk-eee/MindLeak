- Both local planes now refuse workspace-local storage fallback when Git
  discovery fails and repository metadata is still present, including broken
  worktree links and dangling metadata symlinks. Repair guidance replaces a
  misleading empty store. Explicit database paths and genuine non-Git scratch
  use are unchanged. A reclaimed directory with all metadata removed remains
  indistinguishable from a new scratch workspace.
