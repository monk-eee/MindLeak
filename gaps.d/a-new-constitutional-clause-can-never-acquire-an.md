- **A new constitutional clause can never acquire an enforcement contract —
  OPEN.** There is no supported route to add a locally authored, enforceable
  clause to the constitution. `define_goal` writes the clause with
  `status = active` and `constitution_version = None` (`store/goals.rs`), and
  `complete_clause_contract` refuses any clause that is active
  (`facade/constitution.rs`) — correctly, because hardening a live rule
  mid-flight is an amendment. But `propose_amendment` only copies existing
  active clauses into the draft; nothing inserts a new one, so the clause that
  most needs a contract can never be given one. The policy-pack path is not a
  workaround: it records immutable upstream provenance, which would be false
  for a locally authored rule. — Measured impact: it blocked the ratchet half
  of task:3eab606fbaf6. The measurement is independently deliverable in PR
  #147; the ratchet remains task:8000f45e0dfd, waiting on
  task:4cef8e361fc7. — Found 2026-07-29; still open.
