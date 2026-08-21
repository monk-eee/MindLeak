# ADR-0114: Context packets are bounded, attributed, and reproducible

- Status: Accepted
- Date: 2026-08-21
- Deciders: MindLeak maintainers
- Accepted: 2026-08-21 by the repository owner, authorized directly in session
   — attributed human adoption after review.
- Depends on: [ADR-0086](0086-postgresql-is-the-ackplane-ledger-and-arbiter.md)
  (durable Industrial state),
  [ADR-0102](0102-context-is-compiled-not-assembled-by-hand.md) (local packet
  semantics), [ADR-0106](0106-ackplane-closes-the-agentic-operating-loop.md)
  (Industrial guidance)
- Related: [ADR-0107](0107-registered-agents-accept-authenticated-control-directives.md)
  (directives reference context packets),
  [ADR-0108](0108-knowledge-rpcs-authenticate-with-operation-signing.md)
  (operation-specific authentication)

## Context

ADR-0102 established the right local principle: context is a typed,
token-bounded compilation over existing evidence, graph, working-memory, and
governance sources. ADR-0106 extends that principle to an Industrial fleet,
where an agent may run on a developer machine, in a pipeline, or in a cloud
worker that has no local copy of the relevant SQLite stores.

An Industrial context packet needs stronger properties than a prompt assembled
inside one client process. A person investigating a later failure must be able
to answer what the agent was told, what policy governed it, which projection
versions supplied the facts, whether any material was already stale, and why
other apparently relevant material was excluded. At the same time, retaining
unbounded prompts or blindly replaying them into another agent would create a
sensitive data store and a dangerous command replay surface.

## Decision

**Ackplane compiles a versioned, immutable `ContextPacket` for a specific
agent session and work target. The packet is reproducible for audit, bounded
for use, and never itself an authorization to act.**

1. **A packet is a typed, durable record rather than free-form prompt text.**
   It has a packet id, digest, compiler/protocol version, tenant, repository,
   project where applicable, target agent session, task and goal references,
   issuing time, expiry, source ledger and projection positions, requested
   budget, actual budget use, and lifecycle state. Its payload contains typed
   selections for objective/acceptance, Constitution and policy, task and
   lease state, relevant evidence, active knowledge, prior outcomes, and
   bounded structural context. Every item is a reference plus the minimum
   rendered data required by the recipient.

2. **Required governance material is not traded away for relevance.** The
   compiler reserves budget for the target identity, acceptance criteria,
   applicable Constitution/policy, explicit safety controls, and required
   evidence conditions before it ranks optional knowledge and graph material.
   It refuses a request whose declared budget cannot fit that mandatory
   envelope rather than omitting a safety condition to make a smaller packet.
   Optional candidates are ranked deterministically and included whole,
   highest-first, until the remaining budget is spent.

3. **Every inclusion and exclusion is explainable.** Each included item names
   its source, scope, provenance, freshness, source version, selection reason,
   and any effective relevance used in ranking. Each budget-excluded candidate
   records its identity, ranking inputs, and exclusion reason. Candidates
   rejected because they are unauthorized, stale beyond policy, retired, out
   of scope, or missing required evidence are distinct from budget exclusions.
   A packet never turns absence into evidence that no relevant context exists.

4. **Compilation is deterministic before optional narration.** The base
   compiler uses typed queries, policy evaluation, and the established ranking
   rules; it does not require a model call to return a packet. A configured
   model may produce an optional narration or plan proposal after the typed
   packet exists. That output is separately marked with model and source
   provenance, has its own budget, and cannot remove or reinterpret the
   packet's governing material.

5. **Packets are immutable snapshots with bounded reuse.** A packet can be
   fetched again by id for an authorized audit or to resume the same targeted
   session before expiry. It must not be reused to silently retarget another
   agent, task, repository, or policy version. A changed Constitution/policy,
   revoked delegation, expired task lease, material source advancement beyond
   the packet's declared freshness budget, or expired packet requires a new
   compilation. Reproducibility means an auditor can recover the original
   packet and its source versions; it never means an old packet can replay an
   old action.

6. **Packet requests and outcome reports are authenticated, scoped
   operations.** A supervisor or authorized service requests a packet for its
   own enrolled identity and declared target. The request binds tenant,
   repository, task, agent session, source selectors, budget, and requested
   purpose through an operation-specific authentication domain. Authorization
   checks the requester, target, policy, and data reach before source content
   is selected. A `ContextPacket` identifier is opaque; possession of an id
   grants no cross-tenant or cross-session read access.

7. **Use is reported, not assumed.** An enrolled supervisor reports whether a
   packet was received, accepted, applied to planning, superseded before use,
   refused, or expired. Work evidence, directives, checkpoints, and outcomes
   reference the packet id they used. That closes the causal loop: the
   knowledge plane can learn whether a selection improved an outcome, became
   stale, or should be revalidated. A report is an attributed observation, not
   proof that a task completed correctly.

8. **Packets minimize stored data and have an explicit retention policy.**
   They store references and bounded rendered material, never credentials,
   unrestricted terminal output, or raw source trees. Sensitive rendered
   content is protected by tenant-scoped access control and retention; logs use
   packet ids, digests, state, and timing rather than packet bodies. A retained
   audit copy does not bypass source-level deletion, export, or redaction
   policy.

9. **Packet state is a projection of authoritative facts, not a second task
   authority.** A packet may state the task and lease state observed at
   compilation, but it cannot grant a lease, decide a claim conflict, approve
   a waiver, or authorize a directive. Commands re-check their own live
   authority when executed. A stale packet is visible context, never a cached
   permission.

## Consequences

- The Industrial plane needs a versioned `ContextService`, packet store,
  compiler input projections, packet-use/outcome records, and Bridge inspection
  views. These are typed domains, not a remote wrapper around local MCP.
- Every agent handoff, restream, or later incident review gains a compact
  answer to "what did this agent know and why?" without preserving arbitrary
  prompts as unbounded historical data.
- Context budgets become a measurable product concern: the system can report
  source selection quality, stale guidance, excluded material, and token use
  rather than treating context cost as invisible prompt construction.
- Control directives can safely reference a packet whose exact contents,
  policy basis, target, expiry, and receipt chain are available for audit.

## Rejected alternatives

**Send an opaque assembled prompt to every agent.** Rejected because it hides
selection, provenance, freshness, and safety omissions, and gives no durable
way to explain a later decision.

**Store only source references and regenerate packet text whenever someone
asks.** Rejected because sources and policies change. It would show an auditor
what the system knows now, not what the agent was actually told then.

**Treat a packet as a signed authorization token.** Rejected because context
is advisory and may be stale. Claims, waivers, delegation, and directives have
their own live authorization contracts.

**Use a model as the primary context compiler.** Rejected because model output
is neither deterministic nor sufficient to prove omission, source reach,
budget enforcement, or policy coverage.
