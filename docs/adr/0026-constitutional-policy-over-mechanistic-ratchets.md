# ADR-0026 - Constitutional policy over mechanistic ratchets

- **Status:** Accepted
- **Date:** 2026-07-23
- **Accepted:** 2026-07-24 by the repository owner — attributed human adoption,
  satisfying the acceptance gate defined in this ADR.

## Context

[ADR-0004](0004-intent-plane-spec-brain.md) established Lodestar's durable,
versioned Constitution, and [ADR-0009](0009-evidence-backed-conformance.md)
established evidence-backed enforcement. The current model can express
objectives, constraints, invariants, and two code-binding modes, but it does not
yet define a complete governance hierarchy.

Quality systems commonly fill that gap with ratchets: coverage cannot decrease,
warnings cannot increase, latency cannot regress. Ratchets are reproducible and
cheap, but they are mechanisms rather than policy. They cannot explain why the
measurement matters, settle competing values, delimit legitimate exceptions, or
distinguish a trustworthy baseline from inherited debt. Treating the mechanism
as authority encourages teams to optimise the proxy and hides philosophy inside
tool configuration.

Adoption creates a second problem. Most repositories begin with no explicit
constitution. They have scattered evidence of intent in documentation, CI,
tests, ADRs, and conventions, but importing a supposedly universal policy as
active law would replace project judgment with product defaults. Inferring law
from repeated behaviour would also collapse MindLeak's descriptive facts into
Lodestar's normative authority.

## Decision

Adopt the governance model in
[SPEC-CONSTITUTION.md](../SPEC-CONSTITUTION.md):

1. **The Constitution is the normative authority.** It contains an interpretive
   preamble, principles, objectives, constraints, and invariants. Enforceable
   clauses declare rationale, scope, evidence, consequence, and waiver policy.
2. **Controls are subordinate evidence mechanisms.** Tests, scanners, procedural
   checks, thresholds, and ratchets reference an active clause. Their output is
   a control observation; Conformance applies the clause to produce a verdict. A
   control with no active governing clause cannot hard-block work.
3. **Facts cannot silently become obligations.** MindLeak telemetry and learned
   knowledge may propose amendments, never activate them. Project authority,
   not frequency of behaviour or an optional model, legitimises policy.
4. **An absent constitution fails open to deliberation, not false compliance.**
   MindLeak remains usable, deterministic discovery may prepare a draft, and a
   requested policy verdict returns `needs_human` with `constitution_absent`.
   The engine neither reports constitutional alignment nor violation before
   adoption.
5. **Onboarding is proposal, review, then activation.** Bootstrap inventories
   cited repository facts, combines them with explicitly selected policy packs,
   and creates a draft. A maintainer adopts, tailors, or rejects each common
   proposal before one attributed activation transaction.
6. **The Common Core is opt-in and small.** Lodestar proposes five principles:
   evidence, preservation of intent, safety, proportionality, and explicit
   evolution. They begin as broad principles, not hard invariants.
7. **Extension packs compose without live inheritance.** Adopting a pack
   materialises local clauses with pack id, version, digest, and disposition.
   Upstream updates produce amendment proposals; they never mutate active local
   policy. Conflicts require human resolution rather than hidden precedence.
8. **Exceptions are explicit constitutional objects.** Waivers are scoped,
   attributed, expiring, auditable, and connected to remediation. Permanent
   exceptions require an amendment.

The first implementation extends the existing goal and conformance model with
typed data and controls. It does not introduce a generic policy DSL, preserving
ADR-0009's narrow deterministic surface.

Implementation is gated on attributed human acceptance of this ADR. On
acceptance, materialise the six ordered tasks in
[SPEC-CONSTITUTION.md §12.1](../SPEC-CONSTITUTION.md#121-actionable-backlog-after-acceptance)
under the existing durable Intent Plane objective. Do not create claimable
implementation tasks while this decision remains `Proposed`.

## Philosophical basis

Philosophy is an architectural input because enforcement always embeds a theory
of authority and evidence. This decision makes that theory inspectable:

- purpose precedes mechanism;
- observation describes what *is* but does not decide what *ought* to be;
- legitimacy comes from explicit, attributed adoption;
- policy may require contextual judgment while evidence remains concrete;
- durable law is amendable rather than silently mutable; and
- consequences must be proportional, explainable, and contestable.

These principles constrain implementation. Automatic template activation,
telemetry-derived law, unexplained hard failures, and non-expiring hidden
exceptions would violate the architecture even if they were operationally
convenient.

## Consequences

- Lodestar can serve mature governed projects and repositories beginning from
  no explicit policy without pretending either has the same starting state.
- Teams can share common principles and domain packs while retaining local
  sovereignty, provenance, and reviewable divergence.
- Ratchets remain useful, especially as transition controls that stop a known
  gap worsening, but their authority and exception policy become explicit.
- The model needs constitution versions, a preamble/principle representation,
  clause provenance and consequences, typed controls, draft dispositions, and
  waivers. Existing goals migrate as local clauses without invented policy.
- Conformance audits become more explainable: every hard verdict resolves to an
  active clause, evidence, applicable control observations, and waivers.
- Bootstrap can be partially automated, but activation cannot. Adoption has an
  intentional human decision cost.
- Until this proposal is implemented, the existing objective/constraint/
  invariant model and ADR-0009 verdict rules remain authoritative.

## Rejected alternatives

- **Ratchets are the governing model:** confuses a measurable proxy with the
  reason and authority for enforcing it.
- **Activate a universal default constitution automatically:** optimises initial
  convenience by removing project consent and hiding policy provenance.
- **Infer policy from existing CI or repeated behaviour:** turns descriptive
  facts, accidental habits, and legacy debt into normative law.
- **Live inheritance from centrally updated packs:** lets an upstream release
  change local enforcement without a project amendment.
- **Copy a template with no recorded dispositions:** makes rejected and
  unreviewed clauses indistinguishable from adopted ones.
- **Build a generic policy DSL first:** broadens the trusted enforcement surface
  before clause provenance, controls, and adoption are proven end to end.
