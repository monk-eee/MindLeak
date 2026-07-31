- **VS Code now records saves and deletes from sibling worktrees.** When a file
  sits outside the window's workspace folders, the extension sends its normalized
  filesystem path so MindLeak can map it through the repository's known worktree
  roots. Normal workspace-relative paths are unchanged, and save refreshes use
  the canonical artifact id returned by the server rather than minting an
  absolute-path identity in the client.
