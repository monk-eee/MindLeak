# ADR-0089: MindLeak is an operating system for agent coordination

- Status: Accepted
- Date: 2026-08-10
- Deciders: MindLeak maintainers
- Accepted: 2026-08-10 by the repository owner — attributed human adoption after
  review.
- Related: [ADR-0028](0028-external-adoption-evidence-gate.md) (claim tiers and
  evidence discipline), [ADR-0045](0045-a-fleet-is-a-distributed-system.md) (a
  fleet is a distributed system), [ADR-0059](0059-the-tool-surface-is-a-vocabulary.md)
  (the tool surface is a vocabulary), [ADR-0024](0024-preflight-overlap-detection.md)
  (advisory overlap), [ADR-0054](0054-identity-is-the-session-not-the-process.md)
  (session identity), [ADR-0082](0082-backplane-is-a-standalone-federation-service.md)
  (federation boundary), [ADR-0084](0084-backplane-evidence-has-explicit-trust.md)
  (attribution versus authentication),
  [ADR-0071](0071-task-resolution-records-an-unverified-reviewer-label.md)
  (unverified reviewer label)

## Context

MindLeak currently describes itself as *local context infrastructure for coding
agents*. That is accurate and it undersells the system: it names a component
inside someone else's architecture rather than the category the product occupies.
It also fails to explain why memory, intent, claims, identity, policy, and
evidence belong in one product at all, which makes the surface look like an
accumulation of features instead of one idea.

Two things have changed since that wording was chosen.

First, the failures we actually fix turned out to be operating-system failures.
ADR-0045 already recorded this: lost updates, ID collisions, stale reads, split
registries, and lease expiry, each wearing a Git costume. The remedies were
scheduling, arbitration, identity, and isolation — the concerns an OS exists to
provide.

Second, the Ackplane series extends coordination across machines and
organisations. A product that arbitrates claims, resolves identity, governs
permissions, schedules work, ages memory, and keeps an audit journal is not
describing itself well as "infrastructure for context".

The hazard in the stronger frame is precise. A real operating system preempts,
isolates, and enforces. MindLeak does none of those. Claims are advisory and
never filesystem locks (ADR-0024). Overlap detection warns and cannot block. The
local planes are unauthenticated by design and record attribution rather than
identity (ADR-0084), of which the unverified reviewer label is one instance
(ADR-0071). An unclaimed agent that ignores every warning still writes to the
disk. Adopting OS vocabulary
without stating that boundary would manufacture exactly the impression
ADR-0028 exists to prevent, and would be the most expensive kind of overclaim:
one a security reviewer discovers rather than one we disclosed.

## Decision

1. **The product category is an operating system for agent coordination.** The
   one-line frame is: *MindLeak is the operating system for coordinating
   autonomous coding agents — it gives a fleet shared memory, durable intent,
   arbitrated work, and provable completion.* It is the layer agents run on top
   of, not an agent and not a model.

2. **The correspondence is stated with its limits, never implied.** Whenever the
   category is used in documentation or product material, this mapping is the
   canonical explanation:

   | OS concept | MindLeak realisation | Honest limit |
   |---|---|---|
   | System-call surface | MCP tool vocabulary (ADR-0059) | A client may ignore it; nothing traps |
   | Scheduler and run queue | Tasks, claims, leases, next-task allocation | Cooperative; no preemption or forced yield |
   | Locks and mutual exclusion | Claims plus pre-flight overlap (ADR-0024) | Advisory; never a filesystem or Git lock |
   | Memory hierarchy and eviction | Decay graph, working set, consolidation, pruning | Eviction by relevance, not by correctness |
   | Process identity | Session identity (ADR-0054), enrolled nodes (ADR-0085) | Local is attribution; only Ackplane authenticates |
   | Permissions and policy | Constitution clauses, consequences, waivers | Governs verdicts; does not intercept writes |
   | Journal and audit | Evidence bundles, conformance receipts, task event log | Records what happened; prevents nothing |
   | Device drivers | Passive terminal and Git sensors (ADR-0011) | Best-effort; degrade visibly when unsupported |
   | IPC and networking | Durable task thread (ADR-0046), Ackplane federation | Asynchronous; no direct agent-to-agent channel |

3. **Cooperative, not preemptive, is part of the claim.** Any statement of the
   category carries the constraint that MindLeak coordinates willing clients. We
   do not write "enforces", "prevents", "guarantees", "sandboxes", or "blocks"
   about agent behaviour. Constitutional consequence may block a *verdict* and a
   publication gate may block a *merge*; neither stops a process from editing a
   file.

4. **The metaphor is a design constraint, not decoration.** A proposed
   capability must map to a coordination primitive in the table above, or extend
   it deliberately with its own decision. "It would be useful" is not sufficient
   justification for a feature that belongs in an agent, an editor, a CI system,
   or a model. This gives the repositioning a stopping rule rather than a licence
   to grow.

5. **Product names and the capability vocabulary are fixed.** *MindLeak Core* is
   the local pair of planes. *Ackplane* is the shared control plane, named for
   the acknowledgement stream it actually is (ADR-0083) rather than the passive
   bus a backplane would be. *The Bridge* is the human operations interface.
   Inside those, each capability has a name and the question it answers:

   | Capability | Name | Answers |
   |---|---|---|
   | Memory | MindLeak | What happened? |
   | Intent | Lodestar | What are we trying to achieve? |
   | Coordination | Beacon | Who is working on this? |
   | Governance | Gatekeeper | Is this allowed? |
   | Evidence | Librarian | Where is the proof? |
   | Verification | Verifier | Does the evidence prove it? |

   These are capability names inside the two planes, not deployable services and
   not separate products. Beacon, Gatekeeper, Librarian, and Verifier are all
   `lodestar-core` today; naming a capability never authorises splitting it into
   a service, and ADR-0004 still decides where a process boundary belongs.
   Gatekeeper's question is answered with advice and a verdict, never an
   interception, because clause 3 still holds. Nothing is named *Certifier*:
   certification is a status a subject holds, not a component that issues it
   (ADR-0090).

   A capability name earns its place if a dashboard widget can be named after
   it — Beacon Conflicts, Gatekeeper Decisions, Librarian Evidence, Lodestar
   Goals, MindLeak Context. A name that reads awkwardly as a widget is naming
   the wrong thing.

6. **Claim discipline is unchanged.** ADR-0028's tiers still bind every
   measurable statement. A category frame is a description of what the system
   is, and it earns no percentage, no efficacy claim, and no adoption claim.
   Repositioning does not convert engineering-tier evidence into product-tier
   proof, and the evaluation pages keep their scope labels.

7. **On acceptance, the surfaces change in one reviewed change.** `README.md`,
   `AGENTS.md`, `docs/ARCHITECTURE.md`, and the extension marketplace copy adopt
   the frame together, so the product does not describe itself two ways at once.
   Nothing was rewritten before this attributed acceptance; that change follows
   it and is not bundled into the decision record.

8. **What does not change.** The zero-token deterministic write path, decay as a
   feature, derived effective weight, optional and off-path models, plane
   separation, local-first operation without account or network, and stdio MCP
   transport. Repositioning is a claim about what the system *is*, never
   permission to relax an invariant that makes the claim true.

## Consequences

- The surface stops reading as an inventory. Memory, intent, claims, identity,
  policy, and evidence become one coherent story, which is also the honest
  reason they belong in one product.
- Buyers and contributors get a category they already understand, and a mapping
  table that tells them precisely where the analogy stops.
- Clause 4 makes some attractive work explicitly out of scope, which is the
  point. A coordination OS that starts writing code, running pipelines, or
  hosting models has stopped being one.
- The cooperative boundary must be repeated in places marketing prose would
  rather leave it out. That repetition is the cost of being believed by the
  security reviewer who checks.
- "Operating system" invites the question of what happens when an agent ignores
  the OS. The answer — advisory coordination plus retrospective evidence — is
  now a stated design position instead of a gap discovered later.
- Existing documentation that says "context infrastructure" becomes inconsistent
  until the acceptance change lands, so that change should follow acceptance
  promptly rather than drifting.

## Rejected alternatives

**Keep "local context infrastructure for coding agents".** Rejected because it
describes a subsystem, hides the coordination and governance value, and gives no
reason for the planes to be one product.

**Position as an agent framework or an agent platform.** Rejected because
MindLeak deliberately does not run, host, or implement agents. That category
sets the expectation of orchestration and execution the product refuses.

**Position as an AI memory product.** Rejected because memory is one plane, and
the market meaning of the term has collapsed to "vector store", which is the
comparison ADR-0002 exists to reject.

**Position as a compliance or governance platform outright.** Rejected as the
primary category because governance is the first commercial use case rather than
the whole system; a coordination substrate that only sells to auditors would
lose the developer surface that produces the evidence in the first place.

**Adopt the OS frame without the cooperative limitation.** Rejected because it
would imply isolation and enforcement the product does not provide, which is an
overclaim under ADR-0028 and a security misrepresentation.

**Wait for external adoption evidence before repositioning.** Rejected because
the tiers govern *measurable outcome claims*, not how a product describes its own
architecture; and the current description is already actively misleading about
scope.
