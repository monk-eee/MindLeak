# ADR-0037: A ratchet never sets its own baseline

- Status: Accepted
- Date: 2026-07-27
- Deciders: MindLeak maintainers
- Refines: [ADR-0034](0034-typed-controls-and-enforcement-ceilings.md) (typed
  controls and enforcement ceilings)
- Related: [ADR-0026](0026-constitutional-policy-over-mechanistic-ratchets.md)
  (constitutional policy over mechanistic ratchets),
  [SPEC-CONSTITUTION](../SPEC-CONSTITUTION.md) §4

## Context

SPEC-CONSTITUTION §4 requires every ratchet to reference one active clause and
to emit a `ControlObservation` rather than a verdict. ADR-0034 supplied the
resolution path. What neither settled is the question §4 raises and leaves open:
**"whether the baseline was trustworthy"** is listed among the things a ratchet
cannot determine about itself.

That question is not decorative. The common implementation of a ratchet stores
its last measurement and compares the next one against it. That design has a
quiet failure mode: a single regression that slips through — a flaky suite, an
excluded file, a partial report — is adopted as the new baseline, and every
subsequent run is judged against the degraded number. The ratchet keeps
reporting green while the standard it defends slides downward. It cannot detect
this, because from inside the mechanism a lowered baseline and a legitimate
one are indistinguishable.

A second, subtler version appears once baselines can move at all: evidence
gathered under one baseline may still be resolved under a different one. A run
that genuinely passed against 79% is not a pass against 90%, but nothing in the
observation itself says which baseline it was measured against.

## Decision

**A baseline is an attributed input to a ratchet, never an output of it.**

1. **No baseline reports `unknown`, never `pass`.** A newly registered ratchet
   has nothing to compare against. `pass` would let it certify conformance it
   never checked, which is the same "absence of evidence is not evidence of
   conformance" rule ADR-0034 applies to every other control.
2. **A ratchet cannot move its own baseline.** Accepting one is a separate,
   attributed act recording who stood behind the number.
3. **Accepting a baseline bumps the control version.** ADR-0034 already coerces
   a version-mismatched observation to `unknown`; making a baseline change a
   version change reuses that refusal, so an observation taken under the old
   baseline is treated as stale rather than re-scored against a number it never
   saw.
4. **A ratchet's power is `observed`, so a failure resolves at `review`.** It
   reads a report someone else produced and proves what already happened; it
   stopped nothing. Under the ADR-0034 ceiling that caps it below `block`, which
   matches §4: whether a particular regression is acceptable — a security repair
   that costs latency, a refactor that moves coverage between files — is a
   judgement about the change, not a comparison.
5. **The engine ships no coverage ratchet.** §4 says a ratchet cannot determine
   whether coverage is the right proxy for confidence. Shipping one as a
   built-in would answer that question, identically, for every project that
   adopts Lodestar — the one question the mechanism is explicitly not entitled
   to answer. The adapter is generic; a project registers the ratchets its own
   clauses justify.

The baseline is also read from the store on every observation and never accepted
from the caller. A caller that could supply its own baseline could choose one it
knows it beats.

## Consequences

**Good.** A ratchet cannot degrade silently: the standard moves only when
someone moves it, and the audit says who. Stale evidence is refused rather than
re-judged. A failing ratchet opens a conversation instead of refusing work,
which keeps it usable on the day a justified regression has to land.

**Costs.** Adopting a ratchet is now a two-step act — register, then accept a
baseline — and a ratchet that nobody baselines stays permanently `unknown`. That
is deliberate: an unbaselined ratchet is inert and visibly so, rather than
quietly passing. Projects that want the value automatically re-baselined on
every green run cannot have it; that is the behaviour this ADR rejects.

**Not decided here.** How a project chooses its baseline, how often it should
move, and which metrics deserve a ratchet at all remain project judgements under
the clause that authorises each one.
