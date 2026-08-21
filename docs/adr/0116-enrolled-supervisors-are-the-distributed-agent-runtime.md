# ADR-0116: Enrolled supervisors are the distributed agent runtime

- Status: Accepted
- Date: 2026-08-21
- Deciders: MindLeak maintainers
- Accepted: 2026-08-21 by the repository owner, authorized directly in session
   — attributed human adoption after review.
- Depends on: [ADR-0082](0082-ackplane-is-a-standalone-federation-service.md)
  (separate, interruption-tolerant federation),
  [ADR-0083](0083-grpc-is-the-ackplane-node-protocol.md) (node protocol),
  [ADR-0085](0085-node-enrolment-requires-proof-of-possession.md) (node
  enrollment), [ADR-0107](0107-registered-agents-accept-authenticated-control-directives.md)
  (directed managed-agent control)
- Related: [ADR-0098](0098-connection-trust-reuses-the-enrolled-key-oidc-waits.md)
  (connection identity), [ADR-0100](0100-repository-node-owns-one-non-exporting-signer.md)
  (node-held signing material),
  [ADR-0115](0115-human-delegation-bounds-industrial-agent-autonomy.md)
  (delegated authority)

## Context

Industrial agents will run on developer laptops, cloud workers, CI pipelines,
and long-lived services. They differ in operating system, network durability,
process isolation, credential availability, persistence, and lifetime. A
central service cannot safely assume it can reach into one of those machines,
and each agent runtime cannot be expected to independently implement identity,
outbox, directive ordering, checkpointing, receipt semantics, and safety
boundaries correctly.

ADR-0107 intentionally requires a supervisor for managed control, but it does
not yet define the runtime role across persistent and ephemeral hosts. Without
that contract, a pipeline may claim durable delivery without any durable local
state, a browser or coordinator may target an arbitrary host process, and a
new agent framework may silently invent a different meaning for pause,
checkpoint, reconnect, or termination.

## Decision

**An enrolled supervisor is the only Industrial runtime endpoint. It adapts
local, cloud, and pipeline workers to one authenticated, receipted protocol
without making Ackplane a remote shell or requiring every worker to become a
control-plane implementation.**

1. **Node identity is stable; agent sessions and workers are ephemeral.** An
   enrolled node identifies a machine, workload identity, or controlled runner
   pool through ADR-0085 enrollment. A supervisor opens agent sessions beneath
   that node, each with a runtime kind, declared capabilities, task/assignment
   reference, start time, and lifecycle state. A worker is a supervisor-owned
   execution unit within one session; a rerun, replacement machine, or new CI
   attempt creates a new session rather than impersonating an old one.

2. **The supervisor makes outbound authenticated connections.** It opens the
   existing bidirectional gRPC stream to Ackplane, completes the enrolled-key
   challenge, publishes typed records, receives only authorized directives, and
   reports receipts. Ackplane never opens an inbound administrative socket to a
   laptop, runner, container host, or cloud VM. Browsers communicate only with
   Ackplane/Bridge, never directly with supervisors.

3. **The supervisor owns durable delivery semantics honestly.** Persistent
   runtimes maintain a durable local outbound event/receipt outbox and a
   directive inbox keyed by directive id and per-session sequence. They publish
   idempotently and resume from acknowledged positions. An ephemeral pipeline
   must either use a configured durable runner-state facility or advertise that
   it cannot guarantee recovery after process teardown. Ackplane records that
   capability and displays an unflushed or lost-outbox condition as incomplete
   evidence; an ephemeral process never pretends it persisted an event it did
   not publish.

4. **Capabilities are declared, versioned, and enforceable.** Before accepting
   work, a supervisor declares the worker runtimes it can launch, its supported
   directive kinds/versions, checkpoint support, persistence guarantees, data
   classifications, network constraints, and any force-stop capability. The
   supervisor may not advertise a control it cannot enforce. Ackplane selects
   directives only when target capability, delegation, and policy all match;
   unsupported work is refused with a typed reason rather than approximated.

5. **The worker boundary is local and narrow.** A supervisor may start,
   observe, checkpoint, pause, drain, or terminate only a worker it registered
   or its owned child-process tree. It cannot execute arbitrary shell strings,
   attach to unrelated host processes, alter host networking, read arbitrary
   files, or turn a directive into unrestricted environment injection. A
   runtime that needs a broader action declares a separate reviewed capability
   and receives a separate policy/delegation decision.

6. **Every lifecycle transition has a durable receipt.** Session registration,
   worker start, task acceptance, context-packet receipt, checkpoint, pause,
   resume, drain, termination, crash detection, reconnect, and completion
   report typed state plus identifiers, timing, reason, and evidence references
   to Ackplane. A heartbeat proves only recent communication; it never proves
   that a directive or task completed. Bridge distinguishes connected, stale,
   disconnected, draining, paused, failed, and completed from their receipts.

7. **Disconnection follows the federation boundary rather than inventing local
   authority.** A supervisor may continue local work that its existing,
   unexpired delegation permits, while buffering records according to its
   advertised durability. It cannot renew a federated claim, obtain a new one,
   accept an unseen privileged directive, or treat a lost connection as a
   successful outcome. On reconnect, it reconciles positions, redelivers
   idempotently, reports gaps, and obtains fresh authority before resuming work
   outside the surviving delegation.

8. **Signing material stays under the node's control.** The supervisor obtains
   operation signatures through the enrolled node's approved signer facility.
   Private keys, bearer tokens, and secrets are never placed in a directive,
   context packet, log, or worker command line. Rotating, revoking, or losing a
   key changes connection and publication state through the existing enrollment
   contract; it does not create an untracked fallback identity.

9. **The supervisor is runtime-neutral, not an agent framework monopoly.** It
   exposes a small adapter contract to AgentD, Agency, editor agents, pipeline
   steps, and future runtimes. The adapter translates a typed assignment and
   bounded context into that runtime's local invocation and returns typed
   lifecycle events. The supervisor never requires Ackplane to host a model or
   source editor, and an agent framework never defines global task, evidence,
   or control semantics for the fleet.

10. **Failure and upgrade are visible operational states.** The protocol
    declares supervisor and capability versions. An unsupported version,
    corrupt inbox/outbox, clock skew, duplicate directive with changed digest,
    missing checkpoint, or lost worker returns a typed refusal/failure and
    leaves the last known receipt intact. Rollout uses additive protocol
    compatibility and capability negotiation; a coordinator does not assume
    every node upgraded merely because a new feature exists server-side.

## Consequences

- The Industrial product gains a deployable supervisor SDK/daemon and runtime
  adapters rather than requiring each agent integration to reinvent durable
  delivery and safe process control.
- CI and cloud workers become first-class fleet members, but their weaker
  persistence guarantees are visible to operators and evidence evaluation.
- Bridge can show a real operational picture of agents: node/session identity,
  health, current work, capability, delivery durability, checkpoint state, and
  control receipts.
- Implementation needs supervisor registration/session records, capability
  contracts, durable inbox/outbox behavior, typed worker lifecycle events, and
  adversarial tests for cross-worker targeting, replay, lost state, and
  disconnect/reconnect behavior.

## Rejected alternatives

**Let Ackplane SSH into machines or invoke arbitrary cloud/pipeline commands.**
Rejected because it turns the coordination plane into a fleet-wide remote shell
with no portable ownership, replay, or process-scope guarantee.

**Embed the full protocol implementation independently in every agent
framework.** Rejected because every implementation would drift on identity,
outbox, directive ordering, checkpoint, and termination semantics.

**Treat a CI job as a durable desktop node by default.** Rejected because an
ephemeral runner can disappear with its disk. Stating a weaker durability
capability honestly is safer than creating receipts the system cannot recover.

**Use a dropped connection as evidence that a worker stopped.** Rejected
because absence can mean a partition, runner loss, or telemetry delay. Only a
typed lifecycle receipt can establish the effect of a control action.
