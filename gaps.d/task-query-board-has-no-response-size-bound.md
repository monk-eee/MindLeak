- **`task_query(view="board", include_terminal=true)` has no response-size
  bound.** — Observed 2026-08-24 while investigating a suspected 0.1.7-alpha
  memory leak: `board()` (`crates/lodestar-mcp/src/tools/executive/tasks.rs`)
  serializes every task the ledger has ever held (any status, when
  `include_terminal=true`, the default), enriching each row with a `scope`,
  `claim_window`, and `receipt` lookup, and returns the whole thing as one
  unbounded `Vec<Task>` — unlike `existing_work` in the same file, which
  already truncates via `bounded_by_recency`/`TASK_PREVIEW_LIMIT`.
  Where: `crates/lodestar-mcp/src/tools/executive/tasks.rs::board`, backed by
  `Lodestar::board`/`LodestarStore::board`.
  Impact: measured directly this session — a single live `task_query` MCP
  result reached roughly 1.95 MB on this repository's own board (which has
  grown into the many hundreds of tasks, terminal and live). VS Code's chat
  session store persists a tool call's full result inline in
  `chatSessions/*.jsonl` (and was observed storing it more than once per
  turn), so a handful of such calls in one session materially inflates the
  extension-host/renderer memory the editor has to hold — the same class of
  pressure as upstream VS Code issues #294050 and #326906 (large persisted
  chat-session state), just fed by an oversized tool reply rather than a
  chat-side bug. `active_knowledge` had the identical shape of problem and was
  fixed this session (see `changelog.d/fixed-active-knowledge-response-size.md`)
  by capping its array and keeping full-set counts accurate.
  Left for later: `board()` returns a bare array, not an object, and is read
  directly (over MCP/JSON-RPC) by several Node scripts
  (`scripts/board-health.mjs`, `scripts/canonical-push.mjs`,
  `scripts/stranded-report.mjs`, `scripts/worktree-reclaim.mjs`,
  `scripts/evaluate-pr-effectiveness.mjs`) and VS Code extension providers
  (`boardViewProvider`, `fleetController`, `readinessController`,
  `evidenceBoardViewProvider`) that all assume a complete list — some compute
  repo-wide health/counts over it. Truncating or reshaping it needs each
  consumer's tolerance checked first, or a small design decision (e.g. an
  opt-in `limit`/`since` argument, or moving the per-row `scope`/
  `claim_window`/`receipt` enrichment behind a flag) rather than a same-session
  change to a contract this many callers depend on for completeness.
