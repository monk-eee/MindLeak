# ADR-0139: `ackplane-mcp`'s task surface scopes to Ackplane's existing claim and read authority, not full Lodestar parity

- Status: Accepted
- Date: 2026-08-29
- Deciders: MindLeak maintainers
- Accepted: 2026-08-29 by the repository owner, authorized directly in session
  — attributed human adoption after review.
- Refines: [ADR-0136](0136-ackplane-gains-an-mcp-front-door-not-a-duplicated-storage-core.md)
  (decision 3 named this as one of three follow-up gaps; this ADR resolves the
  task/claim parity question specifically)
- Depends on: [ADR-0096](0096-ackplane-arbitrates-federated-claims-through-leased-delegation.md)
  (ClaimStore is the only federated lease authority, and only ownership moves —
  content stays local), [ADR-0120](0120-industrial-work-domain-is-an-authoritative-task-projection.md)
  (Industrial Work's current scope: an append-only projection with commands
  explicitly deferred), [ADR-0111](0111-bridge-recovers-a-stranded-claim-as-a-tenant-scoped-administrative-action.md)
  (the one approved claim mutation beyond node-signed RPCs),
  [ADR-0112](0112-bridge-list-reads-are-paginated-sortable-and-filterable.md)
  (bounded list-read discipline this ADR's read tools must follow)
- Related: [ADR-0020](0020-task-lifecycle-states.md) (the local task lifecycle
  this deliberately does not replicate), [ADR-0059](0059-the-tool-surface-is-a-vocabulary.md)
  (the tool vocabulary an honest refusal must still respect)

## Context

ADR-0136 decision 3 named task/claim domain parity as an open follow-up rather
than resolving it: "decide whether `work_tasks` grows the missing
claim-transfer/renew/recover/goal-decomposition semantics, or whether
`ackplane-mcp`'s task tools deliberately compose Ackplane's already-federated
claim arbitration with Industrial Work rather than pretending Industrial Work
already equals Lodestar's full task board."

Read `crates/ackplane-server/migrations/0028_work.sql` and
[ADR-0120](0120-industrial-work-domain-is-an-authoritative-task-projection.md)
directly rather than assuming. The schema is deliberately narrow:
`work_tasks` stores a title, acceptance, an optional `goal_id` (a bare
identifier, never a body — ADR-0120 decision 2 is explicit that "it never
carries a Goal or Constitution definition/body"), an 8-state lifecycle, and
owner/lease columns that ADR-0120 decision 4 states are *derived from*
`ClaimStore`, never a second authority. There is no conformance, waiver,
ratchet, or amendment table anywhere in this domain — those concepts do not
exist here at all, not merely unpopulated.

The load-bearing fact is ADR-0120 decision 8, quoted directly: **"Commands are
deliberately deferred... This ADR does not authorize Bridge creation, routing,
assignment, release, pause, resume, answer, review, completion, abandonment,
or any generic task mutation endpoint. Existing node-signed claim RPCs and
ADR-0111's narrowly approved recovery keep their current contracts."** This is
not a Bridge-specific restriction that a different client could route around —
it states plainly that Ackplane itself has no accepted command for creating or
mutating an Industrial Work task beyond claim/renew/release/recover and
ADR-0111's tenant-scoped stranded-claim recovery. Nothing in `ackplane-server`
contradicts this: `work_task_history`'s `event_kind` is bounded 1-4 and every
row so far in this codebase is produced by claim-adjacent code paths, never a
free-standing "create a task" command.

This means the honest answer to ADR-0136 decision 3 is not a choice between
two options of comparable size. Growing `work_tasks` to cover
claim-transfer/goal-decomposition/pause/resume/answer/complete/abandon is
ADR-0120's own already-identified, already-deferred future work — a
substantial, separately reviewed effort on Ackplane's side, not something this
ADR should casually authorize by defining `ackplane-mcp` tools that assume it
already happened. Composing what already exists is available today, at zero
new server-side authority, and is honest about the resulting gap rather than
hiding it behind a tool name that sounds complete.

## Decision

**`ackplane-mcp`'s first task-related tool surface exposes exactly what
Ackplane already authoritatively supports — federated claim/renew/release/
recover and read-only Work projection queries — and explicitly refuses every
task-lifecycle mutation ADR-0120 decision 8 itself defers, rather than
inventing a client-side approximation of them.**

1. **A `task_claim`-named tool composes ADR-0096's existing federated claim
   arbitration.** `claim`, `renew`, `release`, and `recover` map directly onto
   `ClaimStore`'s compare-and-swap operations already exposed to
   `lodestar-mcp` today via `with_federated_claim_authority`. This is a
   translation, not new authority: `ackplane-mcp` calls the same
   `ClaimDelegationService` RPCs `ackplane-client` already defines
   (`delegate_claim`, `renew_claim`, `release_claim`, `recover_claim`).

2. **A `task_query`-named tool composes ADR-0120's existing read-only Work
   projection.** List, detail, event history, declared scope/overlap,
   stalled/waiting work, and Board Doctor findings are read from `work_tasks`/
   `work_task_history`/`work_task_waits`/`work_task_checkpoints` exactly as
   Bridge's first Work read surface already does, following ADR-0112's bounded
   pagination/sort/filter discipline. Every response carries the same
   publication-state honesty ADR-0120 decision 6 requires
   (`current`/`lagging`/`claims_only`/`not_published`/`unavailable`) — a
   quiet, empty-looking result is never presented as "no work" without saying
   which of those it actually is.

3. **`ackplane-mcp` exposes no `task_create` tool, and no tool named
   `task_transition` that accepts `to="complete"`/`"paused"`/`"blocked"`/
   `"in_review"`/`"abandoned"`.** These would require Ackplane itself to accept
   a create or general mutation command it explicitly does not (ADR-0120
   decision 8). A client that calls for one of these operations receives a
   typed refusal naming the exact gap — "Industrial Work does not yet accept
   this operation; see ADR-0120 decision 8" — never a silent no-op, a fabricated
   success, or a client-side approximation that pretends to move a task's
   state without Ackplane's own authority recording it. This mirrors the
   `worker_adapter.rs`/`AdapterError::Unsupported` discipline ADR-0136 already
   commits to.

4. **Local-only task concepts stay local-only; `ackplane-mcp` does not
   simulate them.** Goal and Constitution *bodies* (not bare ids), acceptance
   evolution history, conformance verdicts, waivers, ratchets, and amendments
   remain exclusively `lodestar-mcp`/local `spec.db` concepts. `ackplane-mcp`'s
   task tools never accept or return a goal/constitution body, never compute a
   conformance verdict, and never expose a waiver or ratchet — because
   Ackplane's own schema has none of these tables, not because of an arbitrary
   client-side restriction. A user who needs those still needs the local
   plane; this ADR does not change that.

5. **When ADR-0120's deferred command contract eventually lands server-side,
   `ackplane-mcp`'s tool surface extends to match it — not before.** This
   decision is not a permanent ceiling on Industrial Work parity; it is a
   statement that `ackplane-mcp` reflects Ackplane's actual authority at every
   point in time rather than getting ahead of it. The sequencing is: Ackplane
   gains a reviewed command contract (ADR-0120 decision 8's own explicit
   future work) first; `ackplane-mcp` exposes the corresponding tool second.

6. **This is a materially narrower task surface than `lodestar-mcp`'s, and
   that narrowing is disclosed, not hidden behind matching tool names.** A
   client switching from `lodestar-mcp` to `ackplane-mcp` cannot claim a task
   it did not already know about (no create), cannot pause/resume/answer a
   wait through this surface, and gets no conformance or goal-body
   information. `ackplane-mcp`'s tool descriptions (ADR-0093's "a contract,
   not a narrative") name this explicitly rather than reading as if a smaller
   Lodestar existed underneath.

## Consequences

- `ackplane-mcp` ships a genuinely useful task surface today — an MCP client
  can see Industrial Work state and participate in federated claim
  arbitration — without waiting on Ackplane's own deferred command contract.
- The real gap (task creation and most lifecycle mutation) is visible and
  named rather than quietly absorbed into a tool that looks complete. A future
  agent or user is not misled into thinking `ackplane-mcp`'s `task_query`/
  `task_claim` already equals `lodestar-mcp`'s full task board.
- No new authority is added to Ackplane by this ADR. It is purely a scoping
  decision for `ackplane-mcp`'s tool surface against what already exists.
- Growing Industrial Work's command contract remains its own, separately
  reviewed effort (ADR-0120 decision 8's explicit future work), unblocked by
  and independent of this decision.

## Rejected alternatives

**Grow `work_tasks` to a full create/pause/resume/answer/complete/abandon
command contract as part of landing `ackplane-mcp`.** Rejected as
out-of-proportion for this ADR: ADR-0120 decision 8 already identifies this as
substantial, separately reviewed future work with its own principal
authorization, compare-and-swap, idempotency, confirmation, and
typed-local-supervisor-directive requirements. Bundling it into an MCP
protocol-adapter ADR would either under-specify a real command contract or
stall `ackplane-mcp` on work with no defined scope yet.

**Have `ackplane-mcp` fake the missing commands client-side (e.g., simulate
`complete` by writing a Work event directly from the adapter, bypassing
Ackplane's own command authority).** Rejected: this recreates exactly the
split-authority failure ADR-0136's own "Rejected alternatives" section
already ruled out — a second implementation deciding a question Ackplane's
server is supposed to own, this time from inside the MCP adapter instead of a
forked storage crate.

**Present `ackplane-mcp`'s task tools under the same names and descriptions as
`lodestar-mcp`'s, silently omitting unsupported operations.** Rejected: ADR-0093
requires tool descriptions to be a contract, and a same-named tool that quietly
supports fewer operations than its local counterpart is exactly the kind of
narrative-shaped surface that ADR treats as a defect.
