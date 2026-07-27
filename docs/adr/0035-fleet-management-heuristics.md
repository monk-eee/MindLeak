# ADR-0035: Fleet management heuristics and feedback

- Status: Proposed
- Date: 2026-07-27
- Deciders: MindLeak maintainers
- Related: [ADR-0030](0030-discrete-per-agent-identity.md) (session identity),
  [ADR-0024](0024-preflight-overlap-detection.md) (overlap),
  [ADR-0032](0032-single-checkout-fleet-integration.md) (fleet integration),
  [ADR-0015](0015-advisory-symbol-leases.md) (false safety),
  [ADR-0034](0034-typed-controls-and-enforcement-ceilings.md)
  (enforcement ceilings)

## Context

ADR-0030 gave every agent a stable, restart-safe identity, so both planes know
*who* is acting. Neither plane knows *where*. `open_session(token)` returns an
identity and nothing else; the only git-aware code in Lodestar resolves the
database path. Branch, head commit, base, and working-tree cleanliness are
invisible to the Intent Plane.

Three consequences follow.

**Ownership is half-reported.** The board shows that
`session:v1:copilot:3ca2ce9f…` holds a claim, but not the branch that work is
landing on. In a fleet sharing one checkout that is tolerable; the moment two
agents sit on different branches it is not.

**Overlap is binary when it should be proportional.** `check_overlap` treats
every intersecting path claim as equal risk. Two agents editing the same file on
the same branch are colliding now; on different branches they are creating
merge-time risk later. Those deserve different advice, and today they get the
same answer.

**Staleness is invisible until it is expensive.** ADR-0032 treats divergence as
a stop signal, which is reactive: an agent discovers it when `canonical-push`
refuses or a merge conflicts. Nothing surfaces drift while it is still cheap to
fix. This repository hit that twice in one session — a fleet branch left stale
after its predecessor merged, and a branch whose base moved when an auto-merge
landed mid-work.

## Decision

1. **Working context is declared, not detected.** `open_session` accepts
   optional `branch`, `head_sha`, `base`, and `dirty`. The client supplies them,
   matching `reconcile_workspace` and `propose_constitution`: the server
   performs no git or filesystem inspection of its own. This is also the only
   correct answer for a stdio server that may not share a working directory with
   the agent, and for a linked worktree whose branch differs from the database
   root.

2. **Declared once per session and refreshed on change, never echoed into every
   response.** Attaching agent and branch to every MCP result would bloat large
   payloads (`board`, `graph_snapshot`, `recall`), duplicate state the session
   registry should own, and require touching every read tool for one
   cross-cutting concern.

3. **Surfaced where a decision is made**, not everywhere: the `claim_task`
   response, board rows, the VS Code Intent Board, and `check_overlap`.

4. **Four heuristics** derive from the declared context:

   | Heuristic | Signal |
   |---|---|
   | Staleness | commits behind the declared base |
   | Divergence | live sessions working from different bases |
   | Overlap precision | same-branch collision versus cross-branch merge risk |
   | Ownership clarity | who is working where, live |

5. **Advisory only.** Declared context is self-reported: an agent can omit it,
   let it go stale, or state it wrongly. Its enforcement power under ADR-0034 is
   therefore `advisory`, and the ceiling rule caps its effective consequence at
   `review`. It must never block. Only a control that reads git itself could
   claim `observed`, and nothing here does.

6. **Absent context degrades to today's behaviour.** A session that declares
   nothing keeps working exactly as it does now; the heuristics simply report
   `unknown` rather than guessing.

7. **This supplies evidence for `fleet.branch_freshness`** (ADR-0034 task 4),
   which would otherwise be a clause with no observation behind it.

## Consequences

- Overlap advice becomes proportional instead of binary, which is the first
  concrete improvement to ADR-0024 since it shipped.
- Drift is visible while it is still cheap, rather than at the publisher or the
  merge.
- The fleet gains an honest answer to "who is doing what, where" without a new
  coordination mechanism, a network listener, or a model.
- Self-reported context can be wrong. That is accepted and bounded by the
  advisory ceiling; a wrong branch label degrades advice, it never blocks work
  or invalidates a claim.
- `open_session` grows optional parameters. Existing clients are unaffected,
  including the released v0.1.2 extension.

## Rejected alternatives

- **Echoing agent and branch into every MCP response.** Bloats the large read
  payloads, creates a second source of truth beside the session registry, and
  spreads one concern across every tool.
- **Server-side git detection.** A stdio server may not share the agent's
  working directory, a linked worktree's branch differs from the database root,
  and it contradicts the established caller-supplies-facts precedent.
- **Blocking on staleness.** Exactly the ADR-0015 false-safety trap: an advisory
  signal that looks like a guarantee. Staleness advice informs; the publisher's
  ancestor check remains the mechanical control.
- **A separate `fleet_status` registration call.** A second handshake beside
  `open_session` would let identity and context drift apart.

## Enforcement and test plan

Platform-agnostic (`cargo` / `npm` / `node` / `git` only):

1. **Optional context.** A session that declares no context behaves exactly as
   today, and heuristics report `unknown` rather than a guess.
2. **Round trip.** Declared branch, head, base, and dirty flag survive a server
   restart under the same session token.
3. **Overlap precision.** Two claims on one path report a higher-signal overlap
   when both sessions declare the same branch than when they declare different
   branches.
4. **Ceiling.** No heuristic can produce `block`; a stale or wrong declared
   branch degrades advice and never refuses a claim, a commit, or a push.
5. **Surface.** The claim response and board rows carry agent and branch when
   declared, and omit them cleanly when not.
