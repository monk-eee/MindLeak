# ADR-0092: Ackplane is governed by its own goal

- Status: Accepted
- Date: 2026-08-13
- Deciders: monk-eee
- Accepted: 2026-08-13 by monk-eee (repository owner) — attributed human
  adoption under ADR-0043.
- Adopted: 2026-08-13 as `amendment:a334b7f2c123`, which promoted
  `constitution:v4` and created `goal:ackplane-federation-service@constitution:v4`.
  All twelve Rust files of `ackplane-core`, `ackplane-protocol` and
  `ackplane-server` are bound to it as `governed`. Approved by monk-eee and
  executed by an agent, which is the separation of parties `amend_constitution`
  requires. `advise` over those crates now returns a governing clause where it
  previously returned an empty set, so decision 6 has been discharged.
- Depends on: [ADR-0082](0082-ackplane-is-a-standalone-federation-service.md)
  (Ackplane is a separately deployable service)
- Related: [ADR-0026](0026-constitutional-policy-over-mechanistic-ratchets.md)
  (constitutional authority), [ADR-0043](0043-adoption-into-active-constitution-is-an-amendment.md)
  (attributed amendment flow), [ADR-0009](0009-evidence-backed-conformance.md)
  (evidence-backed verdicts), [ADR-0041](0041-cross-cutting-work-is-declared.md)
  (declared coverage), [ADR-0086](0086-postgresql-is-the-ackplane-ledger-and-arbiter.md)
  (the ledger and arbiter)

## Context

No clause in the adopted constitution governs any Ackplane code. Measured on
2026-08-13, `advise` over `crates/ackplane-core/src/lib.rs`,
`crates/ackplane-protocol/src/lib.rs`, `crates/ackplane-server/src/lib.rs` and
`crates/ackplane-server/src/main.rs` returns a single finding — *no active clause
governs this change; proceed* — with an empty governing set.

The tasks are not ungoverned in the same way. They were produced by a coarse
decomposition under whichever goal was to hand, so the Ackplane work sits under
`goal:local-temporal-context-graph`, which is the *graph engine's* objective, and
under the Intent Plane's. The result is visible in
`governing_for_task` for the ledger task, which reports its governing clauses as
bound to `Cargo.toml`, `crates/mindleak-model/src/lib.rs`, and
`crates/lodestar-core/src/store/test_support.rs`. None of those is Ackplane code.
The task is graded against three files it will never touch.

So every Ackplane change produces the same verdict. The server crate was
published with `needs_human` and both of its new files reported `UNBOUND`, and
neither outcome said anything about the change: the first measured the
mislabelling, the second recorded that nothing exists to bind to. That a file
cannot be bound by any agent-reachable verb is separately filed in
`gaps.d/a-publication-can-report-an-unbound-file-no-agent-can-bind.md`.

The cost is not the individual verdict. It is that a signal which always reads
the same way stops being read. ADR-0026 rejects mechanistic ratchets because a
check people learn to route around governs nothing; a conformance verdict that
returns `needs_human` for an entire subsystem regardless of the work is that same
failure arriving from the other direction. The correct response to it is a
taxonomy fix, and the tempting one — narrowing evidence until the grade improves
— destroys exactly the signal the grade exists to carry.

## Decision

1. **Ackplane has one objective goal of its own.** Proposed statement: *provide
   Ackplane, a separately deployable federation service that arbitrates
   coordination for enrolled repositories across an organisation boundary,
   holding the durable ledger, receipts, enrolments, and projections that
   repository nodes publish to — while never becoming a mode of either local
   plane, and never silently recomputing or overwriting the receipts they
   export.*

2. **The Ackplane crates bind to it.** `ackplane-core` (the repository side of
   the boundary), `ackplane-protocol` (the wire contract), and `ackplane-server`
   (the service) are governed by that goal and by no plane goal.

3. **It does not inherit the planes' goals, and they do not inherit it.**
   ADR-0082 clause 1 makes Ackplane a separate deployable rather than a mode of
   `mindleak-mcp` or `lodestar-mcp`. A shared goal would re-couple in the
   constitution what that decision separates in the code, and the first symptom
   would be a plane change drifting because it touched a federation clause.

4. **Existing Ackplane tasks keep the goal they recorded.** They are not
   re-pointed. A task's goal is what its verdicts were measured against, and
   rewriting it after the fact would silently restate what past conformance
   records meant. New Ackplane work is created under the new goal instead, and
   the older tasks are completed, resolved, or retired as they stand.

5. **Adoption is an attributed act under ADR-0043.** The amendment names both
   the agent that executed it and the person who approved it, and the binding of
   the three crates happens with it.

   This decision originally added that the Intent Plane exposed no
   agent-reachable verb defining a goal or binding code to one, and concluded
   that "an agent cannot adopt this even if it wanted to". That was false, and
   it is corrected here rather than quietly dropped, because it was believed and
   acted upon: it authorised a task to build a constitution API that already
   shipped. ADR-0059 narrows `tools/list` to a default profile and deliberately
   does **not** narrow dispatch, so `constitution_define`,
   `link_goal_to_artifact` and the whole amendment lifecycle —
   `propose_amendment`, `draft_clause`, `amend_constitution`, `amendments` —
   were callable by name the entire time. An empty tool list is evidence about
   the advertisement, never about the capability. Recorded in
   [`gaps.d/the-constitution-verbs-were-reachable-all-along.md`](../../gaps.d/the-constitution-verbs-were-reachable-all-along.md).

6. **An Ackplane `needs_human` verdict meant ungoverned — until this was
   adopted.** While Ackplane had no goal of its own, that verdict was read as
   "no clause covers this" rather than "this change is suspect", and it was not
   to be worked around by narrowing the evidence bundle, declaring unrelated
   coverage under ADR-0041, or suppressing the binding-coverage report. The
   adoption recorded in the header ended that condition: a `needs_human` verdict
   on Ackplane work now says something about the change again, which is the
   outcome this decision existed to produce.

## Consequences

- Ackplane work can reach `aligned` on its merits, and a `needs_human` verdict
  there recovers its meaning: it starts indicating something about the change.
- The binding is performed by an attributed amendment. Adopting this decision
  made the amendment verbs load-bearing rather than latent, and that pressure
  produced the fix immediately: `amend_constitution` now requires an
  `approved_by` distinct from the calling agent, so the adoption above names who
  approved it as well as who ran it.
- The goal is an objective, so it decomposes into claimable work. Constraints
  specific to Ackplane — that authority is never dual-written, that a durability
  claim names its failure domain — remain candidates for separate clauses, and
  are deliberately not bundled here.
- One more goal is one more thing to keep honest. The failure this fixes was
  created by a generator reaching for the nearest goal; a taxonomy with more
  entries gives it more nearby wrong answers, so the accompanying discipline is
  that generated work states the goal it serves rather than inheriting one.

## Rejected alternatives

**Keep using `goal:local-temporal-context-graph`.** The status quo. It makes the
graph engine's objective govern a federation service, which is how conformance
came to grade the ledger task against `test_support.rs`. The statement of that
goal is about turning telemetry into a decay-weighted graph; no honest reading of
it covers a PostgreSQL arbiter.

**Bind the Ackplane crates to the Intent Plane's goal.** Closer, since both
concern coordination, but that goal ends "local, stdio-only, no network
listener", which is the precise opposite of what Ackplane is. Adopting it would
put a clause in force that the subsystem must violate to exist.

**Leave Ackplane ungoverned and accept the verdict.** What is in force today. It
is cheap, and it is what trains agents to treat `needs_human` as noise — the
outcome ADR-0026 exists to prevent.

**Suppress the `UNBOUND` report for new Ackplane files.** Rejected because the
report is correct. Silencing a true statement to make a pipeline quiet is the
same move as narrowing evidence to improve a grade, and it would remove the only
surface currently telling anyone this is wrong.
