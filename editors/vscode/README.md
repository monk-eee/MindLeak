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
- **Privacy by default** — command metadata is enabled; terminal output storage
  is disabled unless explicitly opted in, then redacted and capped.
- **Live graph** — a Cytoscape sidebar renders the current context subgraph:
  - 🔵 file · 🟠 symbol · 🟢 intent · 🔴 execution
  - edge width ∝ time-decayed effective weight
- **Controls** — Refresh, Prune decayed edges, Export snapshot JSON.

## Requirements

- VS Code 1.93 or newer.

Build the MCP server first:

```bash
cargo build --release
```

The extension auto-detects `target/release/mindleak-mcp` (or `debug`) in the
workspace. Override with the `mindleak.serverPath` setting.

## Settings

| Setting | Default | Description |
|---|---|---|
| `mindleak.serverPath` | `mindleak-mcp` | Path to the `mindleak-mcp` binary. |
| `mindleak.databasePath` | `` | Graph DB path; empty = `<workspace>/.mindleak/graph.db`. |
| `mindleak.autoIngestOnSave` | `true` | Ingest a file's symbols on save. |
| `mindleak.captureExecutions` | `true` | Capture shell-integrated command metadata and outcomes. |
| `mindleak.captureTerminalOutput` | `false` | Retain bounded, redacted output with executions. |
| `mindleak.terminalOutputMaxChars` | `8192` | Maximum retained output characters per execution. |
| `mindleak.captureExcludePathPrefixes` | internal/generated paths | Path prefixes excluded from changed-file evidence. |
| `mindleak.maxChangedFilesPerExecution` | `200` | Maximum changed paths attached to one execution. |
| `mindleak.captureCommits` | `true` | Capture built-in Git extension commit events. |
| `mindleak.snapshotLimit` | `60` | Max nodes rendered. |

## Develop

```bash
npm install
npm run watch    # incremental compile
# then press F5 in VS Code to launch an Extension Development Host
```

> The webview loads a **vendored** `cytoscape.min.js` from `media/vendor/`
> (copied from the npm package by `npm run compile`) — no CDN, fully offline.
