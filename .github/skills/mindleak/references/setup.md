# Setup and Verification

Use this procedure when the Memory Plane (`mindleak-mcp`) or Intent Plane
(`lodestar-mcp`) is missing, disconnected, newly installed, or pointing at the
wrong workspace.

## Choose the Existing Path

1. **Tools already visible:** do not reinstall. Open one shared session, call
   `storage_status` on both planes, and continue to verification.
2. **Extracted release archive:** use its installer. This is the recommended
   path and requires Node.js 20 or newer, but no Rust toolchain.
3. **VS Code extension (VSIX):** install the platform-targeted VSIX, reload the
   window, and use its Workspace view to confirm both embedded servers.
4. **MindLeak source checkout:** build both binaries with stable Rust, then
   register their absolute paths manually if no release archive is available.

## Release Installer

Before running it, tell the user that the installer will:

- smoke-test and copy both binaries under `.mindleak/bin/<version>/`;
- merge registrations into `.vscode/mcp.json` without removing other servers;
- write `.mindleak/copilot-mcp.json` for GitHub Copilot CLI;
- install this skill under `.github/skills/mindleak`;
- add local MindLeak/Lodestar state rules to `.gitignore`.

From the target workspace root, run:

```bash
node /path/to/extracted/install.mjs --agent your-name
```

Use a stable, human-readable `--agent` label. Do not use a token or secret. The
installer can be rerun: managed skill files update automatically, while locally
edited skill files are preserved and named in its output.

After registration changes, restart the MCP client. For GitHub Copilot CLI use:

```bash
copilot --additional-mcp-config @.mindleak/copilot-mcp.json
```

## Source Build

From a MindLeak checkout:

```bash
cargo build --release --locked -p mindleak-mcp -p lodestar-mcp
```

That build coordinates through the repository-local SQLite stores. Released
binaries additionally carry the Ackplane client, so they can honour
`MINDLEAK_COORDINATION_MODE=federated`; to match them from source, add
`--features mindleak-mcp/federation-client,lodestar-mcp/federation-client`.
Without it a `federated` declaration is refused rather than downgraded, because
arbitrating locally would create a second arbiter for claims Ackplane owns.

Register both binaries with absolute paths. Each registration must set its
matching agent label and `MINDLEAK_WORKSPACE` to the target workspace. VS Code
uses the `servers` key in `.vscode/mcp.json`; Copilot CLI, Claude Desktop, and
Cursor use `mcpServers`. Do not set direct database paths unless the deployment
is intentionally managed; repository-id storage is the normal path.

## Verify End to End

1. Confirm both servers appear in the client's MCP tool list.
2. Mint one 32-character lowercase hexadecimal session token.
3. Call `open_session` on both planes with that same token. In an unborn local
  repository, omit `head_sha`, `base`, and `behind`; neither a first commit nor
  an `origin/main` remote is required.
4. Call `storage_status` on both planes.
5. Require matching non-empty `repository_id` values and database paths under
   the same `repositories/<id>/` directory.
6. Call `constitution_query(action="active")`. This is the positive goal list;
  an empty array means goals have never been seeded for this repository.
7. If the repository chooses Lodestar task coordination and the list is empty,
  call `constitution_define(action="goal", kind="objective", title=...,
  statement=...)` from an accepted, current repository decision. Never turn a
  proposed record into active policy. Use the returned goal id with
  `task_create`, then query `active` again to verify it.
8. Run one read-only memory query and one additional read-only intent query.

If verification succeeds, report the effective session identity and repository
id without dumping database contents. An intentionally goal-less repository is
connected but has not opted into Lodestar task coordination; say so explicitly.

## Troubleshoot

- **No tools after install:** restart the client and inspect its MCP server list.
- **Process will not start:** ensure command paths are absolute in clients that
  do not expand `${workspaceFolder}` and select the binary for the current OS.
- **Planes have different repository ids:** check that both registrations use
  the same `MINDLEAK_WORKSPACE` and belong to the same Git clone.
- **Git cannot find `origin/main`:** do not create a remote merely for the MCP
  servers. Omit undeclared base/divergence fields until the repository has an
  upstream; local repository identity still works before the first commit.
- **Linked worktrees differ:** remove direct `MINDLEAK_DB`/`LODESTAR_DB`
  overrides unless deliberate; linked worktrees should share repository-id
  storage automatically.
- **Recall is unavailable:** deterministic graph and intent tools still work.
  Configure an optional OpenAI-compatible embedding endpoint only when semantic
  recall is wanted.
- **Protocol output is corrupt:** server diagnostics belong on stderr; stdout is
  newline-delimited JSON-RPC only.

Do not repeatedly reinstall around an unexplained failure. Capture the failing
server, path, and error, then fix registration or report the blocker.
