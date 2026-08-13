- **The constitution verbs were reachable all along; a tool profile made them
  look missing, and the mistake reached an accepted ADR — OBSERVED 2026-08-13,
  left OPEN.** `advertised_for` in `crates/lodestar-mcp/src/tools/mod.rs`
  narrows `tools/list` to a 17-name `DEFAULT_PROFILE_TOOLS` allowlist under
  ADR-0059. Dispatch is deliberately *not* narrowed, and the function's own
  comment says so: "a specialist tool called by name still runs under the
  default profile, so nothing the server can do becomes unreachable — it is
  only no longer paid for up front."

  An agent that reads its own tool list therefore sees `advise` and
  `governing_for_task` and no constitution verbs, and concludes the capability
  does not exist. Measured under the default profile:

  | Question | Answer |
  |---|---|
  | Does `tools/list` advertise `constitution_status` or `governing_goals`? | no |
  | Does `tools/call constitution_status` answer? | yes — returned the live `constitution:v3` |
  | Does `tools/call governing_goals` answer? | yes — returned `[]` for `ackplane-core` |

  Both probes were read-only and mutated nothing. The verbs exist, dispatch,
  and can be called by name today: `constitution_define` (which supersedes
  `define_goal`), `supersede_goal`, `link_goal_to_artifact`,
  `unlink_goal_from_artifact`, `governing_goals` and `constitution_query` in
  `tools/constitution.rs`, plus the whole amendment lifecycle in
  `tools/amendments.rs` — `propose_amendment`, `draft_clause`,
  `complete_clause_contract`, `amend_constitution` (which promotes with an
  attributed rationale and an explicit clause diff, superseding rather than
  deleting) and `amendments` (the audit history, newest first, with each
  rationale and stored diff).

  The cost is what makes this worth a fragment rather than a note. The false
  premise was written into ADR-0092 decision 5 — *"the Intent Plane exposes no
  agent-reachable verb that defines a goal or binds code to one, so an agent
  cannot adopt this even if it wanted to"* — and that ADR is now `Accepted` on
  `main`, so the error is inside a constitutional record rather than beside it.
  It then authorised `task:6cd90c2d2b4f`, "Make an accepted ADR adoptable into
  the active constitution", whose acceptance describes append-only, auditable,
  attributed adoption: clause for clause, the subsystem `amendments.rs` already
  is. That task was blocked before anyone claimed it, so the rebuild did not
  happen, but nothing structural stopped it — the board offered it as ordinary
  open work.

  What is genuinely unresolved is narrower and worth keeping: `amend_constitution`
  takes a `rationale` but no `human` label distinct from the calling agent, so
  the attributed-adoption shape ADR-0043 and ADR-0071 require may not be
  expressible yet. That is the question to answer, not "write a constitution
  API".

  Left open because the two honest repairs are both human calls: correcting a
  factual claim inside an `Accepted` ADR is an amendment under ADR-0043, and
  performing the Ackplane adoption with the existing verbs is a constitutional
  act. An agent surfacing them is the right move; an agent making them is not.

  The narrow fix, if a mechanical one is wanted later, is to make absence
  legible rather than silent: have `tools/list` under a narrowed profile say
  that specialist tools exist and remain callable, so a reader cannot mistake
  the advertisement for the capability. PORTABLE: an empty tool list is
  evidence about what is advertised, never about what exists — a capability
  behind a profile, a feature flag, or a stale client is indistinguishable from
  one that was never written, and the cheap disambiguator is to call the verb
  you believe is missing and read the source's aggregator.
