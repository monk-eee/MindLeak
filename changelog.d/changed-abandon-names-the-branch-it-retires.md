- **`task_transition to=abandon` no longer retires branched work silently.** A
  task records the branch its claiming session declared, and abandon is a
  one-way door; measured, every abandoned task carrying a branch corresponded to
  a real pull request, most of them already merged. Abandon now refuses a task
  that recorded a branch unless `acknowledge_branch=true` is passed, and the
  refusal names the branch — the ledger cannot see a pull request from the stdio
  server, so it asks the caller to check rather than deciding for them. A
  branchless task, or one abandoned with the acknowledgement, retires exactly as
  before.
