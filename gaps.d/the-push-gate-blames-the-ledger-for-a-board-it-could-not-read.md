- **The push gate reports an unreachable ledger when the ledger answered and
  only the board could not be read — OBSERVED 2026-07-31, OPEN, latent.**
  `publishVerdict` in `scripts/claim-gate.mjs` branches on
  `!reachable || !agent` and emits one message: *"the Lodestar ledger is
  unreachable, so this publication cannot be attributed to a claim"*, offering
  `cargo build --release` or `LODESTAR_MCP_BIN`. But `canonical-push` computes
  `reachable = Boolean(agent) && Array.isArray(board)`, so that single message
  covers three different situations: the server did not answer, it answered but
  did not identify the session, and it answered and identified the session while
  `task_query view=board` failed. Only the first is unreachability. The comment
  immediately above the branch says the checks "are ordered so each refusal
  names its own cause"; for this one that is not true.

  Observed while publishing a docs branch. `canonical-push` refused as
  unreachable; `open_session` against the same binary returned
  `agent_id: session:v1:cca3…` in 450ms. Timing the gate's four calls separately
  found the real cause: `task_query view=board` answered
  `invalid: unknown task event kind: coverage_declared`. A newer build had
  written an event kind the deployed binary cannot parse, and one unrecognised
  row fails the whole board read. The offered remedy is wrong for this case —
  the binary is not missing, it is *older than the ledger it is reading* — and
  the diagnosis costs a session to reach, because nothing in the message
  suggests looking at the board at all.

  — *Latent rather than currently biting.* The event-kind fragility that
  triggered it is being fixed separately, so once a newer writer no longer
  blinds an older reader this particular trigger disappears. The conflation does
  not: any future board-read failure will still be announced as an unreachable
  ledger. — *Same class as*
  [the pre-commit stash race](the-pre-commit-stash-race-reports-a-failure.md),
  where hooks that modify nothing report `files were modified by this hook`
  about files the committer never touched. Both are guards whose refusal names a
  cause that is not the real one, and in both the natural response — retry, or
  rebuild — makes things worse or wastes the session. A guard that refuses is
  doing its job; a guard that refuses with the wrong reason is worse than one
  that says only "refused", because it spends the reader's attention in the
  wrong place. — Not fixed here: the push gate is the one path every agent
  depends on, and v0.1.4 was being finalised at the time. Separating the
  conditions is a small change and wants its own commit, its own tests in both
  `scripts/claim-gate.test.mjs` and `editors/vscode/scripts/claim-gate.test.mjs`,
  and a quieter moment.
