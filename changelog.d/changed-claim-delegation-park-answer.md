### Changed

- ADR-0096 clause completion: `ClaimDelegationService` gains `ParkClaim` and
  `AnswerClaim` RPCs, so a federated repository routes a task entering
  `needs_input`/`paused` (a durable question or a deliberate pause, ADR-0020)
  and its resumption through Ackplane's claim CAS instead of deciding them
  locally — matching the existing `delegate`/`renew`/`release`/`recover`
  pattern. A parked claim keeps its owner's exclusive hold over the task's
  scope even though its lease is cleared, so it still blocks a competing
  `delegate`/`recover` and still counts as active for overlap detection;
  only `answer`, called by the exact parking owner, returns it to
  circulation with a fresh lease. The durable question/answer/note text
  itself stays local (`task_qa`) in both coordination modes — Ackplane
  arbitrates only the claim-state transition, never becoming a mode of the
  local plane's own dialogue.
