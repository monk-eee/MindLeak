# ADR-0073: Each window roots its servers at the worktree it edits

- Status: Accepted
- Date: 2026-07-30
- Decider: MindLeak maintainer (option C selected explicitly after measurement)
- Related: [ADR-0038](0038-isolated-worktrees-shared-repository-state.md)
  (isolated worktrees, shared repository state),
  [ADR-0007](0007-structural-snapshot-reconciliation.md) (structural snapshot
  ownership), [ADR-0005](0005-signal-weighted-decay.md) (reinforcement)

## Context

Every agent window ran its MCP servers rooted at the primary checkout, because
`.vscode/mcp.json` bound both servers to `${workspaceFolder}/target/release/...`
and set `MINDLEAK_WORKSPACE=${workspaceFolder}`, and every window's workspace
folder was `Repos/MindLeak`.

Node ids are repository-relative by contract (ADR-0038). A file is made relative
against the server's workspace root, so a file edited in `MindLeak-build` while
the server is rooted at `Repos/MindLeak` cannot be placed. Before the ingest
guard landed it was minted under an absolute id instead, giving one file a second
identity per checkout; one file was measured holding 117 structural edges under
its absolute id and 43 under its relative one. With the guard, it is refused.

Measured on 2026-07-30:

- `ingest_file` refused **257 of 6450 calls (4.0%)**, roughly two per minute.
  Those files never entered the graph at all — the guard converted silent
  corruption into visible loss, but not into coverage.
- `git status` fails in the workspace folder, because the primary checkout is
  bare: *"this operation must be run in a work tree"*.
- The build notice compares against that checkout's `HEAD`, which is **599
  commits behind main**, so a current binary was reported stale forever (fixed
  separately, in the notice itself).

Two things were verified before deciding, because both are load-bearing:

- **Rooting per worktree does not split storage.** `MindLeak`,
  `MindLeak-extpaths` and `MindLeak-artifactid` all resolve repository id
  `9188c30bd2968f1b2aacd329e0d0a6af` and the same `graph.db`, because
  `repository_id` derives from the git *common* dir, which linked worktrees
  share. The usual objection to per-worktree rooting does not apply.
- **Rooting per worktree fixes the refusal.** With one shared binary: rooted at
  the primary checkout the path is refused; rooted at the worktree the same path
  is accepted as `artifact:editors/vscode/src/util.ts` — the canonical id.

## Decision

A window is opened on the worktree it edits, and `.vscode/mcp.json` keeps `cwd`
and `MINDLEAK_WORKSPACE` following `${workspaceFolder}` so the servers are rooted
there.

The servers themselves are installed **once per machine, outside every
worktree**, at `${userHome}/.mindleak/bin`. `make install-servers` builds and
copies them there. The committed config spells that location with the predefined
`${userHome}` variable, so it names no machine and pins no version.

## Consequences

A file saved in the worktree an agent is working in reaches the graph under its
canonical id. All worktrees continue to share one graph and one board, so the
fleet still coordinates.

Because the binary now lives outside the workspace, the build notice reports it
as an installed binary — identity without a staleness claim — which is the honest
answer for a shared build and removes a warning that could not be acted on.

One binary serves every window, so a deploy is atomic across the fleet: install
once, restart the servers. The cost is that `make install-servers` becomes a
required one-time step on a new machine, and a forgotten install fails as a
missing executable at `${userHome}/.mindleak/bin`, which names its own fix.

## Alternatives considered

**A binary per worktree** (keep `${workspaceFolder}/target/release`). Rejected on
measurement: 56 worktrees, 184 GB of `target/` already on disk, and only 15
holding a server binary. It would also multiply the stale-binary problem by 56.

**Copying binaries into each worktree's `target/release`.** Rejected: cargo's
fingerprints would still read fresh, so that worktree would never rebuild and
would silently serve a binary that does not match its source.

**An absolute path in `.vscode/mcp.json`.** Rejected: the file is tracked, so it
must not name one machine.

**`${env:MINDLEAK_MCP_BIN}`.** Rejected: an unset variable yields an empty
command and a failure that names nothing, across every window at once.

**An extension-provided MCP server** via `mcpServerDefinitionProviders`, reusing
the binary resolution the extension already has. This is the better long-term
shape, and it is deferred rather than dismissed: the API postdates the
`^1.93.0` engine floor that the extension declares and the Extension Host smoke
job pins, so adopting it is an engine-support decision of its own.
