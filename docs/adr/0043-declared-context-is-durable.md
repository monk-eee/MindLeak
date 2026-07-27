# ADR-0043: Declared context is durable, and staleness is declared too

- Status: Proposed
- Date: 2026-07-27
- Amends: [ADR-0035](0035-fleet-management-heuristics.md) (fleet heuristics)
- Related: [ADR-0015](0015-advisory-symbol-leases.md) (false safety),
  [ADR-0030](0030-discrete-per-agent-identity.md) (session identity),
  [ADR-0034](0034-typed-controls-and-enforcement-ceilings.md)
  (enforcement ceilings),
  [ADR-0038](0038-isolated-worktrees-shared-repository-state.md)
  (shared repository state)

## Context

ADR-0035 decided that a session declares where it is working, and that four
heuristics derive from that. Implementing the first of them surfaced two things
the ADR asserts but the design cannot deliver. Both were found before any code
was written for the heuristics task, by reading the acceptance against the
mechanism rather than starting from the acceptance.

**Staleness is not computable from what is declared.** Decision 4 defines
staleness as "commits behind the declared base". Decision 1 forbids the server
from inspecting Git. `branch`, `head_sha`, `base`, and `dirty` are enough to
show that two commits *differ*; they can never show the distance between them.
No amount of care on the server side closes that gap, because the missing input
is a repository walk the server is not allowed to perform.

**A fleet view cannot be built on an in-memory registry.** ADR-0035 stored
declared context in the process-local `SessionRegistry`, on the reasoning that
persisting self-reported facts would make them durable beyond the session that
asserted them. That reasoning holds for identity. It does not survive ADR-0038:
every linked worktree shares one `spec.db`, but each runs its own server with
its own registry. Observed while implementing: two `lodestar-mcp` processes
live, three worktrees, one `repositoryId`. Claims are therefore visible across
the fleet and declared context is not.

The consequence is worse than an incomplete view. A fleet view reading the local
registry would report the sessions of whichever process answered the call, while
presenting itself as the fleet. That is not a gap a reader can see; it is a
confident wrong answer, which is precisely the ADR-0015 false-safety shape that
ADR-0035 decision 5 exists to rule out.

## Decision

1. **`open_session` accepts a declared `behind` count.** The client is the side
   that has Git, so it is the side that can count. This keeps decision 1 intact
   rather than eroding it: the server still performs no inspection of its own.
   Undeclared means `unknown`, never zero.

2. **Declared context is persisted in `spec.db`, keyed by agent id, with its
   `declared_at`.** This supersedes the in-memory-only choice in ADR-0035. The
   original concern is real and is answered by exposure rather than by absence:
   `declared_at` ships with every reading, so a consumer can discount an old
   declaration instead of being quietly misled by one.

3. **Silence is not agreement.** Divergence is computed only from bases that
   were actually declared. A session holding a claim with no declared base is
   counted and shown, never folded into the consensus.

4. **`unknown` is a first-class value, distinct from `current`.** A session that
   declared nothing is unmeasured, not up to date. `Staleness` models the two
   separately so no caller can collapse them by accident.

5. **The view carries its own ceiling.** `fleet_view` returns a fixed statement
   that it is advisory and capped at `review`, so the limit travels with the data
   instead of living only in this document.

## Consequences

- Staleness becomes real rather than aspirational, at the cost of one more thing
  a client may decline to declare.
- The fleet view is honest across worktrees and processes, which is the only
  configuration where the word "fleet" means anything.
- Declared context now outlives the process that declared it. That is the point,
  and `declared_at` is what keeps it honest.
- A client can still declare a wrong `behind`, a stale branch, or nothing at all.
  Bounded by the advisory ceiling: wrong context degrades advice, it never blocks
  work or invalidates a claim.
- `mindleak-session` gains a `serde` dependency so one context type serves both
  the wire and the store. The alternative — a second near-identical struct in
  `lodestar-core` — was written and then deleted; two shapes for one concept is
  how they drift.

## Enforcement and test plan

Platform-agnostic (`cargo` / `npm` / `node` / `git` only):

1. **Unknown is not current.** `Staleness::from_declared(None)` is `Unknown`,
   `Some(0)` is `Current`, `Some(n)` is `Behind(n)`.
2. **Context is durable and replaced wholesale.** A later declaration that omits
   a field clears it, because the client is the source of truth for its own
   position.
3. **Divergence needs two declared bases that disagree.** One declared base, or
   one declared plus one silent, is not divergence.
4. **A silent session is still shown.** A claiming session that declared nothing
   appears in the view with `unknown`, and is counted in
   `undeclared_sessions`.
5. **The view states its own ceiling** in the payload, not only in the docs.
