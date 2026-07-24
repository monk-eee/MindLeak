# Screenshots — capture checklist

These images are referenced by the extension
[README](../../README.md) (the Marketplace page) and by
[docs/WALKTHROUGH.md](../../../../docs/WALKTHROUGH.md). Until they are captured,
those pages show the alt text instead of a picture, so **capture them before the
next Marketplace publish**.

## How to capture

1. Open a real workspace that has some MindLeak history (run the
   [Quickstart](../../../../docs/QUICKSTART.md) smoke test, save a few files, run a
   test in the integrated terminal, and make a commit so the panels have content).
2. Use a **dark** theme for consistency, hide personal paths, and keep the VS Code
   window around 1400&nbsp;px wide.
3. Export as **PNG**. Keep each file under ~400&nbsp;KB (crop tightly; you do not
   need the whole screen except for `overview.png`).
4. Save with the exact filenames below — the docs already point at them.

## Shot list

| Filename | What must be visible | Suggested width |
|---|---|---|
| `overview.png` | The MindLeak activity-bar icon selected, with all four views (Context Graph, Intent Board, Telemetry, Design Board) open in the sidebar next to an editor. This is the hero image. | ~1400 px |
| `context-graph.png` | The Context Graph webview showing several nodes and edges, with the colour legend visible. | ~900 px |
| `intent-board.png` | The Intent Board tree with at least one `claimed` task, hovering so the inline actions (Complete With Evidence, Pause) show. | ~900 px |
| `telemetry.png` | The Telemetry pane showing graph size, per-tool call/error/latency metrics, and the Live log toggle. | ~900 px |
| `design-board.png` | The Design Board with a `Proposed` row (Accept / Reject actions) and an accepted/pending row (Promote action). | ~900 px |

Alt text in the docs already describes each panel, so the prose reads correctly
even before the images land.
