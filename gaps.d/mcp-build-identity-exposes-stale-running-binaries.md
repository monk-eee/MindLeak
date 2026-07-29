- **MCP build identity exposes stale running binaries.** Both servers now report
  `serverInfo.version` as `<package-version>+<12-character-git-sha>` during MCP
  initialize. Compare the suffix with `git rev-parse --short=12 HEAD`; a mismatch
  means the server must be rebuilt and restarted before debugging source
  behaviour or relying on newly added tools. The shared Cargo build helper watches
  Git HEAD/ref changes and supports `MINDLEAK_BUILD_SHA` outside a checkout. —
  Resolved Jul 2026.
