- **Policy can grow: a new clause can be written into an amendment.** Two
  correct rules met in a corner. `define_goal` states a rule that is live the
  moment it is written, and `complete_clause_contract` refuses to give a live
  rule a contract, because hardening what people are already working under is
  precisely what an amendment is for. But `propose_amendment` only carried
  existing clauses forward and nothing could add one — so the clause that most
  needed an enforcement contract was the one clause that could never be given
  one, and belonging to no constitutional version it never appeared in
  `constitution_diff` either. The only route into a version was
  `register_policy_pack`, which records immutable *upstream* provenance: minting
  a pack to carry a rule this project wrote itself would have put a fabricated
  source in the record. Measured impact — this blocked registering a ratchet
  over the MCP tool surface, because `register_ratchet` needs an active clause
  that authorises it and none of the 25 clauses mentioned the tool surface.
  `draft_clause` authors a clause into an open draft: it enters as part of the
  draft rather than as live policy, reads as `added` in the diff a reviewer
  sees, and carries the same id shape as a clause copied forward, so nothing
  downstream can tell an authored clause from an inherited one once promoted.
