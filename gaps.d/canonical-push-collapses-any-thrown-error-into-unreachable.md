- **canonical-push.mjs's claim-gate `callTools` batch collapses any thrown
  exception into the generic "the Lodestar ledger is unreachable" message,**
  even when the real cause has nothing to do with reachability. Where:
  `scripts/canonical-push.mjs`'s `try { ... callTools(...) ... } catch { reachable
  = false; }` around the `fleet_view`/`open_session`/`board`/`overlap`/
  `board-by-branch` batch. Measured 2026-08-25: a worktree whose
  `canonical-push.mjs` already called the new `task_query(view=board,
  branch=...)` argument (added for `gaps.d/task-query-board-has-no-response-
  size-bound.md`), pushed while `LODESTAR_MCP_BIN` pointed at a `target/release`
  binary built *before* that server-side argument existed. The server answered
  every call, but the branch-filtered `board` call came back as a non-array,
  `withReconciliationCandidates(board, branchHistory)` in `scripts/claim-gate.mjs`
  threw `"(candidates ?? []).filter is not a function"` on the non-array
  `branchHistory`, and the bare `catch` reported the generic "unreachable"
  message with its "rebuild it or point LODESTAR_MCP_BIN at an existing server"
  advice — technically the right remedy here (a stale binary), but for a
  reason the message never states, and the same message would be printed for
  a totally different bug in the parsing/merge logic with no way to tell them
  apart short of instrumenting the catch block by hand.
  Impact: cost real diagnosis time (copying the script to a scratch file and
  adding `console.error(error.message)` to the catch) before the actual cause
  was visible. A future genuine bug in `withReconciliationCandidates` or the
  result-parsing chain would look identical to a stale binary or a down
  server, sending whoever hits it toward a rebuild that will not help.
  Not fixed this run (out of scope for the coverage task in progress): the
  fix direction is to log `error.message` (or thread through a distinguishing
  code) before collapsing to `reachable = false`, so a parse/shape defect in
  the JS layer cannot masquerade as a binary/connectivity problem.
