- **Added:** `amend_constitution` now records who approved the change, not only
  which agent executed it. The new `approved_by` argument is required and must
  differ from the calling agent, so an agent may approve an amendment but never
  its own — the same separation of parties `task_transition to="resolve"`
  already applies to a single task, now applied to the larger act of changing
  adopted policy. Previously `apply_session_contract` inserted `agent` from the
  resolved session *after* the caller's arguments and the handler forwarded that
  as `amended_by`, so every amendment named the calling agent and the
  `amendments` audit history could not distinguish a reviewed adoption from an
  agent changing policy on its own initiative — which is precisely what ADR-0043
  and ADR-0026 rest on. Attributed, never authenticated (ADR-0071): this
  establishes what the record can say, and nothing here is enforced against a
  determined caller on a local stdio server. `constitution_amendments` gains a
  nullable `approved_by` column; amendments written before this backfill as
  NULL, because naming their executor as their own approver would assert exactly
  the separation the column exists to prove. Closes
  `gaps.d/an-amendment-cannot-be-attributed-to-a-human.md`.
