- **Claiming a task overwrites the branch its previous owner recorded, and
  `abandon` then cannot be answered honestly — OBSERVED 2026-08-12, left
  OPEN.** A claim writes the claimant's declared branch onto the task, replacing
  whatever the previous owner recorded. Rescuing a lapsed claim therefore
  destroys the one field that says where the stranded work actually is.

  Measured on four tasks in one session, three claimed and one deliberately not:

  | Task | How it was taken | Recorded branch after | Recorded branch before |
  |---|---|---|---|
  | `task:523510b1663f` | claimed | `fix/adr-cross-reference-links` | `feat/ackplane-federation-service` |
  | `task:93473480a526` | claimed | `fix/adr-cross-reference-links` | `main` |
  | `task:fc192f8ed4bf` | claimed | `fix/adr-cross-reference-links` | `detached@origin/main` |
  | `task:b0979f99d856` | blocked, never claimed | `main` | `main` |

  The lost value was load-bearing: `feat/ackplane-federation-service` is the
  branch that carried merged PR #402, so the record no longer points at the work
  the task produced.

  The consequence lands on `abandon`, which refuses a task that recorded a
  branch unless the caller attests that branch carries no open or merged pull
  request — the flag exists precisely because the ledger cannot see GitHub. A
  rescuer who claims first can no longer answer that truthfully: the branch it
  names is now their own, and in this case carried an open pull request of
  unrelated work. Retiring three duplicate seeds therefore required `block` with
  a written reason instead, which is honest and reversible but is not what the
  verb is for. The alternative is to attest something false to a guard, which is
  the failure mode a guard exists to prevent.

  Impact is quiet and cumulative. Rescue is the sanctioned response to a lapsed
  lease — `open_session` advertises it under `rescue_work` and hands over the
  exact `task_claim` call to make — so the ledger actively recommends the step
  that erases the evidence, and nothing reports the loss. Nine tasks on this
  board record a branch; every future rescue costs one of them.

  The workaround is real and cheap: transition a task without claiming it first.
  `block`, `abandon` and the other lifecycle verbs act on a task the caller does
  not own, so a rescuer who only intends to retire work should not claim it —
  which is how `task:b0979f99d856` above kept its branch.

  Left for later, deliberately. Whether a claim should retarget the branch, keep
  the original, or record both is a design question about what the field means:
  ADR-0048 treats a lapsed claim as keeping its evidence window, and ADR-0044
  says the server records what the client declares and never inspects Git, so
  "the branch" may need to become "the branch per claim" rather than one slot.
  That is a decision, not a patch. Observed while rescuing four lapsed claims.
