# ADR-0016: Platform packaging and workspace registration

- Status: Accepted
- Date: 2026-07-22
- Deciders: MindLeak maintainers
- Related: [ADR-0013](0013-local-data-lifecycle.md)

## Context

A source checkout can auto-detect `target/release`, but a packaged VS Code
extension currently falls back to `PATH`. Shipping a VSIX without native servers
would therefore produce an installed UI that cannot start either plane. Agents
also need both servers in `.vscode/mcp.json`; asking users to hand-edit two
similar entries is error-prone and can overwrite unrelated MCP registrations.

Native servers are platform-specific, while extension JavaScript and the two
SQLite invariants are shared. One universal VSIX cannot contain every binary
without unnecessary size and ambiguity.

## Decision

- Publish one targeted VSIX per supported platform: `win32-x64`, `linux-x64`,
  `darwin-x64`, and `darwin-arm64`. Each contains exactly the matching
  `mindleak-mcp` and `lodestar-mcp` binaries under `bin/`.
- Default binary resolution prefers an explicitly configured path, then packaged
  `bin/`, then a workspace `target/release` or `target/debug` build, then `PATH`.
  This keeps installed releases self-contained and source development unchanged.
- Each native release archive also contains a bundled, dependency-free
  `install.mjs`. One invocation copies both servers into the workspace-local,
  gitignored `.mindleak/bin/<version>/` directory, smoke-tests them, and merges
  both registrations into `.vscode/mcp.json` using a JSONC parser.
- Registration preserves comments and unrelated servers. It writes through a
  temporary sibling and atomically renames only after both native servers pass
  an MCP initialize/tools-list smoke test.
- The two registrations share a stable agent id but retain separate database
  paths and environment variables. Installation never merges the stores.
- Platform archives, direct VSIX assets, checksums, and signed GitHub provenance
  attestations are produced by the tag release workflow. OS publisher/code
  signing remains a release-credential concern and must not be simulated.

## Consequences

- Installing the targeted VSIX is sufficient for the extension experience; no
  Rust toolchain or global `PATH` mutation is required.
- Running the archive installer is sufficient for agent MCP registration in one
  workspace and does not disturb existing servers.
- Releases grow by one VSIX per platform, and the release matrix must verify both
  native stdio servers before packaging.
- A mismatched VSIX cannot silently select another architecture; its packaged
  servers are fixed by the release matrix target.
