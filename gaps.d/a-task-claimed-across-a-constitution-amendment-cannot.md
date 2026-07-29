- **A task claimed across a constitution amendment cannot certify itself, and
  must be human-accepted — MEASURED, OPEN.** Observed 2026-07-29 on
  `task:7b6154f1d69a` (ADR-0064). The task was claimed under
  `goal:durable-intent-plane-for-multi-agent-coordinatio`; during the claim that
  goal was re-versioned and the governing clause became
  `goal:durable-intent-plane-for-multi-agent-coordinatio@constitution:v2`. The
  `reconnect_superseded_clauses` migration exists to move non-terminal tasks onto
  their successor clause, and it **deliberately skips any task holding a live
  lease** — correctly, because moving one mid-flight re-governs an agent's work
  outside its evidence window (that guard was added after it harmed 3 agents).
  The consequence is a vice. While the lease is live the task covers no active
  clause: `governing_for_task` returns `[]`, `advise` returns `review`, and
  `check_conformance` returns **drift** — "governed code changed without a
  covering task" — naming the very goal the task serves. Letting the lease lapse
  *would* let the migration move it, but a lapse holes the evidence window and
  ADR-0048 caps the verdict at `needs_human`. So no route available to the
  holding agent reaches `aligned`: keep the claim and it drifts, release it and
  the window is holed.
  The system's answer is the human one, and it worked: `complete_task` recorded
  the drift and moved the task to `in_review`, and a human accepted it out
  (`resolved_by`, `resolved_conformance_id: 182` — the overruled verdict pinned,
  per ADR-0009). Nothing is broken, and nothing was laundered. — Impact: every
  claim spanning an amendment becomes human work, and the longer and more careful
  the work the likelier it is to span one. That is a load on review that scales
  with amendment frequency, not with risk. — **Not fixed this run.** The obvious
  workarounds are both wrong: releasing and re-claiming opens a fresh window and
  orphans the commits, and narrowing the evidence to dodge the finding is the
  laundering ADR-0048 exists to stop. Worth deciding whether the migration should
  also move a task whose lease is live *but whose owner is the one asking* — the
  ADR-0063 harm was re-governing work behind an agent's back, which is not the
  same as an agent consenting at completion time.
  Measured against the deployed build `d4addbd9a2fc`, not this checkout.
