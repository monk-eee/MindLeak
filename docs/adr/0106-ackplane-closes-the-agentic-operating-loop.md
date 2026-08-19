# ADR-0106: Ackplane closes the agentic operating loop

- Status: Accepted
- Date: 2026-08-19
- Deciders: MindLeak maintainers
- Accepted: 2026-08-19 by the repository owner, authorized directly in
  session - attributed human adoption after review.
- Amends: [ADR-0089](0089-mindleak-is-an-operating-system-for-agent-coordination.md)
  decisions 2-4 for the Industrial profile (a managed fleet can be guided and
  preempted; the Local profile remains cooperative)
- Refines: [ADR-0105](0105-bridge-is-the-server-version-of-the-vsix.md)
  (the Industrial profile's operating model)
- Depends on: [ADR-0022](0022-learned-knowledge-loop.md) (revalidated learned
  knowledge), [ADR-0026](0026-constitutional-policy-over-mechanistic-ratchets.md)
  (constitutional authority), [ADR-0072](0072-an-advisory-informs-it-does-not-cap-the-verdict.md)
  (knowledge advises rather than legislates),
  [ADR-0086](0086-postgresql-is-the-ackplane-ledger-and-arbiter.md)
  (authoritative PostgreSQL state),
  [ADR-0102](0102-context-is-compiled-not-assembled-by-hand.md)
  (bounded context compilation)

## Context

A one-shot coding agent can be given a repository, retrieval, and a large
prompt and still leave the organisation no wiser. Its useful discoveries live
inside one context window; its novel failures become prose in one transcript;
the next agent repeats them. Adding more repository knowledge to every prompt
improves one attempt but does not create a system that learns, governs, or
changes course.

MindLeak already has the pieces of a different system: decaying evidence,
durable goals and tasks, learned knowledge, a Constitution, policy controls,
conformance, and an authenticated Ackplane ledger. Those pieces become an
agentic operating system only when they form one feedback loop that can observe
the fleet, retain what it learns, guide later work, evaluate outcomes, and
recalibrate.

The agents are the scouts and executors. Ackplane is the durable Industrial
brain and control plane. Bridge is its human decision surface. The distinction
is not marketing: agents may be replaced at any time, while the system's
memory, intent, governance, evidence, and decisions must survive them.

## Decision

1. **The Industrial product is a centralized agentic operating loop.** Its
   canonical cycle is:

   ```text
   observe -> evidence -> learn -> govern -> compile guidance -> steer
           -> evaluate outcomes -> recalibrate -> observe
   ```

   Every Industrial capability must identify where it participates in that
   loop. A feature that merely gives one agent a larger prompt but leaves no
   durable observation, decision, or outcome does not advance the system.

2. **Agents are managed scouts and executors, not the institutional brain.**
   AgentD, Agency, editor-connected agents, and other runtimes inspect code,
   propose or perform work, report evidence, and surface novel conditions.
   They can keep local scratch state, but reusable learning crosses into
   Ackplane through a typed, attributed record before it becomes fleet memory.
   Replacing an agent never discards the task history or what the fleet learned.

3. **PostgreSQL holds the shared durable operating state.** The Industrial
   domain includes goals, tasks, claims, agent sessions, knowledge, context
   packets, Constitution versions, policies, controls, waivers, evidence,
   conformance results, decisions, directives, and outcome receipts. These are
   typed domain stores and projections, not wholesale copies of `graph.db`,
   `spec.db`, source trees, terminal logs, or editor storage.

4. **Learning is evidence-backed and continuously revalidated.** Agent
   observations enter as evidence or candidate lessons. Corroboration,
   provenance, source diversity, time, and outcomes determine whether a lesson
   becomes shared knowledge. Later evidence reconfirms, revises, supersedes, or
   retires it. A novel problem that cannot be resolved from active knowledge is
   escalated as a durable question or design decision instead of being hidden
   in one agent's improvisation.

5. **Learning never silently becomes law.** Shared knowledge may rank context,
   recommend an action, explain a risk, or cause review. It cannot amend the
   Constitution, activate policy, waive a rule, or authorize a destructive
   control by itself. Those transitions keep their attributed amendment,
   approval, control, and waiver contracts. This separation lets the system
   learn rapidly without converting a mistaken correlation into fleet policy.

6. **Guidance is compiled for the work and agent in front of the system.** A
   context compiler selects the objective, acceptance criteria, applicable
   Constitution and policy, current task and lease state, relevant knowledge,
   previous outcomes, evidence requirements, and bounded repository context.
   Every included conclusion carries provenance and freshness. The result is a
   versioned context packet, not a concatenated history dump.

7. **The decision layer may use deterministic rules and configured models.**
   Scheduling, policy resolution, evidence checks, and safety gates stay
   deterministic where their contract requires it. A configured model may
   synthesize plans, prompts, hypotheses, or recommended steering off the
   deterministic ingestion path. Model output is a proposal with model and
   source provenance; it receives only the authority of the human or adopted
   policy that approves it.

8. **Steering and evaluation close the loop.** Ackplane routes work and
   ADR-0107 directives to registered agents. Agents return checkpoints,
   evidence, refusals, failures, and completion claims. Ackplane evaluates them
   against acceptance criteria, policy, and evidence contracts. The resulting
   outcome updates task state and becomes new evidence for knowledge and future
   guidance. A failure therefore changes the next attempt rather than merely
   ending the current one.

9. **Bridge exposes the brain, not only the telemetry.** A person can inspect
   why work was assigned, what knowledge and policy shaped a prompt, which
   agent is acting, what changed, what evidence exists, and why the system
   decided to continue, review, resteer, pause, or terminate. Human decisions
   enter the same durable log as automated ones.

10. **MCP is a Local compatibility surface, not the Industrial nervous
    system.** The VSIX and Local planes may continue to expose stdio MCP tools.
    Industrial agents use versioned SDKs and stateful Protobuf/gRPC streams for
    identity, coordination, evidence, context, control, and acknowledgement.
    Ackplane does not become a remote MCP proxy, and Industrial evolution is
    not constrained to one request/one tool-call interactions.

11. **The Local profile remains lightweight and cooperative.** Nothing here
    makes SQLite, local ingestion, or the VSIX depend on Ackplane. Local agents
    may ignore advice exactly as ADR-0089 originally described. Preemptive
    control applies only to agents that explicitly enrol in an Industrial
    supervisor and accept ADR-0107's authenticated control contract.

## Consequences

- Ackplane's current ledger, enrolment, claims, and structural projection are
  foundations, not the completed OS. PostgreSQL-backed task, knowledge,
  governance, context, decision, directive, and outcome domains remain product
  work.
- ADR-0102's context compiler becomes a central Industrial subsystem: it is how
  accumulated knowledge and governance reach an agent as bounded guidance.
- The system can improve across agents and sessions, but it also acquires a
  powerful central failure domain. PostgreSQL durability, authentication,
  authorization, auditability, backup, and disaster recovery are load-bearing.
- Model-assisted decisions require evaluation and provenance. A model being
  available never bypasses deterministic policy or evidence requirements.
- Success metrics move from single-agent completion rates toward closed-loop
  outcomes: repeated-problem rate, useful knowledge reuse, resteer recovery,
  evidence quality, policy conformance, and time to resolve novel conditions.

## Rejected alternatives

**Treat one-shot repository-grounded agents as the product.** Rejected because
each run may know the repository while the fleet still forgets outcomes,
governance, and novel failures between runs.

**Put all known text into every prompt.** Rejected because volume is not
learning, stale guidance is dangerous, and unbounded context hides authority
and provenance. Guidance is compiled for one decision.

**Let repeated knowledge automatically become policy.** Rejected because a
correlation, even a useful one, has not passed constitutional adoption and may
be wrong or stale.

**Use remote MCP as the Industrial control fabric.** Rejected because a tool
inventory does not provide a durable ordered stream, live identity, resumable
delivery, control acknowledgements, or the feedback loop the server needs.

**Make every agent carry its own independent governance and memory.** Rejected
because N private interpretations produce N authorities and guarantee that one
agent's learning cannot reliably change another agent's next action.
