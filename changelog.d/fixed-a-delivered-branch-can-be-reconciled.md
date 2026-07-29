### Fixed

- **A delivered branch can be reconciled again.** Completing a task releases its
  claim, and publication requires one — so once a task was done, its pull
  request could never be brought up to date. `main` moved, the branch went
  stale, and the delivery queue stepped over it forever. #168 needed hand
  rescuing three times, and each rescue invented a throwaway task purely to get
  past the gate. Minting a task per republish is exactly how six duplicate tasks
  reached the board.

  A task now records the branch it was claimed on, so a delivered branch is
  already attributed; re-attributing it to a fresh task records a fiction.
  `canonical-push` publishes it as a reconciliation and says whose work it was.

  Deliberately narrow: **every** new commit must be a merge. A reconciliation
  merges the base in and nothing else, so this cannot decay into "finish a task,
  then push anything to that branch forever" — the moment real work appears, a
  claim is required again. That case is tested, because an exemption without one
  is a bypass wearing a fix's clothes.
