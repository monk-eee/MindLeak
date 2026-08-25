# ADR-0125: Bridge Work commands are principal-scoped and receipted

- Status: Proposed
- Date: 2026-08-25
- Deciders: Pending human acceptance
- Depends on: [ADR-0095](0095-the-bridge-uses-an-authenticated-projection-api.md)
  (the Bridge's principal-scoped browser boundary),
  [ADR-0105](0105-bridge-is-the-server-version-of-the-vsix.md) (the Work
  control-room parity promise),
  [ADR-0107](0107-registered-agents-accept-authenticated-control-directives.md)
  (typed directives and receipts for device-bound effects),
  [ADR-0115](0115-human-delegation-bounds-industrial-agent-autonomy.md)
  (human-approved delegation), and
  [ADR-0120](0120-industrial-work-domain-is-an-authoritative-task-projection.md)
  (the authoritative Industrial Work domain)
- Refines: ADR-0120 decision 8 (commands are deliberately deferred until a
  separate contract defines their authority and safety properties)
- Related: [ADR-0111](0111-bridge-recovers-a-stranded-claim-as-a-tenant-scoped-administrative-action.md)
  (the narrower, store-safe stranded-claim recovery exception),
  [ADR-0116](0116-enrolled-supervisors-are-the-distributed-agent-runtime.md)
  (the supervisor owns device-bound worker effects), and
  [ADR-0119](0119-industrial-administration-lifecycle-policy.md) (the
  request-and-receipt pattern for privileged Industrial operations)

## Context

ADR-0105 decision 5 makes the Bridge the human control room for Industrial
Work: an authorized operator can create and route work, allocate it, recover or
release leases, answer durable questions, and follow review and completion.
ADR-0120 deliberately shipped the prerequisite Work namespace as a bounded,
read-only projection. Its decision 8 says that reading a Work task does not
authorize creating, assigning, pausing, answering, reviewing, or completing
one. Each mutation needs a distinct authority, concurrency, idempotency,
receipt, and local-effect contract.

That deferral is still correct. The current Bridge developer profile derives a
single loopback tenant token; it is useful for tenant-scoped read views and the
exception in ADR-0111, but it does not identify an accountable operator.
`WorkStore::create_task` and `ClaimStore` expose storage primitives, not browser
permissions. Calling either directly from an HTTP handler would make a browser
session the authority for a fleet task without a verified principal, an active
delegation, a task-version comparison, a durable command record, or evidence of
what an enrolled worker actually did.

The existing recovery route is not a general precedent for that shortcut.
ADR-0111 permits only `recover` under the current developer profile because
`ClaimStore::recover` unconditionally refuses a live lease based on the
database clock. That one-way, store-enforced predicate is its safety boundary.
Creating or routing Work, releasing a lease, answering a wait, changing review
state, or directing a worker has no equivalent caller-independent guarantee.

ADRs 0107, 0115, and 0116 already decide the missing pieces separately:

- a device-bound action reaches an enrolled supervisor only as a closed,
  authenticated directive and later receives a typed receipt;
- an automated decision must remain inside an active human-approved delegation;
- a verified human principal or an authorized organizational process supplies
  the accountable authorization basis for consequential work.

What remains undecided is the command boundary that joins those decisions to
the Industrial Work projection. Without it, a future implementation could make
the apparently expedient but unsafe choices: a generic `POST /work/action`, a
browser-to-PostgreSQL shortcut, a proxy to Local Lodestar, or a UI that claims a
pause or assignment succeeded before a supervisor receipt exists.

## Decision

**Industrial Work commands are a closed, principal-scoped request-and-receipt
domain. A Bridge route may request one only after verified authorization and
task-version validation; Ackplane records the request durably before effect;
and the response distinguishes accepted command delivery from a completed
server or supervisor effect. This ADR authorizes no implementation while it is
Proposed.**

1. **The command vocabulary is closed and separated by effect owner.** The
   initial contract contains only these named operations:
   - `CreateWork`: create a new Industrial Work record, never a Local Lodestar
     task;
   - `RouteWork`: select a bounded Industrial route or assignment reference;
   - `ReleaseLease`: release the current Industrial claim under a guarded
     administrative workflow;
   - `AnswerWait` and `SubmitReview`: append the named Work-domain answer or
     review transition;
   - `Assign`, `Steer`, `Pause`, `Resume`, and `Drain`: request a matching
     ADR-0107 directive from an enrolled supervisor.

   `RecoverClaim` remains ADR-0111's separate store-safe route and is not
   broadened by this contract. `DelegateClaim` and `RenewClaim` remain
   node-signed ClaimStore operations: an operator does not acquire or extend a
   node's lease in that node's name. `Terminate`, policy/delegation changes,
   Constitution changes, waivers, retention actions, and arbitrary workflow
   transitions remain outside this first command set and need their own
   decision.

2. **Every command has a verified authorization basis.** A remotely reachable
   Bridge command requires a verified principal resolved by the ADR-0095
   authentication verifier, tenant and repository permission, and an
   operation-specific policy permission. An automated requester additionally
   needs an active ADR-0115 delegation naming the operation, target scope,
   limits, expiry, and required evidence. A direct verified human request is
   recorded as such; it is not silently rewritten as an agent delegation.

   The current loopback development tenant token does not satisfy this
   requirement. Under that profile, every command in this ADR is unavailable
   with a typed `authorization_unavailable` result and an explanation of the
   missing verifier or delegation. ADR-0111's expiry-constrained recovery
   exception remains exactly as adopted; it is not a fallback authorization
   path for any command here.

3. **Tenant and repository authorization precede every target lookup.** The
   command service checks the principal's tenant and repository reach before it
   reads a Work task, claim, wait, review, directive, or receipt. A cross-tenant
   or out-of-scope request is refused without revealing whether its target
   exists. The authoritative command service, not a route handler or browser,
   performs this check so gRPC, HTTP, and future clients cannot grow different
   meanings of scope.

4. **A command request is an immutable, bounded record.** Ackplane assigns a
   command id and records at least the command kind and schema version,
   tenant/repository/task scope, verified principal, optional delegation and
   policy references, idempotency key, canonical payload digest, bounded
   rationale, requested and expiry times, expected task version when applicable,
   confirmation reference when required, and the resulting receipt reference.
   Payloads contain only operation-specific bounded fields and durable
   references. They never accept arbitrary JSON patches, Local task databases,
   thread transcripts, source text, credentials, shell commands, raw MCP
   requests, or unconstrained prompt content.

5. **Create uses idempotency; every existing task mutation uses compare and
   swap.** `CreateWork` has no prior task version, so its caller provides a
   scoped idempotency key. Retrying identical canonical content returns the
   original command and receipt; reusing the key with changed content is
   refused. Every command targeting an existing Work task includes an
   `expected_task_version` derived from the authoritative Work event/projection
   version, never from a browser clock or `updated_at` display string. The
   command service performs authorization, version comparison, Work event
   append, projection update, and receipt creation in one transaction for a
   server-owned effect. A stale version returns a typed conflict containing the
   current safe-to-disclose version; it never overwrites a later task change.

6. **Lease actions preserve ClaimStore as the sole lease authority.**
   `ReleaseLease` names the expected claim owner and lease/version the operator
   observed, requires a non-empty rationale and confirmation, and executes its
   ClaimStore compare-and-swap with the related Work event and command receipt
   atomically. A changed owner, renewed lease, expired lease, or missing claim
   returns a typed conflict or refusal. Work commands never write a second lease
   column, infer a lease from a heartbeat, or make the browser's displayed owner
   authoritative.

7. **Supervisor-directed operations are not immediate Work transitions.**
   `Assign`, `Steer`, `Pause`, `Resume`, and `Drain` first create the durable
   Work command and an ADR-0107 directive addressed to one enrolled supervisor
   whose advertised capability, task scope, and delegation all match. The
   immediate receipt may say `accepted` or `pending_delivery`; it does not say
   the worker paused, resumed, or accepted an assignment. Only the supervisor's
   typed `applied`, `refused`, `failed`, or `expired` directive receipt may
   append the corresponding Work event and change its projection. A disconnected
   node, unsupported capability, expired directive, or lost delivery remains a
   visible result, never an optimistic state change.

8. **Confirmation is command-specific and expires.** A consequential command
   - releasing a live lease, routing or steering active work, changing review
   disposition, or sending a supervisor directive - first produces an immutable
   preview containing its exact scope, current task version, principal and
   delegation basis, rationale, expected effect, and payload digest. The caller
   confirms that one digest before its short expiry. Changing any field,
   authorization basis, or task version requires a new preview. Low-risk
   `CreateWork` may be accepted without a second confirmation only when the
   verified policy explicitly classifies it as routine; an unclassified action
   is escalated under ADR-0115 rather than assumed safe.

9. **Escalation and receipts are first-class outcomes.** The command service
   records and exposes `refused`, `pending_confirmation`, `pending_delivery`,
   `accepted`, `applied`, `failed`, `expired`, and `conflicted` outcomes. A
   missing principal, delegation, capability, evidence, confirmation, or policy
   creates an ADR-0115 human decision request when the policy permits
   escalation; otherwise it produces a durable refusal. An HTTP response
   returns the durable command and receipt identifiers plus current state, not a
   claim that an asynchronous worker effect already happened.

10. **Bridge controls reflect this authority model.** The UI shows the current
    principal, tenant/repository scope, policy/delegation basis, command
    preview, confirmation requirement, request state, final receipt, and safe
    refusal reason. It disables unavailable controls rather than hiding their
    capability gap, rolls optimistic display state back on conflict or refusal,
    and uses accessible status/error reporting. It never connects directly to a
    supervisor, PostgreSQL, Local SQLite, MCP, a terminal, or a shell.

11. **Implementation arrives as independently testable slices.** The first
    implementation must add an authoritative command service and durable schema
    before exposing any route. Tests must cover forged and missing principals,
    tenant/repository scope, revoked or expired delegations, same-key changed
    payloads, exact retries, stale task/claim versions, confirmation expiry,
    command-to-directive sequencing, unsupported capabilities, reconnect
    delivery, receipt provenance, and browser disabled/refusal states against a
    real PostgreSQL instance. A route that reaches `WorkStore` or `ClaimStore`
    directly without this service is a contract violation.

## Consequences

- ADR-0120's read-only Work surface gains a precise implementation gate rather
  than a vague future promise. The next Work-control task can be decomposed by
  command family without redefining principal scope, receipt meaning, or task
  concurrency at every endpoint.
- The current Bridge developer profile remains honest: it can show why an
  operation is unavailable, but cannot turn a loopback tenant token into an
  operator identity or a standing delegation.
- Ackplane needs a durable Work-command request/receipt domain, an
  authorization/delegation evaluator, task-version projection support, and an
  explicit transaction boundary joining server-owned Work and Claim changes.
  Supervisor-directed actions additionally use the existing typed directive and
  receipt channel rather than a new device-control transport.
- Existing `recover` behavior remains narrowly available under ADR-0111. It is
  not silently migrated to this command model until a later implementation
  deliberately reconciles the two receipt histories.
- The Work-controls umbrella may now be split into small implementation tasks:
  command persistence and idempotency, verified-principal authorization,
  server-owned create/release/wait/review actions, supervisor directive
  issuance, and Bridge controls. None should ship before this proposal is
  accepted.

## Rejected alternatives

**Call `WorkStore` or `ClaimStore` directly from a Bridge route.** Rejected
because a storage method is not an authorization model. It would omit verified
principal scope, delegation, immutable request identity, confirmation, and the
receipt proving what happened.

**Reuse a node signing key or proxy node-signed gRPC from the browser.**
Rejected because an operator is not the node that owns the key or the claim.
Borrowing that identity violates ADR-0100's non-exporting signer boundary and
would falsely attribute an administrative action to an enrolled node.

**Expose one generic Work action endpoint.** Rejected because a free-form
`action`, state patch, or JSON payload cannot be authorized, validated,
confirmed, idempotently replayed, or receipted with operation-specific meaning.
Adding a new command kind requires an explicit contract amendment, not a string
the browser may invent.

**Proxy Local Lodestar task mutations through Ackplane.** Rejected because it
would recreate the two-authority task mirror ADR-0120 forbids. This contract
applies only to Industrial Work records and their authoritative claims.

**Treat command acceptance as an applied worker effect.** Rejected because a
supervisor can be disconnected, unsupported, expired, or refuse the directive.
Only its ADR-0107 receipt can establish an effect on a device-bound worker.

**Allow a loopback development token to authorize Work controls permanently.**
Rejected because network location and a tenant selector do not identify the
accountable principal, policy permission, or human-approved delegation ADR-0115
requires for consequential Industrial actions.
