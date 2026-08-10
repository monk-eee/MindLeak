# ADR-0090: Certification is a status, not a service

- Status: Accepted
- Date: 2026-08-10
- Deciders: MindLeak maintainers
- Accepted: 2026-08-10 by the repository owner — attributed human adoption after
  review.
- Amends: the original text accepted under this number, which refused
  certification outright. That refusal was an over-correction and is replaced
  here rather than superseded, by explicit owner direction while the decision
  was unmerged and had no other readers.
- Depends on: [ADR-0089](0089-mindleak-is-an-operating-system-for-agent-coordination.md)
  (product category and capability vocabulary)
- Related: [ADR-0026](0026-constitutional-policy-over-mechanistic-ratchets.md)
  (constitutional authority), [ADR-0034](0034-typed-controls-and-enforcement-ceilings.md)
  (controls and ceilings), [ADR-0039](0039-waivers-end-amendments-change.md)
  (waivers expire), [ADR-0009](0009-evidence-backed-conformance.md)
  (evidence-backed verdicts), [ADR-0031](0031-exportable-conformance-evidence.md)
  (portable proof), [ADR-0028](0028-external-adoption-evidence-gate.md)
  (claim tiers)

## Context

Compliance and security assurance is the first commercial use case, and the
thing those buyers actually ask for is certification. The original text of this
ADR refused it outright. That was wrong, and it was wrong in a specific way: it
collapsed two different claims into one refusal.

| Claim | Can we make it? |
|---|---|
| This change conforms to the policy this project adopted | **Yes.** The project adopted the policy, we hold the evidence, and the loop closes. |
| This organisation is compliant with SOC 2, ISO 27001, DORA | **No.** An accredited auditor decides that, against controls we do not define and scope we cannot see. |

Refusing the second is correct. Refusing the first discards the product. It is
also the more valuable of the two: a framework badge is annual and
point-in-time, whereas conformance to an adopted constitution is per-change and
continuous, and it produces exactly the artefact an auditor asks for. We feed
the audit rather than competing with it.

A second problem was structural. Naming a component *Certifier* puts the claim
in the component name, where it cannot be qualified. It also fails the naming
test in ADR-0089 clause 5: "Certifier Certifications" is not a dashboard widget,
while "Librarian Evidence" and "Beacon Conflicts" are. The awkwardness was the
tell that the noun belonged to the output rather than to the component.

## Decision

1. **Verification is the capability; certification is the status.** *Verifier*
   evaluates evidence against governing clauses. What it produces is a status a
   subject holds. No component is named for the claim it emits.

2. **A certification status is always qualified, never a bare badge.** Every
   status carries its subject and commit, the policy version it was judged
   against, the evidence bundle behind it, the date, and the verdict:

   ```text
   Repository Status: Certified
   Certification Date: 2026-08-10
   Evidence Bundle:    LIB-4821
   Policy Version:     GP-7
   ```

   The policy version is what makes the status self-limiting. A reader cannot
   mistake "certified against GP-7" for a framework verdict, and the honesty is
   structural rather than a disclaimer somebody can crop out.

3. **MindLeak never asserts external framework compliance.** A framework mapping
   is customer-authored policy adopted through the existing constitutional path.
   A status against those clauses certifies conformance to *them*, and the
   product says so in those words.

4. **Every status resolves to the ADR-0026 chain.** Clause, applicable control
   observations, evidence, verdict. A status that cannot be resolved to that
   chain is not issued.

5. **The states other than certified are distinct and legible.** *Not certified*
   with its reason, *waived* with its expiry and remediation, *needs human*,
   *uncertifiable* where no constitution has been adopted, and *stale* where the
   subject moved past its evidence. None of these render as certified, and a
   quiet result is never mistaken for a clean one.

6. **Certification is bound to a commit and expires when the subject moves.** A
   new commit is uncertified until verified. Staleness is displayed, never
   assumed away, so a green status can always be traced to the revision it
   actually describes.

7. **A status covers the clauses it names and nothing more.** There is no
   "fully compliant", and no aggregate percentage stands in for a specific
   obligation. Coverage — which clauses were evaluated and which were not — is
   shown beside the status.

8. **The MVP is deliberately small.** Verifier ships as the deterministic
   conformance verdict that already exists, plus the qualified status line.
   Durable bundle identifiers, exportable certificate documents, per-clause
   breakdowns, and organisation-wide rollups are later work. The status model is
   what has to be right first, because everything above it inherits its honesty.

9. **Outcome claims still follow the evidence tiers.** Statements about audit
   effort or reviewer time need a reviewed pilot under ADR-0028. The mechanism
   may be described now; the savings may not.

## Consequences

- The product can say the word buyers need to hear, in a form that survives
  scrutiny from the reviewer being sold to.
- "Trust the agent" becomes "trust the evidence", and the status line is the
  shortest honest expression of that.
- The status references the Librarian bundle and the Gatekeeper policy version,
  so the capability vocabulary shows up in the artefact rather than only in
  documentation.
- Showing stale and uncertified states will sometimes make a repository look
  worse than a dashboard that quietly omits them. That difference is the
  product.
- Framework templates arrive as policy packs reviewed clause by clause under
  ADR-0026 and ADR-0043, never as activated defaults.
- Verification is a projection over primitives that already exist and are
  already tested, not a new subsystem with its own semantics.

## Rejected alternatives

**Name a component Certifier.** Rejected because it puts an unqualifiable claim
in a component name, and because it fails the widget test that the rest of the
vocabulary passes.

**Refuse certification entirely.** The original text of this ADR. Rejected as an
over-correction: it protected against a claim we cannot make by discarding one
we can, and it left the strongest commercial capability unnamed.

**Ship a bare "Compliant" badge.** Rejected because an unscoped badge is read as
a framework verdict no matter what the surrounding documentation says.

**Ship a compliance score or grade.** Rejected because one number hides which
obligation failed and invites optimising the proxy, which is the failure
ADR-0026 exists to prevent.

**Ship built-in SOC 2 and ISO mappings as defaults.** Rejected because it places
framework interpretation, and its liability, inside the tool, and activates
policy no customer reviewed.

**Let a status assert compliance once every clause passes.** Rejected because
clause coverage is defined by the customer's own constitution; passing
everything asked proves exactly that, and claiming more misrepresents scope.
