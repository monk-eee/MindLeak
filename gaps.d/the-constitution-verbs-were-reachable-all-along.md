- **A narrowed `tools/list` does not say what it hides, so an absent tool is
  indistinguishable from one that was never written — OBSERVED 2026-08-13,
  still OPEN.** `advertised_for` in `crates/lodestar-mcp/src/tools/mod.rs`
  filters the advertised surface down to `DEFAULT_PROFILE_TOOLS` under ADR-0059
  and adds nothing beside it. Dispatch is deliberately *not* narrowed — the
  function's own comment says "a specialist tool called by name still runs under
  the default profile, so nothing the server can do becomes unreachable" — but
  no part of the response tells a caller that, so an agent reading its own tool
  list concludes the capability does not exist.

  The cost has already been paid once. The false premise reached ADR-0092
  decision 5 as *"the Intent Plane exposes no agent-reachable verb that defines
  a goal or binds code to one, so an agent cannot adopt this even if it wanted
  to"*, putting a factual error inside a record that was `Accepted` on `main`.
  It then authorised `task:6cd90c2d2b4f`, "Make an accepted ADR adoptable into
  the active constitution", whose acceptance describes append-only, auditable,
  attributed adoption — clause for clause, what `tools/amendments.rs` already
  was. That task was blocked before anyone claimed it, but nothing structural
  stopped it; the board offered it as ordinary open work.

  Everything that episode motivated is now repaired, and none of those repairs
  is this gap: `constitution_define` and `constitution_query` are on the default
  profile so a goal-less repository can bootstrap (PR #455), `amend_constitution`
  requires an `approved_by` distinct from the calling agent (PR #441), ADR-0092
  decision 5 carries its own correction (PR #443), and the Ackplane adoption
  happened as `constitution:v4`. Each was a fix at one named site.

  The general shape is untouched. `grant_waiver`, the design board, the policy
  packs and the rest of the amendment lifecycle are still advertised nowhere by
  default and still dispatch by name, so the next agent to look for one of them
  can draw the same wrong conclusion. The fix, when it is wanted, is to make
  absence legible rather than silent: have a narrowed `tools/list` state that
  specialist tools exist and remain callable. Left open deliberately — it
  changes a published response shape, so it wants a decision of its own rather
  than a drive-by edit.

  PORTABLE: an empty tool list is evidence about what is advertised, never about
  what exists — a capability behind a profile, a feature flag, or a stale client
  is indistinguishable from one that was never written, and the cheap
  disambiguator is to call the verb you believe is missing and read the source's
  aggregator.
