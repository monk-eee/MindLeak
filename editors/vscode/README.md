# MindLeak — Temporal Context Graph (VS Code)

Live visualizer and passive sensor for the [MindLeak](../../README.md) context
graph engine.

## What it does

- **Passive sensing** — focusing a file boosts its node (resets the decay clock);
  saving a file ingests its symbols into the graph. No manual bookkeeping.
- **Live graph** — a Cytoscape sidebar renders the current context subgraph:
  - 🔵 file · 🟠 symbol · 🟢 intent · 🔴 execution
  - edge width ∝ time-decayed effective weight
- **Controls** — Refresh, Prune decayed edges, Export snapshot JSON.

## Requirements

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
| `mindleak.snapshotLimit` | `60` | Max nodes rendered. |

## Develop

```bash
npm install
npm run watch    # incremental compile
# then press F5 in VS Code to launch an Extension Development Host
```

> The webview loads a **vendored** `cytoscape.min.js` from `media/vendor/`
> (copied from the npm package by `npm run compile`) — no CDN, fully offline.
