- **A task claimed across a constitution amendment cannot certify itself —
  NARROWED 2026-08-26 by ADR-0109, one residual still OPEN.** Observed
  2026-07-29 on `task:7b6154f1d69a` (ADR-0064). The task was claimed under
  `goal:durable-intent-plane-for-multi-agent-coordinatio`; during the claim that
  goal was re-versioned and the governing clause became
  `goal:durable-intent-plane-for-multi-agent-coordinatio@constitution:v2`. The
  `reconnect_superseded_clauses` migration exists to move non-terminal tasks onto
  their successor clause, and it **deliberately skips any task holding a live
  lease** — correctly, because moving one mid-flight re-governs an agent's work
  outside its evidence window (that guard was added after it harmed 3 agents).
  While stranded, the task covered no active clause: `governing_for_task`
  returned `[]`, `advise` returned `review`, and `check_conformance` returned
  **drift** — "governed code changed without a covering task" — naming the very
  goal the task serves.

  **The holding agent now has a route, and it is not human review.** This
  fragment used to close by asking whether the migration should also move a
  task "whose lease is live *but whose owner is the one asking*".
  [ADR-0109](../docs/adr/0109-a-live-claim-may-consent-to-follow-its-amended-clause.md)
  ("A live claim may consent to follow its amended clause", Accepted
  2026-08-20) decided exactly that question — yes, on the reasoning this
  fragment itself proposed: the ADR-0063/ADR-0068 harm was re-governing work
  *behind* an agent's back, which is not what an owner consenting to their own
  move is. It shipped: `LodestarStore::reconnect_claim_clause`
  (`store/coordination/creation.rs`), `Lodestar::reconnect_claim_clause`
  (`facade/executive/tasks.rs`), and the MCP surface
  `task_claim(step="renew", reconnect_clause=true)`.

  Concretely, an owner stranded by an amendment mid-claim should renew with
  `reconnect_clause: true` rather than accept a drift verdict. Only the current
  owner may ask, only for their own unexpired lease, and only when the
  superseded clause has exactly one active same-slug successor; a refusal says
  which of "not superseded", "ambiguous successor", or "not the current owner"
  applied instead of erroring generically. It records one attributed,
  append-only act on the task's thread and never rewrites a
  `conformance_records` row already written under the old clause (ADR-0025):
  only evidence checked *after* reconnection is judged against the new clause.
  `reconnect_superseded_clauses` and every future amendment's automatic
  carry-forward are unchanged — ADR-0068 decision 5 still holds for everyone
  who is not the holder.

  **Still OPEN — the holder who has already moved on.** ADR-0109's own
  Consequences name what it deliberately does not cover: it "does nothing for
  a task whose holder has already moved on". That case keeps the original
  vice, because reconnection requires a live lease held by the caller. Letting
  the lease lapse *would* let the migration move the task, but a lapse holes
  the evidence window and ADR-0048 caps the verdict at `needs_human`; so an
  abandoned claim spanning an amendment still reaches `in_review` and a human,
  not `aligned`. The workarounds remain wrong for the same reasons: releasing
  and re-claiming opens a fresh window and orphans the commits, and narrowing
  the evidence to dodge the finding is the laundering ADR-0048 exists to stop.
  Human acceptance is still the correct terminus there — as it was for the
  original measurement, where `complete_task` recorded the drift, moved the
  task to `in_review`, and a human accepted it out (`resolved_by`,
  `resolved_conformance_id: 182`, the overruled verdict pinned per ADR-0009).
  Nothing was broken and nothing was laundered.

  Original measurement taken against the deployed build `d4addbd9a2fc`, not
  this checkout.
