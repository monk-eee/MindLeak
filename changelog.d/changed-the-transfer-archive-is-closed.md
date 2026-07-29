- **The claim-transfer archive is closed to writes, and kept.** ADR-0064
  decision 5, second half. `recover_claim` no longer writes
  `task_claim_transfers`; a recovery is recorded once, as a `claim_recovered`
  event in the task log, beside every other transition it has to be read
  alongside. Two records of one act could disagree, and the disagreement would
  surface exactly when the ledger was being used to settle who held what.
  The table is **not** dropped. It holds ownership recoveries that happened
  before the log existed — real, accurate, and nowhere else — so
  `claim_transfer_history` now reads both and each row carries a `source` of
  `archive` or `log`. That is stated rather than implied because `id` means
  different things in each: an archive row id, or a position in the task log.
  The prior claim's window is reconstructed from the after-image of the event
  the recovery interrupted, so it is read back from the log rather than copied
  into a second table.
