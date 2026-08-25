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
  **Narrowed**: `board()` now takes an opt-in `detail` argument (default
  `true`, so every existing caller's contract is unchanged). `detail=false`
  skips the three per-row `scope`/`claim_window`/`receipt` engine lookups and
  the free-text `acceptance` field, while still returning every task — only
  the cheap, already-in-hand `lease_state` derivation rides along regardless.
  Nine callers verified (by reading their own field usage, not assumed) to
  read only base task fields (`id`/`title`/`status`/`goal_id`/`owner`/
  `branch`/`lease_expires_at`/`claim_started_at`) now pass it:
  `scripts/board-health.mjs`, `scripts/canonical-push.mjs` (its claim gate
  reads only status/owner/lease/branch), `scripts/evaluate-pr-effectiveness.mjs`
  (`claimStartedAt()` prefers the base `claim_started_at` field over
  `claim_window.started_at`), `scripts/status.mjs` (`liveClaims`),
  `scripts/worktree-reclaim.mjs` (`liveClaimBranches`), `editors/vscode/src/
  designBoardController.ts`'s quick-pick, `editors/vscode/src/
  extension.ts`'s `refreshEvidence`, `editors/vscode/src/fleetController.ts`
  (`FleetTaskRow` mapping), and `editors/vscode/src/readinessController.ts`
  (reads only the array's `.length`).
  Left for later: `scripts/stranded-report.mjs` genuinely reads
  `task.claim_window.lapses`/`.unleased_seconds`, and `editors/vscode/src/
  extension.ts`'s `refreshBoard` (feeding `boardViewProvider.ts` via
  `util.ts`'s `taskTooltip`/`taskDescription`) genuinely reads
  `task.scope.paths`/`.symbols` and `task.acceptance` — both stay at the
  default `detail=true`. `boardViewProvider`/`evidenceBoardViewProvider` never
  call `board()` themselves; they render whatever `refreshBoard`/
  `refreshEvidence` already fetched. `board()` also still returns a bare array
  rather than an object, so it has no room for a truncation/paging signal the
  way `active_knowledge` gained one; that reshape is unchanged by this fix.
  **Found and fixed while investigating this fragment (2026-08-25)**: the
  `include_terminal: false` this fragment's own earlier pass gave
  `canonical-push.mjs`'s claim gate had a real cost nobody had traced through:
  `reconciliationOf` (claim-gate.mjs) looks for an already-`done`/`abandoned`
  task matching the pushed branch, but a task in that state can never appear
  in a fetch that excludes every terminal task — so a legitimately-delivered
  branch could never be recognized as a reconciliation and always fell
  through to "no live Lodestar claim", silently. Existing unit tests for
  `reconciliationOf` never caught it because they hand-construct `tasks`
  directly, bypassing the real `include_terminal: false` fetch entirely.
  Fixed by adding a `branch` filter to `board()` (independent of
  `include_terminal`, narrows to tasks recorded on exactly that branch, any
  status) and a new `withReconciliationCandidates(primary, candidates)` merge
  helper in `claim-gate.mjs`; `canonical-push.mjs` now makes a second, small,
  branch-scoped fetch specifically for this and merges it in. This is
  additive progress on the general gap too: a caller that only needs "was
  THIS branch already delivered" no longer has to choose between missing it
  (`include_terminal: false`) or paying for the whole terminal history
  (`include_terminal: true`).
  Still open: the general reshape (`board()` returning an object with a
  truncation signal, for the two `detail=true` callers named above) is
  unchanged.
