# ADR-0034: Typed controls, workflow scope, and enforcement ceilings

- Status: Accepted
- Date: 2026-07-27
- Deciders: MindLeak maintainers
- Implements: [SPEC-CONSTITUTION](../SPEC-CONSTITUTION.md) §4, §8, §12.1 task 4
- Related: [ADR-0026](0026-constitutional-policy-over-mechanistic-ratchets.md)
  (clauses over ratchets), [ADR-0029](0029-proactive-constitutional-advice.md)
  (ask-before-act), [ADR-0032](0032-single-checkout-fleet-integration.md) (fleet
  workflow), [ADR-0015](0015-advisory-symbol-leases.md) (false safety),
  [ADR-0009](0009-evidence-backed-conformance.md) /
  [ADR-0025](0025-authoritative-checked-conformance.md) (conformance),
  [ADR-0031](0031-exportable-conformance-evidence.md) (CI gate)

## Context

ADR-0026 established that a clause carries legitimacy — rationale, scope,
evidence contract, consequence, and waiver policy — while a mechanism reports
compliance. Task 2 shipped immutable policy packs and the Common Core, so
clauses can now *declare* a consequence. Nothing yet evaluates a mechanism and
maps its result through that declaration.

Three concrete gaps follow.

**Workflow rules are unrepresentable.** Every scope resolution today runs
through code-node bindings: `resolve_governing_clauses(node_ids, task_goal_id)`
buckets `artifact:` / `symbol:` ids into forbid, in-scope, and other. A rule
such as "a protected branch advances only by reviewed merge" binds to no node,
so `advise` structurally cannot answer "may I push to main". `Goal.scope`
exists but is inert.

**ADR-0032's fleet rules have teeth but no legitimacy.** Single checkout, one
publisher, preserved commit identity, and PR-only `main` currently live in prose
plus Git hooks. A hook can refuse, but it cannot explain itself, record an
attributed exception, or expire one. The only available escape hatch is
`--no-verify`, which AGENTS.md forbids precisely because it is silent. A
justified hotfix and a careless bypass are indistinguishable afterwards.

**Nothing prevents a clause from overstating its own power.** ADR-0015 rejected
advisory symbol leases because an advisory that looks like a mutex grants false
safety. That rule was applied once, by judgement, to one feature. A clause may
today declare `block` while its only mechanism is a decay-weighted hint that
can be stale or incomplete — reintroducing false safety at the policy layer,
where it is harder to see.

## Decision

1. **Workflow scope is a first-class scope kind.** `Goal.scope` accepts
   `workflow:` tokens matched by prefix, alongside existing `artifact:` /
   `symbol:` code bindings: `workflow:git.commit`, `workflow:git.publish`,
   `workflow:review.pull_request`, and `workflow:topology`. A parent token
   matches its children.

2. **Controls are typed and versioned**, not a policy DSL: `Control { id,
   clause_id, kind, power, version, configuration, status }` with `ControlKind`
   of check, threshold, ratchet, procedure, or bounded judgment. Each emits a
   `ControlObservation { control_id, clause_id, control_version, scope, status,
   measurements, baseline, evidence_refs, evaluated_at }` with status pass,
   fail, or unknown.

3. **Every control declares the power it actually has.**

   | Power | Meaning | Ceiling |
   |---|---|---|
   | `mechanical` | Physically prevented it — hook exit, lost CAS, red CI gate | `block` |
   | `observed` | Proves it after the fact from complete deterministic data | `review` |
   | `advisory` | A hint that may be stale or partial | `review` |

4. **Effective consequence is
   `min(clause.consequence, control.power_ceiling)`.** A clause may declare
   `block`, but backed only by an advisory or observed control it resolves at
   `review`. This makes ADR-0015's false-safety rule mechanical and general
   instead of a judgement call applied per feature.

5. **Three refusals bound escalation.** An orphan control (no active clause)
   produces a finding and never escalates. A `unknown` status never escalates
   past `advise`. A control whose version does not match the version its clause
   bound is coerced to `unknown`.

6. **Fleet policy ships as the `fleet-delivery` pack, gated by review, not by a
   flag.** Shipping pack bytes is not enforcement: a pack must be proposed, and
   every clause needs an explicit adopt, tailor, or reject disposition before
   it materialises. The existing review ledger is already the opt-in, so no
   separate feature-flag mechanism is introduced. The only gate worth adding is
   whether repository bootstrap *auto-proposes* the pack, which belongs to the
   project profile in §12.1 task 3.

7. **The pack's initial clauses.**

   | Key | Kind | Scope | Declared | Control (power) | Effective |
   |---|---|---|---|---|---|
   | `fleet.protected_branch` | invariant | `review.pull_request`, `git.publish` | block | branch policy + pre-push refusal (mechanical); history inspection (observed) | block |
   | `fleet.single_publisher` | constraint | `git.publish` | block | `canonical-push.mjs` refusals (mechanical) | block |
   | `fleet.commit_identity` | invariant | `git.publish` | block | non-ancestor refusal (mechanical); patch-id scan (observed) | block / review |
   | `fleet.scoped_commit` | constraint | `git.commit` | block | scoped-commit guard (mechanical) | block |
   | `fleet.governed_receipt` | invariant | `artifact:` / `symbol:` | block | conformance gate (mechanical) | block |
   | `fleet.topology_honesty` | principle | `workflow:topology` | review | declared-vs-actual compare (observed) | review |
   | `fleet.coverage_ratchet` | constraint | `artifact:` | review | coverage vs baseline (ratchet) | review |

8. **Topology is observed, not policed.** `fleet.topology_honesty` is a
   `principle` capped at `review`. It fires only when declared and actual
   topology disagree; it does not prefer a layout. This ADR therefore does
   **not** amend ADR-0032: single checkout remains the adopted workflow, and
   worktrees remain permitted-but-unrecommended rather than becoming a
   constitutional axis.

9. **`fleet.coverage_ratchet` is capped by its own declaration, not its power.**
   Its CI mechanism is mechanical, but §4 notes a ratchet cannot determine
   whether the baseline was trustworthy, so the clause declares `review`.

10. **An undeclared commit scope observes `unknown`, not `fail`.** Refusing a
    claim without declared scope would be stricter but slower, and the fast
    shared-tree path is the one this repository optimises. Tightening later is
    an amendment under `core.evolution`, which is the transition path §7.5
    already describes — not a silent change.

11. **Conformance records constitutional provenance.** Each check persists the
    constitution version and, per applied clause, the clause id, control id,
    control version, observation status, effective consequence, and any waivers
    applied. The ADR-0025 token folds these in, so a policy or waiver change
    invalidates a stale checked-conformance token.

## Consequences

- `advise` can answer procedural questions before work starts — "may I push to
  this branch", "may I publish from here" — closing the ADR-0029 gap where only
  code-node changes could be advised on.
- A justified exception becomes an attributed, expiring waiver with a
  remediation owner instead of `--no-verify`. Bypass becomes visible rather
  than silent.
- A clause can no longer overstate its power. The cost is that some clauses
  resolve weaker than their author intended until a mechanical control exists,
  which is the honest reading.
- `fleet.protected_branch` now has a genuine server-side control. `main`
  requires a pull request and the five CI checks, and refuses force pushes and
  branch deletion, so the clause's declared `block` is backed mechanically
  rather than by a local `pre-push` hook alone. The paired observed control
  still detects a protected branch that advanced by anything other than a
  reviewed merge, which remains the backstop.
- **That control is bounded by `enforce_admins`.** It is currently disabled, and
  the fleet's agents authenticate as a repository admin, so an admin token can
  still push directly to `main`. Against the actor this clause exists to
  constrain, the control is therefore `observed`, not `mechanical`, and the
  ceiling rule caps its effective consequence at `review` until either
  `enforce_admins` is enabled or the bounded-waiver path (task 5) gives an
  attributed, expiring alternative to bypass. Recording this rather than
  claiming enforcement the repository does not have.
- Conformance gains a second resolution path. Code-node resolution is unchanged,
  limiting regression risk to the new workflow bucket.

## Rejected alternatives

- **A generic policy DSL.** Rejected by §4 and ADR-0009's deliberately narrow
  surface. Typed adapters keep the evaluable set small and auditable.
- **Topology as a constitutional axis.** Worktrees are an isolation mechanism,
  not a coordination one; they solve file collision while hiding the concurrent
  activity the memory plane exists to observe, and the one workflow they
  legitimately unblocked — validating around another agent's broken tree — was
  already solved branchlessly by the ADR-0032 temporary-index snapshot.
- **Letting an advisory control block.** This is exactly the ADR-0015
  false-safety trap relocated to the policy layer.
- **A separate environment-variable feature flag for the pack.** Redundant: the
  adopt/tailor/reject ledger already gates enforcement, and a second gate would
  create two sources of truth about whether a clause is active.

## Enforcement and test plan

Platform-agnostic (`cargo` / `npm` / `git` / `node` only):

1. **Ceiling.** A clause declaring `block` with only an advisory control
   resolves `review`; the same clause with a mechanical control resolves
   `block`.
2. **Refusals.** An orphan control cannot escalate; `unknown` cannot exceed
   `advise`; a version-mismatched control coerces to `unknown`.
3. **Workflow resolution.** A clause scoped `workflow:git.publish` is returned
   by `advise` for a publish intent and is *not* returned for an unrelated
   code-node change; prefix matching resolves `workflow:git` against
   `workflow:git.publish`.
4. **End-to-end workflow control.** A direct push to a protected branch
   produces a `violation`; the same change through a reviewed merge produces
   `aligned`; a valid waiver downgrades the former and appears in the audit
   record.
5. **Provenance.** The audit and token record constitution version, clause,
   control id and version, and applied waivers; mutating any of them invalidates
   a previously issued token.
6. **Unchanged path.** Existing code-node conformance and `forbid_change`
   behaviour regress cleanly.
