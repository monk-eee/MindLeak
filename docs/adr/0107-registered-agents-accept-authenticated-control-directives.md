# ADR-0107: Registered agents accept authenticated control directives

- Status: Accepted
- Date: 2026-08-19
- Deciders: MindLeak maintainers
- Accepted: 2026-08-19 by the repository owner, authorized directly in
  session - attributed human adoption after review.
- Amends: [ADR-0083](0083-grpc-is-the-ackplane-node-protocol.md) decision 3
  (this is the reviewed server-initiated command contract it reserved),
  [ADR-0089](0089-mindleak-is-an-operating-system-for-agent-coordination.md)
  decision 3 for enrolled Industrial agents (Local remains cooperative)
- Refines: [ADR-0105](0105-bridge-is-the-server-version-of-the-vsix.md)
  clauses 5 and 10 (Bridge coordination includes direct managed-agent control)
- Depends on: [ADR-0084](0084-ackplane-evidence-has-explicit-trust.md)
  (authentication and trust), [ADR-0085](0085-node-enrolment-requires-proof-of-possession.md)
  (enrolled keys), [ADR-0098](0098-connection-trust-reuses-the-enrolled-key-oidc-waits.md)
  (connection trust), [ADR-0106](0106-ackplane-closes-the-agentic-operating-loop.md)
  (the decision and learning loop that emits guidance)

## Context

Coordinating a managed fleet requires more than observing agents and waiting
for them to poll. The Industrial system must communicate with a specific
registered agent, deliver new knowledge-grounded guidance, change its
assignment, pause or drain it, and terminate it when continuing is unsafe or
wasteful. Bridge is the human control surface; Ackplane is the authority and
delivery plane.

That power changes the threat model. A forged prompt can redirect source
changes. A replayed termination can destroy useful work. An unauthenticated
browser action can stop an entire fleet. A generic remote shell would turn one
server compromise into arbitrary execution on every enrolled machine. TLS,
enrolled identity, authorization, typed gRPC messages, replay protection, and
durable receipts are therefore the feature, not deployment polish.

## Decision

1. **Registered agents have one authenticated bidirectional control channel.**
   ADR-0083's `NodeSyncService.Synchronize` stream carries a closed
   `AgentDirective` family from Ackplane and `DirectiveReceipt` results from the
   enrolled node. The capability handshake declares supported directive kinds
   and versions before any are sent. Browser clients never connect directly to
   an agent.

2. **All three parties are authenticated before control is possible.**
   Outside loopback, the agent validates Ackplane's TLS server identity and all
   traffic uses TLS. Ackplane binds the stream to tenant, repository, node, and
   signing key through ADR-0098's challenge signed by the active enrolled key.
   The Bridge or API caller presents a verified operator or service principal;
   Ackplane authorizes that principal for the target project, repository,
   agent, and directive kind. A salted development tenant token is not
   production authorization for destructive controls.

3. **The vocabulary is closed and typed.** The initial directive classes are:

   - `Notify` - deliver a durable human or system message;
   - `Prompt` - deliver a bounded context packet or follow-up instruction;
   - `Assign` / `Steer` - start or change the current objective and task;
   - `Pause` / `Resume` - suspend and continue work without discarding state;
   - `Drain` - finish or checkpoint the current safe unit and accept no new work;
   - `Terminate` - stop the enrolled worker under clause 9.

   There is no shell command, terminal input, arbitrary process id, environment
   injection, source patch, raw MCP request, or untyped `execute` payload.

4. **Every directive is an immutable durable record before delivery.** It
   carries a directive id, tenant, project/repository, target node and agent
   session, directive kind and version, issuing principal, rationale, task and
   goal references, creation and expiry times, per-agent sequence, idempotency
   key, payload digest, required capability, and the policy, knowledge,
   evidence, or context-packet references that informed it. PostgreSQL is the
   authority; a web response alone never means an agent acted.

5. **Delivery is ordered, resumable, expiring, and idempotent.** Ackplane sends
   a directive at least once in monotonically increasing per-agent order. The
   supervisor deduplicates by directive id and returns the original receipt for
   a replay. Reconnection resumes after the last acknowledged sequence. An
   expired directive is never applied, a gap is requested rather than guessed,
   and disconnecting is not reported as successful termination.

6. **Receipts distinguish acceptance from effect.** An agent or supervisor
   replies `accepted`, `refused`, `applied`, `failed`, or `expired`, with a
   typed reason, timestamp, checkpoint/evidence references, and observed
   payload digest. Ackplane appends every transition. Bridge displays pending
   delivery separately from acknowledged application.

7. **Dynamic prompts are compiled guidance, not opaque improvisation.** A
   `Prompt` references an immutable bounded context packet containing the
   objective, task, applicable Constitution and policy, selected active
   knowledge, evidence requirements, and relevant prior outcomes. Included
   statements carry provenance and freshness. Model-synthesized text is marked
   with model provenance and the human or adopted policy that authorized its
   delivery. Secrets, raw credentials, and unbounded terminal output are never
   prompt inputs.

8. **Steering preserves history.** `Steer` does not rewrite what the agent was
   previously asked to do. The supervisor checkpoints or refuses the current
   unit, and Ackplane records the old assignment, new assignment, issuer,
   reason, context packet, and resulting evidence window. Resteering without a
   durable causal chain is indistinguishable from prompt drift and is refused.

9. **Termination is scoped to an enrolled supervisor.** Ackplane never sends a
   host process id. `Terminate` asks the authenticated local supervisor to stop
   the worker it registered. The first phase requests a bounded graceful
   checkpoint and shutdown. A force phase is permitted only when the supervisor
   advertised that capability, the directive carries a high-risk permission
   and explicit reason, and the graceful deadline elapsed or an adopted safety
   policy authorizes immediate force. The supervisor may terminate only that
   registered worker or its owned child process tree, never arbitrary host
   processes, containers, or other agents.

10. **Authorization is directive-specific and fail-closed.** Read access to a
    Fleet view grants no control. Prompt, steering, pause, and termination are
    separate permissions. Automated decision services act only through an
    adopted policy delegation naming allowed directives and scope; knowledge
    alone never grants authority. Missing principal identity, stale policy,
    tenant mismatch, revoked node key, unsupported capability, or unavailable
    authorization refuses the directive.

11. **Transport and replay protections are tested as safety properties.** TLS
    is mandatory outside loopback. Connection challenges are single-use and
    domain-separated. Directive sequences, ids, expiry, payload digests, and
    receipts prevent cross-agent, cross-operation, and stale replay. Tests must
    attempt forged principals, wrong tenants/repositories, revoked keys,
    duplicate ids with changed payloads, expired directives, sequence gaps,
    reconnect replay, unsupported controls, and termination outside the owned
    worker tree.

12. **Sensitive directive bodies are not transport logs.** Operational logs
    record ids, kinds, principals, targets, outcome, latency, and digests.
    Prompt/context bodies are stored only in their governed durable store with
    access control and retention; credentials and secrets are never accepted as
    directive content. Backup and audit preserve the decision chain without
    spraying prompts through logs.

13. **Local remains cooperative.** A Local VSIX agent that has not enrolled a
    supervisor exposes no remote control channel and cannot be killed or
    resteered by Ackplane. Installing the VSIX never opts a process into
    Industrial control. Enrolment, advertised capabilities, and selected
    Industrial authority are explicit setup acts.

## Consequences

- A real agent supervisor/daemon is required. An ordinary MCP server or agent
  process cannot reliably authenticate a stream, checkpoint a worker, and
  enforce graceful then forced termination of itself.
- The protobuf contract gains directive and receipt messages, capability
  negotiation, sequence resumption, and typed refusal reasons. Compatibility
  remains additive within `v1` or moves to a new package when semantics break.
- Bridge needs verified operator authentication and directive-specific
  authorization before its control UI can leave loopback development. The
  current development tenant token is insufficient for destructive actions.
- PostgreSQL gains directive, delivery, receipt, and context-packet state. This
  is part of the evidentiary chain used to explain who controlled an agent and
  what happened next.
- Compromise has fleet-wide consequences, so key rotation, revocation, TLS,
  least privilege, audit, rate limits, and emergency disablement are release
  gates for control, not follow-up hardening.

## Rejected alternatives

**Expose a remote shell or raw command string.** Rejected because it grants
arbitrary host execution, cannot be authorized by operation, and makes safe
replay or evidence semantics impossible.

**Let Bridge connect directly to agents.** Rejected because it bypasses
Ackplane authorization, durable ordering, tenancy, receipts, and the one
authoritative decision log.

**Use HTTP polling, WebSockets, or remote MCP for control.** Rejected because
the authenticated gRPC stream already owns node identity, ordering,
reconnection, capability negotiation, and typed failure semantics.

**Treat TLS alone as authorization.** Rejected because encryption and server
identity do not prove which operator may terminate which agent. Principal,
tenant, repository, target, and directive permissions are all required.

**Infer termination from a dropped heartbeat.** Rejected because absence can
mean a partition or crash. Only an applied termination receipt proves the
supervisor acted.

**Allow knowledge or a model to issue destructive controls directly.**
Rejected because learning informs decisions but is not authority. A human or
adopted policy delegation must authorize the directive and remain in its audit
chain.
