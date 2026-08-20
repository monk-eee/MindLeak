- The VS Code extension's output channel now names which candidate resolved
  each MCP server binary ("explicit-config", "packaged", "shared-install",
  "workspace-release", "workspace-debug", or "fallback") alongside the
  resolved path, in both plane-connected log lines. A stale binary packaged
  into an installed extension silently outranking a freshly rebuilt shared
  install is now visible instead of invisible.
