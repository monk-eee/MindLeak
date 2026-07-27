# ADR-0045: An agent fleet is a distributed system, not a team

- Status: Accepted
- Date: 2026-07-27
- Deciders: MindLeak maintainers
- Refines: [ADR-0038](0038-isolated-worktrees-shared-repository-state.md)
  (isolated worktrees, shared repository state)
- Related: [ADR-0030](0030-discrete-per-agent-identity.md) (discrete per-agent
  identity), [ADR-0018](0018-conflict-safe-concurrent-editing.md) (conflict-safe
  concurrent editing), [ADR-0032](0032-single-checkout-fleet-integration.md)
  (superseded single-checkout model)

## Context

Running several agents concurrently against this repository produced four
failures in a single day. Each was investigated on its own terms and fixed on
its own terms, and only afterwards did the shape become obvious: they are not
four Git problems. They are four textbook concurrency failures wearing Git
costumes.

| Observed as | Actually |
|---|---|
| `pre-commit` reports `files were modified by this hook` on hooks that modify nothing, naming files the committer never touched | **Lost update.** `pre-commit` stashes unstaged changes around the hook run; a second writer inside that window collides with the restore. |
| Two branches independently create `docs/adr/0043-*.md` | **ID collision.** ADR numbers are a monotonic sequence with no allocator. |
| The ADR-number guard blocks the branch whose ADR had *already landed on `main`* | **Stale read.** The guard read its own branch as authority instead of the converged state. |
| `fleet_view` would report the sessions of whichever server process answered, while presenting itself as the fleet | **Split registry.** Session context was process-local while storage was shared, so each process held a partial view of global state and none knew it. |

The diagnostic cost was almost entirely in the translation. Every one of these
presents with a message that points nowhere near its cause — the stash race is
the extreme case, where the natural response (retry) widens the window that
caused it. Named correctly, each has a known remedy and a known class of
sibling bugs. Named as "a weird Git thing", each is a fresh investigation.

The mistake underneath was modelling a fleet of agents on a team of people.
People coordinate by knowing about each other. Concurrent processes do not, and
neither do agents: they share a filesystem, an index, a branch namespace, and a
number sequence with no mutual awareness whatsoever. The repository stops being
a version-control system and becomes a database with no transactions.

## Decision

**Fleet coordination is designed and diagnosed as a distributed-systems
problem.**

1. **Name the class before fixing the instance.** A fleet failure is triaged
   first against lost update, ID collision, stale read, split registry, and
   lease expiry — not against Git semantics. The presenting message is treated
   as unreliable evidence, because in every case so far it was.
2. **Every shared mutable resource has exactly one arbiter, or it is not
   shared.** The default is isolation: one worktree, one branch, one agent
   (ADR-0038). Sharing a writable resource is a reviewed exception that must
   name its arbiter, not a convenience that accretes.
3. **The arbiter is the converged state, never a local view.** A guard that
   reads its own branch will eventually block the claimant that already won.
   Authority for anything contested — ADR numbers, file ownership, task claims —
   is `origin/main` or the shared database, resolved at check time.
4. **A guard must always be satisfiable, and must name who has to move.** A
   guard with no exit teaches bypass, and the bypass becomes habit; one skipped
   hook is worth less than the guard it disabled for good. Every refusal states
   the party that has to move and the action that resolves it.
5. **Coordination cost must be sublinear in agent count.** A guard that encodes
   a *mechanism* — isolation, arbitration, ownership, expiry — is fixed cost and
   holds for the next agent. A guard that encodes a *single incident* is
   per-incident cost. If the Nth agent needs the Nth guard, the fleet has
   reached its ceiling: the correct response is to stop adding agents, not to
   add another guard.

## Consequences

- The next fleet bug is cheaper. The four incidents above cost roughly a day
  between them, most of it spent discovering that the error message was lying.
  The table is a triage index, and its rows predict siblings that have not
  fired yet — an unarbitrated branch namespace, a lease that outlives its
  holder, a cache read before its write converges.
- Clause 5 gives an explicit stopping condition, which the fleet did not have.
  Scaffolding built to survive concurrency is legitimate cost; scaffolding built
  per incident is a treadmill, and without a stated test the two look identical
  from inside a productive day.
- Clause 2 makes sharing expensive on purpose. Some genuinely shared resources —
  the repository databases under ADR-0038 — must now name an arbiter explicitly
  rather than relying on SQLite's locking being good enough in practice.
- Clause 3 costs a network read in guards that previously ran locally. Accepted:
  a guard that is wrong about who won is worse than a guard that is slow.
- This ADR constrains process, not runtime behaviour. It changes no MCP
  semantics and no verdict.

## Rejected alternatives

- **Treat each incident as a one-off and keep patching.** What was already
  happening. It works, and it hides the ceiling: every fix feels like progress
  while the coordination surface grows with the fleet. The stash race, the
  number collision, and the split registry would each have been fixed again in
  a different disguise.
- **Serialise the fleet — one writer at a time.** Removes the whole class, and
  the throughput that motivated a fleet with it. This is ADR-0032, which was
  superseded for exactly that reason.
- **Adopt a real distributed-coordination substrate (a lock service, a
  transactional VCS).** Correct in the large, disproportionate here. Isolation
  by default plus a named arbiter for the few shared resources covers every
  failure observed, at a fraction of the cost.
- **Record this as a lesson in `DEVELOPERS.md` rather than an ADR.** Rejected
  because it is a decision with a stopping rule, not an observation. Clause 5
  in particular constrains future work, and a "Known gaps" note does not survive
  contact with a productive day.
