# ADR-0033: First-class GitHub Copilot CLI registration for both planes

- Status: Accepted
- Date: 2026-07-24
- Deciders: MindLeak maintainers
- Related: [ADR-0016](0016-platform-packaging-and-registration.md) (packaging and
  workspace registration — this extends it), [ADR-0030](0030-discrete-per-agent-identity.md)
   (session-scoped identity), [ADR-0027](0027-extension-led-progressive-disclosure.md)
  (CLI is a first-class client), [ADR-0013](0013-local-data-lifecycle.md) (local
  stores)

## Context

Both planes are stdio MCP servers, so any MCP client can drive them. The
agent-loop benchmark already proves it end to end:
[`scripts/evaluate-agent-loop.mjs`](../../scripts/evaluate-agent-loop.mjs) writes
a `mcpServers` config, points GitHub Copilot CLI at it with
`--additional-mcp-config @mcp.json` under a scoped `COPILOT_HOME`, and both
`mindleak` and `lodestar` tools answer. That path is real but **test-only**: it
uses hardcoded absolute executables, an `eval-agent` identity, an ephemeral home,
and `--disable-builtin-mcps` — none of it registers MindLeak for a person who
just runs `copilot` in their repository.

The canonical installer
([`editors/vscode/scripts/install.mjs`](../../editors/vscode/scripts/install.mjs),
ADR-0016) registers both servers, but **only** into `.vscode/mcp.json`. That file
is unusable by Copilot CLI for two concrete reasons:

1. **Schema key differs.** VS Code keys servers under `"servers"`. Copilot CLI
   (like Claude Desktop and Cursor) keys them under `"mcpServers"`. The CLI does
   not read `.vscode/mcp.json` at all.
2. **Variable substitution differs.** The VS Code registration embeds
   `${workspaceFolder}` in the binary path, DB paths, and workspace env. Copilot
   CLI does not expand that VS Code-only variable, so a copied entry points the
   CLI at a literal `${workspaceFolder}/...` path and fails to start.

Today a CLI user must hand-author a second config. It drifts from the VS Code
registration, and hand-editing routinely mis-sets the two things that must stay
correct: the agent identity (ADR-0030) and the two SQLite paths — so a CLI
session silently writes to a different graph than the editor, or claims work
under the wrong owner.

## Decision

1. **One registration source, rendered per client.** The installer keeps a
   single server definition (today's `registrations()` shape: command + env for
   `mindleak` and `lodestar`) and renders it into each client's format — VS Code
   under `"servers"`, and Copilot CLI / Claude / Cursor under `"mcpServers"`. The
   two servers are never defined twice; the renderer only reshapes the wrapper and
   the path form.
2. **Register with Copilot CLI as a supported target.** The installer writes a
   CLI-readable config: a project-scoped MCP config merged in place for a single
   repository, and the documented user-level `~/.copilot/mcp-config.json`
   (honouring `COPILOT_HOME`) for a machine-wide default. Both planes are always
   registered together; there is no single-plane flag, matching ADR-0016.
3. **Resolve CLI paths concretely, not with editor variables.** Because
   `${workspaceFolder}` is not portable to the CLI, the Copilot CLI form resolves
   the binary and both DB paths to absolute install-time paths. The DB and
   `.gitignore` targets are identical to the VS Code registration, so the editor
   and the CLI read and write the **same** `.mindleak/graph.db` and
   `.lodestar/spec.db`.
4. **Preserve what is already there.** CLI config is merged with the same
   guarantees as ADR-0016: parse JSON(C) tolerantly, keep unrelated servers and
   comments, write through a temporary sibling, and rename only after both native
   servers pass an MCP initialize/tools-list smoke test.
5. **Identity and transport invariants are unchanged.** Session identity
   (ADR-0030) applies: the CLI inherits matching `MINDLEAK_AGENT` /
   `LODESTAR_AGENT` base labels, mints one opaque token, calls `open_session` on
   both planes, and reuses it on identity-bearing calls. The servers stay
   stdio-only and unauthenticated (SPEC §8); registering the CLI adds no network
   listener.

## Consequences

- A CLI user gets both planes from one installer invocation, with no hand-written
  second config and no drift from the VS Code registration or the shared stores.
- The installer grows a client-format renderer and a second merge target; the
  test suite must cover `mcpServers` rendering, absolute-path resolution, and
  merge-preservation of foreign CLI servers — mirroring the existing
  `.vscode/mcp.json` tests.
- The CLI registration is pinned to absolute install-time paths, so moving the
  workspace requires re-running the installer. That is the documented cost of the
  CLI not supporting `${workspaceFolder}`; the VS Code registration keeps its
  portable variable form.
- Documentation (QUICKSTART, README, USAGE) gains a Copilot CLI section beside the
  VS Code / Claude / Cursor guidance, pointing at the same installer rather than a
  bespoke config.
- The eval harness stays as-is for benchmarking, but is no longer the only place
  that knows how to speak to Copilot CLI; the supported path is the installer.
