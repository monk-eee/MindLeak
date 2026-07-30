- **The claim decision now surfaces the branch, on both the `claim_task`
  response and the VS Code board row (ADR-0035 decision 5).**
  A won claim confirms the branch its evidence window was pinned to; a lost
  claim names not just who holds the task (`owner`) but the branch they hold it
  on (`owner_branch`) — the fact a colliding agent needs to tell a merge risk
  from the same work twice. The board row shows the owner's branch beside the
  owner (`alice on fleet/x`) and in its tooltip. Both come from what the owner
  declared to `open_session`, pinned to the task's window at claim time, and are
  `null`/omitted cleanly when no branch was declared — never guessed, because
  the server never inspects Git (ADR-0044).
