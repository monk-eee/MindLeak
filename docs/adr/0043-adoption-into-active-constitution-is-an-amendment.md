# ADR-0043: Adopting a pack clause into an active constitution is an amendment

- Status: Proposed
- Date: 2026-07-27
- Deciders: MindLeak maintainers
- Refines: [ADR-0026](0026-constitutional-policy-over-mechanistic-ratchets.md)
  (constitutional policy over mechanistic ratchets),
  [ADR-0039](0039-waivers-end-amendments-change.md) (every waiver ends; changing
  the rule is an amendment)
- Related: [ADR-0034](0034-typed-controls-and-enforcement-ceilings.md)
  (enforcement ceilings), [SPEC-CONSTITUTION](../SPEC-CONSTITUTION.md) §7.5, §9

## Context

ADR-0039 established that changing adopted policy is an amendment: draft the next
version, complete the change there, and promote it with a rationale and an
explicit clause diff. `complete_clause_contract` enforces exactly that — it
refuses a clause on an active version, because hardening a rule mid-flight
changes what governs everyone already working under it.

Adopting a pack clause takes a different path, and nobody noticed until it was
used in anger.

`propose_policy_pack` against the active constitution creates proposals bound to
that version. `review_pack_clause(adopted)` then materialises a self-contained
local clause which **inherits the version's status** — and the version is
`active`. So the clause is governing the moment the call returns. There is no
draft, no rationale beyond the review reason, no diff, and no second step.

Turning on the `fleet-delivery` pack demonstrated the asymmetry in a single
session. Seven clauses went from not existing to active and governing, four of
them declaring `block`, in seven tool calls. Each call was attributed and each
recorded a reason, so nothing was hidden — but the constitution went from
governing nothing to governing publication, commit scope, and topology without
producing the one artifact ADR-0039 exists to produce: a diff a human reads
before it takes effect.

The asymmetry is hard to defend on the merits:

- **Hardening `core.evidence` from review to block** requires a draft, a
  rationale, and a diff.
- **Adding `fleet.protected_branch` declaring block** requires one call.

The second changes more. A new clause can govern scopes nothing governed before,
and can reach `violation` on work already in flight under the old rules.

Two considerations pull the other way, and they are why this is not simply a bug.
Bootstrap adoption — the Common Core into a *draft* constitution — is already
correct and must stay a single reviewed step, because there is no prior policy to
diff against. And a heavyweight adoption path discourages adopting anything,
which pushes projects back toward the ungoverned default that ADR-0026 exists to
move them off.

## Decision

**Adoption into an *active* constitution is an amendment; adoption into a *draft*
is not.**

1. `review_pack_clause(adopted | tailored)` against a **draft** constitution is
   unchanged. Bootstrap has nothing to diff against, and every clause is already
   held inert until `activate_constitution` promotes the whole version.
2. `review_pack_clause(adopted | tailored)` against an **active** constitution
   materialises the clause into the **open amendment draft**, not into the active
   version. If no amendment draft is open, the call is refused and names
   `propose_amendment` — the same shape as `complete_clause_contract`'s refusal.
3. `rejected` is unaffected in both cases. Recording that a clause was declined
   changes nothing about what governs, and requiring an amendment to say "no"
   would discourage the disposition we most want recorded.
4. The resulting amendment diff reports each adopted clause as `added`, carrying
   the scope, evidence contract, and consequence it arrives with — so the reader
   sees what will newly govern, and at what force, before promotion.

Adopting a pack therefore becomes: `propose_amendment`, review each clause into
the draft, then `amend_constitution` with a rationale. Three steps instead of
one, and the middle step is the review that already existed.

## Consequences

**Good.** One rule for changing what governs, instead of two paths with different
ceremony reached by an accident of which tool you happened to call. The diff
becomes complete: today it shows a clause that hardened but not a clause that
appeared, which makes it a partial answer to "what changed?" — the most dangerous
kind. And a pack adoption becomes reviewable as a unit, which is how it is
actually decided; seven clauses arriving together is one decision, not seven.

**Costs.** Adoption is heavier, and bootstrap-shaped flows that adopt into an
already-active constitution must be rewritten to open a draft first. Anything
scripting `review_pack_clause` against an active version breaks — deliberately,
because that is the behaviour being removed. The `fleet-delivery` clauses adopted
before this ADR landed are grandfathered: they are active, attributed, and their
review reasons are recorded, but there is no amendment record for them.

**Risks.** Making adoption heavier may mean fewer projects adopt. That is the
trade ADR-0026 already accepts elsewhere — policy that arrives without review is
not policy, it is configuration — but it is a real cost and worth revisiting if
adoption rates suffer.

**Not decided here.** Whether `propose_amendment` should be implicit when a
review targets an active constitution (convenient, but re-hides the step), and
whether a pack adoption should produce one amendment per pack or per clause. One
per pack matches how the decision is made and is the assumed default.
