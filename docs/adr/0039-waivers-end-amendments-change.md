# ADR-0039: Every waiver ends; changing the rule is an amendment

- Status: Accepted
- Date: 2026-07-27
- Deciders: MindLeak maintainers
- Refines: [ADR-0026](0026-constitutional-policy-over-mechanistic-ratchets.md)
  (constitutional policy over mechanistic ratchets)
- Related: [ADR-0025](0025-authoritative-checked-conformance.md) (authoritative
  checked conformance), [ADR-0034](0034-typed-controls-and-enforcement-ceilings.md)
  (typed controls and enforcement ceilings),
  [SPEC-CONSTITUTION](../SPEC-CONSTITUTION.md) §9

## Context

Every enforcement system acquires exceptions. The question is never whether they
exist, only whether they are visible. Today an agent that cannot satisfy a rule
has `--no-verify`, a commented-out check, a skipped test, or a quietly widened
scope. Those are exceptions too — unattributed, unbounded, and invisible. The
constitution gains nothing by pretending otherwise; it loses, because each silent
bypass is a rule that reads as enforced while enforcing nothing.

SPEC-CONSTITUTION §9 names the shape of the replacement — a waiver carrying
scope, reason, approver, expiry, and remediation linkage — and states that a
permanent exception is an amendment. It does not say what makes that boundary
hold. Left unspecified, three failure modes are available, and all three are the
comfortable option in the moment:

1. **The unbounded waiver.** An exception with no expiry is indistinguishable
   from a policy change nobody reviewed. Granted often enough, the waiver table
   becomes the real constitution while the written one describes a project that
   no longer exists.
2. **The silent waiver.** If a waived breach produces the same conformance record
   as a change that never touched a governed node, the waiver is exactly the
   hidden bypass it was meant to replace — now with ceremony.
3. **The self-approved waiver.** A clause reserving authority to a human is
   worthless if the agent that needs relief can grant it to itself.

Amendments carry a fourth: if changing policy is expensive or unreviewable,
nobody amends, and waivers absorb the pressure instead.

## Decision

**A waiver bends a rule once, briefly, for a named reason. An amendment changes
the rule. Each is made cheap enough to use and expensive enough to notice.**

### Waivers

1. **`expires_at` is required and must be in the future.** There is no
   open-ended waiver. An exception that never ends is the policy, and changing
   policy is an amendment.
2. **Expiry is a query bound, not a status transition.** A lapsed waiver keeps
   `status: active` and simply stops matching. Nothing runs, so nothing can fail
   to run; and history reads as it was judged rather than being rewritten by the
   passage of time. Revocation is the opposite case — explicit, attributed,
   immediate for future checks, and recorded rather than deleted, because the
   exception happened.
3. **A waived breach still appears in the findings**, naming the waiver, its
   approver, and its expiry. The verdict changes; the visibility does not.
4. **`approved_by` is the calling session, and the clause's declared authority is
   enforced.** An agent cannot approve an exception to a rule reserved to a
   person. A clause declaring itself unwaivable refuses exceptions outright,
   otherwise `waivable: false` is decorative.
5. **Waivability and authority are part of the clause's enforcement contract, and
   default to `false`.** A clause refuses exceptions by omission rather than
   granting them by omission — the same default-deny posture ADR-0034 applies to
   an incomplete enforcement contract.
6. **Waiver state is part of the conformance token**, including `expires_at`. A
   check made while an exception was in force is not evidence about a world where
   it was revoked. Recording the expiry means a token also stops matching once a
   waiver lapses, which no row rewrite would otherwise signal (ADR-0025).

### Amendments

7. **An amendment draft starts as a copy of the active constitution.** An empty
   draft would make every amendment a re-adoption of the whole document, and the
   diff would report every untouched rule as removed and re-added — burying the
   one line that changed. Clauses match across versions on `slug`.
8. **`amend_constitution` is a different call from `activate_constitution`.**
   Adopting a first constitution and changing an adopted one are different acts;
   only the second retires rules people are currently working under, so only it
   demands a rationale and produces a reviewable diff.
9. **The outgoing version and its clauses are superseded, never deleted**, so a
   prior conformance record keeps naming the version it was judged under. A
   verdict that silently re-reads under rules that did not exist when it was
   given is not a record of anything.
10. **The diff compares the enforcement contract, not just the statement.** A
    clause whose consequence moves `review` → `block`, or whose scope widens,
    governs differently even when every word is identical. That is precisely the
    quiet amendment a statement-only diff would report as nothing at all.
11. **An amendment that changes nothing is refused**, because a no-op version
    bump would retire and re-issue every clause identically, invalidating live
    conformance tokens for no reason.
12. **A pack upgrade is a proposal, never an upgrade.** Upstream can never alter
    active local policy, so planning one is a pure read that produces the
    argument for amending. It compares against the recorded provenance — the
    exact pack clause each local clause was adopted from — rather than against
    the local clause, so a tailored clause does not read as an upstream change.
    Clauses that *were* tailored are flagged, because accepting an upstream
    change to one is the single way a pack upgrade can silently discard a
    deliberate local decision.

## Consequences

**Good.** An exception is now cheaper than a bypass and more honest than a
silence: it takes one call, and it produces a record naming who allowed it and
until when. Enforcement returns by itself, with nothing scheduled to forget.
Because waivers are countable per clause, the system can surface its own
strongest amendment signal — a rule waived repeatedly is a rule that wants
changing, and that argument is now made from data rather than from memory.

**Costs.** Every exception carries an expiry someone must eventually revisit, and
a project that leans on waivers will feel that recurrence as friction. That is
the intended pressure, but it is real. Amendments are heavier than editing a
clause in place: a draft, a rationale, and a diff. Superseded versions and lapsed
waivers accumulate rather than being cleaned up, which is the price of history
that can be trusted.

**Risks.** A clause can still be waived indefinitely by renewing, which the data
makes visible but does not prevent — deliberately, since forbidding renewal would
just push the pressure back to silent bypasses. And a project may amend to
weaken a rule rather than meet it; the diff makes that legible to a reviewer, but
legibility is not prevention.

**Not decided here.** Who may amend, how long a typical waiver should run, and
whether repeated waivers should automatically raise an amendment proposal remain
project judgements. Bounded renewal limits and automatic amendment proposals from
waiver frequency are both plausible later refinements.
