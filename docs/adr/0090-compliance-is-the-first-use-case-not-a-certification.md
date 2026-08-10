# ADR-0090: Compliance is the first use case, not a certification

- Status: Accepted
- Date: 2026-08-10
- Deciders: MindLeak maintainers
- Accepted: 2026-08-10 by the repository owner — attributed human adoption after
  review.
- Depends on: [ADR-0089](0089-mindleak-is-an-operating-system-for-agent-coordination.md)
  (product category)
- Related: [ADR-0026](0026-constitutional-policy-over-mechanistic-ratchets.md)
  (constitutional authority), [ADR-0034](0034-typed-controls-and-enforcement-ceilings.md)
  (controls and ceilings), [ADR-0039](0039-waivers-end-amendments-change.md)
  (waivers expire), [ADR-0009](0009-evidence-backed-conformance.md) (evidence-backed
  verdicts), [ADR-0031](0031-exportable-conformance-evidence.md) (portable proof),
  [ADR-0028](0028-external-adoption-evidence-gate.md) (claim tiers),
  [ADR-0084](0084-backplane-evidence-has-explicit-trust.md) (provenance)

## Context

ADR-0089 sets the category. A category still needs a first buyer, and the
productivity story is the harder one to sell honestly: its strongest evidence is
a controlled-efficacy result on a pinned scenario, and ADR-0028 forbids inflating
that into a general claim.

Compliance and security assurance is different, because the thing those teams
need is the thing this system already produces as a by-product. They do not
primarily ask whether work was fast. They ask which rule applied, who did the
work, what evidence exists, whether an exception was approved, when it expires,
and whether anyone can prove it after the fact. Those map onto primitives that
already exist and are already tested.

The demand is also becoming structural. As more code is written by agents, the
reviewer's question shifts from "is this change good" to "what authorised this
change, and what proves it happened the way it was authorised". That question has
no good answer in a PR description, a green check, or an agent's own summary.

The hazard is equally clear. A compliance interface is a machine for producing
confident-looking green ticks. The moment a screen says a repository is
"compliant", the product has made an assertion about a regulatory framework that
it cannot support, that no clause in the customer's constitution actually says,
and that an auditor is entitled to rely on. That is a materially worse failure
than a wrong latency number.

## Decision

1. **Compliance and assurance operations is the first named commercial use case
   and the first Mission Control surface.** Developer productivity remains the
   underlying mechanism and a legitimate benefit; it is not the leading claim
   while its evidence tier is narrower.

2. **The mapping to existing primitives is explicit and adds no parallel model.**

   | Compliance concept | MindLeak primitive |
   |---|---|
   | Policy | Constitutional clause with rationale, scope, and consequence |
   | Requirement | Objective, constraint, or invariant |
   | Automated check | Typed control producing an observation (ADR-0034) |
   | Approved exception | Scoped, attributed, expiring waiver (ADR-0039) |
   | Work ownership | Claim and lease within an evidence window |
   | Proof of work | Bounded, attributed evidence bundle (ADR-0009) |
   | Verification | Conformance verdict against governing clauses |
   | Audit record | Append-only receipts and task event log (ADR-0064) |
   | Portable attestation | Exported evidence and conformance manifest (ADR-0031) |

3. **MindLeak reports assurance evidence; it does not certify compliance.** No
   screen, export, API field, or marketing statement asserts that a repository,
   tenant, or organisation is compliant with SOC 2, ISO 27001, PCI DSS, DORA, or
   any other framework. A framework mapping is a customer-authored constitution
   or policy pack; the product reports conformance to *those adopted clauses* and
   names the clause every time.

4. **No green result exists without a resolvable chain.** Every positive
   indicator resolves to clause, applicable control observations, evidence, and
   verdict, per ADR-0026. An aggregate percentage may appear as a workload
   summary and never as a headline compliance claim, because an average hides
   which specific obligation is unmet.

5. **An ungoverned repository is never rendered as compliant.** With no adopted
   constitution the answer is `needs_human` with `constitution_absent`. Absence
   of policy, absence of evidence, and absence of findings are three distinct
   states, and none of them is assurance.

6. **A control pass is not a verdict, and a waiver is not a pass.** Control
   observations display as observations. A waived obligation displays as an
   active, attributed exception with its expiry and remediation, never as a
   satisfied requirement, and it is visibly counted as an exception in every
   rollup.

7. **Freshness and coverage are first-class, not footnotes.** Stale, missing,
   uncovered, and unverified are distinct states shown beside any result,
   carrying the ledger position and time behind them. A quiet dashboard means
   nothing was reported, and it must say so rather than appearing calm.

8. **The first screen is the assurance queue, not an executive score.** The
   product opens on work that needs a decision — violations, human reviews,
   expiring waivers, uncoordinated work, and overlaps — because that is the
   surface a compliance owner can act on, and because a score invites the
   overclaim decision 3 forbids.

9. **Evidence minimisation is not relaxed for compliance value.** Auditor demand
   is not a reason to upload source, terminal output, or prompts. ADR-0084
   decision 10 governs payloads; broader collection requires its own reviewed
   contract with a retention policy.

10. **Outcome claims follow the evidence tiers.** Statements about audit effort,
    reviewer time, or finding rates require a reviewed pilot with real
    compliance or security reviewers under ADR-0028, published with scope and
    limitations. Until then the product describes the mechanism it provides, not
    the savings it produces.

## Consequences

- The first commercial surface is a projection and interface layer over
  primitives that already exist and are already tested, rather than a new
  subsystem with its own semantics.
- "Trust the agent" is replaced by "trust the evidence", which is a claim we can
  substantiate for a specific task, window, and clause on demand.
- Refusing the word "compliant" costs a comfortable sales line and buys the
  ability to survive scrutiny from the exact audience being sold to.
- Compliance customers exert pull towards framework templates. Those arrive as
  policy packs under ADR-0026 and ADR-0043, reviewed clause by clause, and never
  as automatically activated law.
- Showing stale and uncovered states will sometimes make a repository look worse
  than a competing dashboard that silently omits them. That difference is the
  product.
- A compliance buyer may request enforcement that the cooperative model does not
  provide. ADR-0089 clause 3 is the honest answer, and publication gating is the
  strongest available control point.

## Rejected alternatives

**Lead with developer productivity.** Rejected as the primary commercial claim
because its strongest evidence is a narrow controlled scenario, and ADR-0028
prevents generalising it. It remains the reason the evidence exists at all.

**Ship a compliance score or grade.** Rejected because a single number is the
fastest route to an unsupported assertion, obscures which obligation failed, and
invites optimising the proxy — the exact failure ADR-0026 was written against.

**Ship built-in SOC 2 and ISO control mappings as product defaults.** Rejected
because it would place framework interpretation and its liability inside the
tool, and would activate policy no customer reviewed. Packs proposed for explicit
adoption are the supported path.

**Let the product declare a repository compliant when all clauses pass.** Rejected
because clause coverage is defined by the customer's own constitution; passing
everything asked proves only that, and stating more would misrepresent scope.

**Build a separate compliance data model.** Rejected because a parallel model
would drift from the conformance semantics that produce the evidence, and two
answers to "did this conform" is worse than none.
