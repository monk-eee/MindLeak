<p align="center">
  <img src="media/mindleak_logo.png" alt="MindLeak" width="420">
</p>

# MindLeak — Temporal Context Graph (VS Code)

Live visualizer and passive sensor for the [MindLeak](../../README.md) context
graph engine.

## What it does

- **Passive sensing** — focus updates attention, save ingests structure,
  shell-integrated terminal events record command outcomes and changed files,
  and Git commit events record commit evidence. No agent-authored ingestion call
  is required.
- **Visible capture health** — the graph status reports active, disabled, or the
  concrete degraded reason. Terminal capture requires VS Code shell integration.
- **Workspace readiness** — one derived tree shows the exact Memory/Intent
  builds, attribution identity, current first-value state, optional degradation,
  and the next concrete action. It stores no graph or task authority.
- **Privacy by default** — command metadata is enabled; terminal output storage
  is disabled unless explicitly opted in, then redacted and capped.
- **Live graph** — a Cytoscape sidebar renders the current context subgraph:
  - 🔵 file · 🟠 symbol · 🟢 intent · 🔴 execution
  - edge width ∝ time-decayed effective weight
- **Telemetry pane** — a real-time effectiveness readout: graph size (nodes,
  active edges), tool-call success and error rates, average latency, and
  per-tool metrics, refreshed live while the pane is visible. A **Live log**
  toggle (off by default) streams recent tool/maintenance events on demand.
- **Design Board** — repository ADRs are synchronized into Lodestar without
  inferring tasks from Markdown. Proposed designs can be accepted or rejected by
  an attributed human reviewer; accepted designs remain visibly pending until a
  reviewer promotes them under an active objective. Materialized rows expose
  their persisted objective, tasks, and constraints, and failed promotion stays
  retryable.
- **Intent Board** — active task ownership and evidence actions remain separate
  from design review, so proposed ADRs never appear as claimable implementation
  work. Open and expired-claim rows can be allocated to a stable agent or claimed
  for the configured extension identity; live claims expose explicit renew and
  release actions. **Next Claimable Task** reveals the scheduler's suggestion but
  never auto-claims it.
- **Controls** — Refresh, Prune decayed edges, Export complete graph JSON, back
  up both planes, and modal reset of regenerable memory only.

## Guided tour

Click the **MindLeak icon in the activity bar** to open the operational views in
the sidebar. You mostly *watch* them while your agent works; a few actions are
yours to click.

![MindLeak's four views open in the VS Code sidebar next to an editor](media/screenshots/overview.png)

### Workspace

The first row is the current derived readiness state: connection remediation,
first ingest, Context Graph, Intent Board, Design Board, or optional-capability
telemetry. The following rows show both MCP build identities and the configured
base label's effective per-activation identity. The extension passes that exact
fixed id to both MCP planes, so attribution and claims agree. A fresh workspace
opens this view once; afterward it stays an ordinary refreshable tree that can
be collapsed or ignored.

The state comes from `graph_stats`, `board`, `design_board`, MCP initialize
metadata, and sensor health. The extension never persists a parallel readiness,
graph, goal, or task state.

### Context Graph

The live, decay-weighted subgraph of what connects to what right now. Node colour
marks the kind (file · symbol · intent · execution) and edge width grows with a
relation's time-decayed effective weight, so fresh, load-bearing links stand out
and stale ones thin away. Use the title-bar **Refresh** and **Back Up Memory and
Intent** actions here.

![The Context Graph webview with nodes, edges, and the colour legend](media/screenshots/context-graph.png)

### Intent Board

Who owns which task, live. Claiming and completing are **agent** actions over
MCP; the board shows the resulting ownership and lets you intervene. Hover a task
for inline actions: **Complete With Evidence** and **Pause** on a claimed task,
**Resume** on a paused one, **Answer** on one that needs input, **Inspect
Evidence** on a finished one, and **Retire** on a stale row.

![The Intent Board tree showing a claimed task with inline actions](media/screenshots/intent-board.png)

### Telemetry

A real-time effectiveness readout — graph size, tool-call success and error
rates, average latency, and per-tool metrics — so you can verify what actually
ran independent of any agent's narration. The **Live log** toggle (off by
default) streams recent tool and maintenance events on demand.

![The Telemetry pane with graph size and per-tool metrics](media/screenshots/telemetry.png)

### Design Board

Turn ADRs into work without inferring tasks from Markdown. **Sync ADRs** imports
`docs/adr/*.md` by path, title, and status. A **Proposed** row exposes **Accept**
/ **Reject** (a human reviewer, not the proposing agent) and updates the ADR's
declared status. An accepted row exposes **Promote**: choose Create, Link
Existing, or No New Work, inspect the concrete plan, then confirm one atomic
materialization. Existing work and no-work decisions require a rationale.

![The Design Board with a proposed row and an accepted, pending row](media/screenshots/design-board.png)

> **Want the full workflow?** [docs/WALKTHROUGH.md](../../docs/WALKTHROUGH.md)
> walks four end-to-end scenarios — look-before-you-leap, ADR-to-tasks, two
> agents splitting a goal, and passive capture during a normal edit/test/commit
> loop.

## Requirements

- VS Code 1.93 or newer.

A platform-targeted release VSIX includes both MCP servers and prefers those
packaged binaries automatically. No Rust toolchain or global `PATH` change is
required. Source development still auto-detects `target/release` (or `debug`),
and explicit server path settings take precedence over both.

For source development, build both MCP servers first:

```bash
cargo build --release
```

Override with `mindleak.serverPath` / `mindleak.lodestarServerPath`.

## Settings

| Setting | Default | Description |
|---|---|---|
| `mindleak.serverPath` | `mindleak-mcp` | Path to the `mindleak-mcp` binary. |
| `mindleak.databasePath` | `` | Graph DB path; empty = `<workspace>/.mindleak/graph.db`. |
| `mindleak.lodestarServerPath` | `lodestar-mcp` | Path to the Intent Plane server. |
| `mindleak.lodestarDatabasePath` | `` | Intent DB path; empty = `<workspace>/.lodestar/spec.db`. |
| `mindleak.agentId` | `vscode` | Base label for one unique per-activation id shared by both child MCP servers. |
| `mindleak.autoIngestOnSave` | `true` | Ingest a file's symbols on save. |
| `mindleak.captureExecutions` | `true` | Capture shell-integrated command metadata and outcomes. |
| `mindleak.captureTerminalOutput` | `false` | Retain bounded, redacted output with executions. |
| `mindleak.terminalOutputMaxChars` | `8192` | Maximum retained output characters per execution. |
| `mindleak.captureExcludePathPrefixes` | internal/generated paths | Path prefixes excluded from changed-file evidence. |
| `mindleak.maxChangedFilesPerExecution` | `200` | Maximum changed paths attached to one execution. |
| `mindleak.captureCommits` | `true` | Capture built-in Git extension commit events. |
| `mindleak.snapshotLimit` | `60` | Max nodes rendered. |
| `mindleak.telemetryRefreshSecs` | `3` | Seconds between live Telemetry pane refreshes while visible. |
| `mindleak.autonomousConsolidation` | `false` | Opt in to idle model-backed consolidation. |
| `mindleak.consolidateIdleSecs` | `300` | Idle seconds before a pass. |
| `mindleak.consolidateMinIntervalSecs` | `3600` | Minimum seconds between attempts. |
| `mindleak.consolidateMaxNodes` | `20` | Maximum candidates per pass. |

The extension settings above are the authority for its child process and
override inherited `MINDLEAK_AUTONOMOUS_CONSOLIDATION` / scheduler environment
values. Reload the extension host after changing them; the worker configuration
is intentionally resolved once at server startup.

## Design workflow

The extension synchronizes `docs/adr/*.md` on activation and file changes using
only the ADR path, H1 title, and declared `Status`. Use **MindLeak: Sync ADRs**
for an explicit refresh. Reconciliation is idempotent and creates no tasks.

From the Design Board:

- Proposed rows expose **Accept** and **Reject** actions.
- Accepted/pending rows expose **Promote** and remain visible after a failed
  attempt so promotion can be retried safely.
- Create mode selects one or more active objectives and previews separate task
  drafts; Link Existing selects authoritative tasks already on the Intent Board;
  No New Work records why nothing should be scheduled.
- Every mode shows a modal review before Lodestar materializes it exactly once.
- Materialized rows expose objective/task/constraint provenance plus immutable
  revision history and an attributed **Repair** action.

Human acceptance and rejection require an identity different from the proposing
agent. ADR discovery never auto-accepts or auto-promotes a design.

## Task allocation

The Intent Board displays the owner, claim start, lease expiry, and whether a
claim is live or reclaimable. Allocation remains advisory until Lodestar's atomic
claim compare-and-swap succeeds.

- **Claim Task for Me** always uses the registered activation session; callers
  cannot select or impersonate another owner.
- Lease choices are bounded from five minutes through eight hours.
- **Renew Task Lease** and **Release Task** are bound by the server to the
  registered activation session, so owner-guard failures remain visible rather
  than silently changing work.
- Expired claims are reclaimable and open a fresh evidence window; parked tasks
  (`needs_input` / `paused`) remain owned and cannot be allocated.
- **Recover Legacy Claim** is offered only for expired pre-session owners and
  records the prior owner/window plus the attributed recovery reason.
- **Next Claimable Task** highlights the next row without claiming it.

## Develop

```bash
npm install
npm run watch    # incremental compile
# then press F5 in VS Code to launch an Extension Development Host
```

> The webview loads a **vendored** `cytoscape.min.js` from `media/vendor/`
> (copied from the npm package by `npm run compile`) — no CDN, fully offline.
