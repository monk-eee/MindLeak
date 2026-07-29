# ADR-0065: Completion belongs at the publication boundary

- Status: Accepted
- Date: 2026-07-29
- Related: [ADR-0009](0009-evidence-backed-conformance.md) (evidence-backed
  conformance),
  [ADR-0045](0045-armed-means-finished.md) (armed means finished),
  [ADR-0046](0046-a-question-nobody-is-asked-is-not-a-question.md) (a question
  nobody is asked),
  [ADR-0048](0048-a-lapsed-lease-holes-the-window-it-does-not-move-it.md) (a
  lapsed lease holes the window),
  [ADR-0049](0049-the-ledger-is-not-optional-at-the-publication-boundary.md)
  (the ledger is not optional at the publication boundary),
  [ADR-0052](0052-a-lease-is-a-heartbeat-not-a-deadline.md) (a lease is a
  heartbeat)

## Context

Twenty-nine claims on this repository cannot be closed by anyone but a human,
and forty conformance audits reported evidence that did not exist. Both trace
to one structural fact:

**The system asks for an action at a moment when the agent has no reason to
act.** `complete_task` must be called after the work is finished, the pull
request is open, and attention has already moved to the next thing. Nothing
forces it and nothing reminds anyone, so it is skipped — and the cost of
skipping it is invisible until it is unrecoverable.

Unrecoverable is the important word. A lapsed claim can never be certified
afterwards, because closing one means re-claiming it, re-claiming records the
lapse, and conformance refuses to certify across a holed window (ADR-0048).
That refusal is correct — narrowing the window around the hole is exactly the
laundering the rule exists to stop — but it means the debt can never be paid
off. Measured while attempting it: a task showing `0 lapse(s)` reported
`the lease lapsed 1 time(s), leaving 85730s unleased` the moment it was claimed
in order to close it.

The pattern is not new here, and neither is the answer.

**Everything in this project that relies on remembering has failed.** ADR-0046
measured adoption of a capability requiring its own call at zero, across the
whole intent plane, and solved it by delivering questions on calls agents
already make. ADR-0052 found leases lapsing mid-work and solved it by treating
six existing calls as proof of life rather than adding an obligation to poll.

**Everything hung off an action already being taken has held.** `canonical-push`
refuses to publish without a live claim (ADR-0049) and compliance is effectively
total, because publishing is not optional. Arming a pull request *is* joining
the delivery queue (ADR-0045, ADR-0062), so there is no queue to remember.

Completion is the last obligation still waiting to be remembered.

## Decision

**`canonical-push` offers completion, because publishing is the moment when
everything completion needs is true at once.**

1. **The boundary is already crossed, and already gated.** `canonical-push` runs
   on every publication and already resolves the agent, the claim and the
   branch. Nothing new has to be remembered, installed, or polled.

2. **At that moment the claim is alive.** The publisher already refuses without
   a live claim, so the window is continuous by construction — the ADR-0048
   hole cannot exist yet. This is the last instant at which that is guaranteed.

3. **At that moment the evidence exists.** The commits are made, and with the
   post-commit hook they are already ingested at their own timestamps. The
   bundle can be assembled rather than reconstructed, which is the difference
   between evidence and archaeology.

4. **It offers; it does not close.** The agent still submits. ADR-0058 decision
   5 is explicit that nothing closes automatically, and an auto-closing
   publisher would record completions nobody attested — the failure ADR-0009
   exists to prevent. Offering means: assemble the bundle, run the check, and
   report what completing would say, so submitting is one call rather than a
   research task.

5. **Declining is free and silent.** Work that is not finished publishes
   normally. A publisher that nagged would be routed around, and a guard people
   route around is worse than no guard.

## Consequences

The unrecoverable case stops being reachable by accident. An agent that
publishes has, at that moment, everything needed to complete; whether it does
is its choice, but it can no longer *lose* the ability by walking away.

Publishing acquires a second meaning, and that is the real cost of this
decision. `canonical-push` is currently a gate — it says no. This makes it also
a prompt, and the two can be confused: a publisher that offers completion looks,
to a careless reader, like a publisher that performs it. Decision 4 is what
keeps them apart, and it must not erode.

It does not repair the twenty-nine. They predate the hook and the offer, their
windows are already holed, and no mechanism in this ADR reaches backwards.
`make stranded-report` names the likely commit for each so a human can confirm
them; that remains the only route, by design.

It is not sufficient alone. An agent that never publishes still never completes.
That is a narrower gap than the one it replaces — work nobody publishes is work
nobody is waiting on — but it is not nothing, and this decision should not be
read as closing the question.

## Alternatives considered

**Make `complete_task` easier to call.** This is what the last three fixes did:
refuse a misspelt argument, return the claim window, refuse an empty bundle.
Each removed a real defect and none changed the outcome, because the problem is
not that completion is hard — it is that nothing prompts it. Measured after all
three landed: sixteen fresh empty bundles.

**Close automatically on merge.** Tempting, and it would work. It also records
completions nobody attested, which is precisely the failure ADR-0009 exists to
prevent. The board would look clean and mean less than it does now, which is a
worse outcome than an untidy board.

**Do nothing and rely on discipline.** This is the current state, and this
repository has measured its result twice: zero adoption in ADR-0046, and
twenty-nine unrecoverable claims here.
