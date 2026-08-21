# ADR-0115: Human delegation bounds Industrial agent autonomy

- Status: Accepted
- Date: 2026-08-21
- Deciders: MindLeak maintainers
- Accepted: 2026-08-21 by the repository owner, authorized directly in session
   — attributed human adoption after review.
- Depends on: [ADR-0106](0106-ackplane-closes-the-agentic-operating-loop.md)
  (the Industrial operating loop),
  [ADR-0107](0107-registered-agents-accept-authenticated-control-directives.md)
  (typed control directives)
- Related: [ADR-0086](0086-postgresql-is-the-ackplane-ledger-and-arbiter.md)
  (durable authority),
  [ADR-0113](0113-the-industrial-knowledge-plane-is-evidence-backed-and-human-governed.md)
  (knowledge informs, not authorizes)

## Context

A large fleet should not require a person to approve every retrieval, plan
step, test run, or ordinary work assignment. That would replace agent delay
with human queueing and ensure the fleet never becomes more effective than one
operator. The alternative, unbounded agent autonomy, is equally unacceptable:
a useful lesson, a model suggestion, or a high-confidence score cannot be
allowed to widen access, override policy, ship an irreversible change, or
silently change who is accountable.

The human must therefore be part of the control system at the right level. A
human defines the operating envelope, approves the policies and delegations
that allow routine work, reviews exceptions and consequential actions, and can
observe or intervene while work is running. The system must make that role
visible in the Bridge and durable in the ledger rather than treating it as a
chat approval or a hidden configuration toggle.

## Decision

**Industrial agents act only inside explicit, attributable human-approved
delegation. Humans govern the envelope and exceptions; agents execute,
recommend, and report within it.**

1. **Delegation is a durable, versioned authority record.** A delegation names
   the issuing verified human principal or authorized organizational process,
   tenant, repository/project/task scope, allowed action categories, required
   controls and evidence, risk/budget limits, effective time, expiry, and
   revocation state. It references the adopted Constitution/policy version
   that authorizes the delegation. A model output, knowledge record, task
   assignment, browser session, or agent assertion is not a delegation.

2. **The delegation describes a bounded operating envelope, not a blank
   approval.** It may permit an enrolled agent to retrieve context, perform
   routine analysis, claim or work on a named task, create candidate knowledge,
   run declared validation, or make another explicitly named low-risk action.
   It must identify the target scope and limits rather than grant generic
   "agent access." Permissions, repositories, data classifications, external
   effects, cost ceilings, and control classes outside the envelope remain
   refused until an appropriate human decision or separately adopted policy
   exists.

3. **A human-approved policy may automate routine decisions, but policy
   adoption remains human governance.** A policy can define an evidence
   threshold, risk class, or routing rule under which Ackplane issues a
   permitted directive or accepts a lifecycle transition. The resulting action
   records both the policy/delegation basis and the automated decision inputs.
   An agent never edits the policy that authorized its own action, and a
   policy cannot silently expand beyond its approved scope through model or
   knowledge inference.

4. **Consequential boundaries require a human or an explicitly higher-trust
   delegation.** At a minimum, the default policy refuses or escalates changes
   to Constitution, policy, waivers, authority grants, identity/key state,
   tenant/repository reach, sensitive-data export or retention, force
   termination, irreversible external effects, and exceptions to required
   evidence or safety controls. An organization may define narrower or
   additional risk classes, but it may not treat an unclassified action as
   routine by default.

5. **Escalation is a first-class durable state.** When an action is outside a
   delegation, lacks evidence, conflicts with policy, encounters an ambiguous
   authority, exceeds a budget, or needs an exception, Ackplane creates a
   human decision request. It names the proposed action, target, reason,
   supporting context packet, evidence, alternatives, expiry, and the safe
   behavior while waiting. The request appears in the Bridge's human queue;
   it is not hidden in agent logs or converted into another prompt.

6. **No response is not an approval.** A waiting agent follows the safe action
   prescribed by the relevant policy: continue only inside an existing,
   unexpired delegation, checkpoint and pause, drain, or refuse. It does not
   widen scope, retry a rejected privileged command with altered wording, or
   infer consent from a missing response. Expiry and revocation prevent new
   delegated actions; a control that affects an active worker uses ADR-0107's
   typed pause, drain, or termination contract and its receipt semantics.

7. **Humans have an inspectable real-time intervention surface.** Bridge shows
   the fleet's active delegation, pending decisions, policy version, context
   packet, current task/lease, evidence, directive state, and projected impact
   before a person approves, refuses, pauses, resumes, drains, or terminates
   work. Each intervention is an authorized command with confirmation for
   consequential actions, a durable rationale, and a receipt. A dashboard
   indication that a command was sent is not evidence that an agent applied it.

8. **Separation of duties is structural.** The agent or automated decision
   service that proposed an exception cannot approve that same exception. A
   human may delegate bounded routine authority, but approval of a delegation
   change, waiver, or high-risk override requires a distinct authorized
   principal as the governing policy specifies. Every read, decision, command,
   and receipt retains the principal and delegation version that justified it.

9. **Delegation is observable and reviewable.** Ackplane records request,
   grant, use, denial, expiry, revocation, and override events. Bridge exposes
   who can currently do what, which agents rely on a delegation, how often
   humans intervene, which escalations wait longest, and whether policy
   automation produces safe outcomes. These metrics measure whether the system
   is earning broader delegation rather than assuming it has.

## Consequences

- Human oversight becomes a scalable control loop rather than a final manual
  sign-off. A person sees exceptions and high-impact decisions, while routine
  work moves under a clear, bounded authorization basis.
- Ackplane needs delegation, human-decision, revocation, and audit domains,
  along with principal-scoped Bridge views and command handlers. The runtime
  must re-check live delegation before consequential actions.
- Every automation decision can be explained in one chain: human/policy
  delegation, selected context, agent proposal, command, receipt, evidence,
  and outcome.
- Organizations can start with narrow delegations and expand them based on
  measured outcomes, instead of jumping directly from human-only work to
  unrestricted autonomous execution.

## Rejected alternatives

**Require human approval for every agent operation.** Rejected because it
turns a fleet into a human serial bottleneck and leaves no room for routine,
auditable automation.

**Give an agent a permanent broad role after enrollment.** Rejected because
enrollment proves node identity and control capability, not perpetual business
authority or an unrestricted risk appetite.

**Let knowledge confidence, a model score, or an agent self-assessment count
as approval.** Rejected because none names an accountable principal or a
reviewed operating boundary.

**Rely on a global emergency stop as the human control model.** Rejected
because it offers only "everything runs" or "everything stops." Scoped
delegation, escalation, pause, drain, and receipted intervention give humans
meaningful control before an emergency.
