- **Audited commit evidence repair:** `repair_commit_attribution` corrects one
  existing commit from exact local Git facts, with a registered session and
  reason, preserving unrelated history and original authorship. The correction
  and complete before/after audit commit together; unavailable or shallow Git
  history is refused. `telemetry_snapshot(event_id)` retrieves the durable audit
  even after it leaves recent events. Repair does not certify a task or rewrite
  earlier conformance records.
