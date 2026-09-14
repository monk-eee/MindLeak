- **Concurrent materialization writers can race revision allocation.**
  In `crates/ackplane-server/src/design_materialization_store/mod.rs`,
  `MaterializationStore::record_materialization` checks idempotency before its
  insert transaction and computes `MAX(revision_number) + 1` without a
  per-design lock. Two callers can select the same next revision, leaving one
  with a database constraint failure rather than a stable replay or next
  revision. This is a code-inspection finding, not a newly reproduced timing
  failure. Left for a separate transactional-concurrency fix; the task-order
  retry defect was fixed and tested in this workstream. A regression should
  exercise simultaneous identical and distinct submissions for one design.
