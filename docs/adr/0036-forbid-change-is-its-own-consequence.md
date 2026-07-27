# ADR-0036: A `forbid_change` lock is its own consequence declaration

- Status: Accepted
- Date: 2026-07-27
- Deciders: MindLeak maintainers
- Refines: [ADR-0034](0034-typed-controls-and-enforcement-ceilings.md) (typed
  controls and enforcement ceilings)
- Related: [ADR-0009](0009-evidence-backed-conformance.md) (evidence-backed
  conformance), [SPEC-CONSTITUTION](../SPEC-CONSTITUTION.md) §10

## Context

ADR-0034 routes every control observation through its clause's declared
consequence, bounded by the control's enforcement power. Migrating
`forbid_change` into that machinery — the first deterministic code control
required by SPEC-CONSTITUTION §12.1 task 4 — exposed a conflict.

`forbid_change` currently produces `violation` unconditionally: any change to a
locked node is a breach. Routed through the ceiling it would instead resolve
against the clause's declared consequence, and a clause that has not completed
its enforcement contract declares none. SPEC-CONSTITUTION §10 says such a clause
is review-only.

So a literal reading silently weakens every existing lock from `violation` to
`needs_human` the moment the migration lands. Most clauses in a real project
have not completed an enforcement contract, so this is the common case rather
than an edge.

## Decision

**A `forbid_change` binding is itself a consequence declaration of `block`.**

1. When resolving a `forbid_change` breach, the declared consequence is `block`,
   taken from the binding mode rather than from the clause's `consequence`
   field. The clause's own consequence governs its *other* controls unchanged.
2. Its power is `mechanical`. Within the Intent Plane's own authority the check
   genuinely refuses a state transition: a `violation` verdict moves the task to
   `blocked` rather than `done`. It prevents, it does not merely observe.
3. The ADR-0034 ceiling still applies, and `min(block, block)` is `block`, so
   existing behaviour is preserved exactly.
4. An incomplete clause remains review-only for every other purpose. This is a
   narrow exception for one explicit, human-placed lock, not a general escape
   from §10.

## Consequences

- Migrating `forbid_change` into a typed control is behaviour-preserving. No
  existing lock weakens, and no existing test changes meaning.
- The exception is explicit and reviewable here rather than implicit in code.
  A reader who finds `forbid_change` ignoring `clause.consequence` has a record
  explaining why instead of inferring a bug.
- `forbid_change` becomes reportable through the same `ControlObservation`
  surface as every other control, so the conformance audit records it uniformly.
- It deviates from a literal §10 reading. That is accepted deliberately: §10
  exists so a clause does not *silently acquire* the power to block, whereas
  `forbid_change` is a deliberate act by a human who already chose that power.
  Applying §10 here would invert its purpose, weakening a lock precisely because
  someone was explicit about wanting one.

## Rejected alternatives

- **Resolve `forbid_change` through the clause's declared consequence.** The
  spec-literal reading. Rejected because it silently softens existing locks at
  migration time, and the softening is invisible: a project would discover it
  when a breach it expected to block came back as `needs_human`.
- **Require every clause carrying a lock to complete an enforcement contract
  first.** Principled, but it makes the migration a breaking change for every
  existing project and offers no benefit — the lock already states the intent
  that the contract would restate.
- **Keep `forbid_change` outside the control machinery.** Leaves the first and
  most important code control unmigrated, so the audit cannot record it
  uniformly and task 4's acceptance is unmet.
