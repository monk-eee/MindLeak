- **An agent can work all day, certify nothing, and only discover it at
  `complete_task` — MEASURED, OPEN.** Evidence-backed conformance (ADR-0009)
  reads what the *Memory Plane* holds for the calling agent. An agent that never
  calls MindLeak's ingestion tools accumulates nothing, so `evidence_for`
  returns an empty bundle, `check_conformance` answers `needs_human` with
  `evidence contains no provenance-bearing mutation`, and `complete_task`
  refuses. Measured on this repository: one session held 15 claims and landed
  roughly a dozen merged pull requests in a day, and `evidence_for` over the
  same 24 hours returned `executions=0; successful=0; commits=0; changed=0;
  failed=0` — while other agents in the same graph held 358, 38, 23, 16 and 7
  observations. The graph was healthy (4901 nodes, 9445 active edges); the work
  simply left no trace attributable to the agent that did it. — Impact: the
  conformance model silently degrades to "a human must look" for exactly the
  agents it exists to hold to account, and the degradation is invisible until
  the work is finished, at which point the task needs a `resolve_task` from a
  human who is not the author. The guard is behaving correctly on absent
  evidence; the gap is that nothing says so at claim time, when it could still
  be acted on. `stalled_work` reports the resulting lapsed claims accurately, so
  the symptom is visible while the cause is not. — Not fixed this run. The cheap
  repair is for `claim_task` to report the claiming agent's current observation
  count, so an agent with none is told before it starts rather than after it
  finishes; naming the ingestion tools in the `complete_task` refusal would help
  the reader who is already stuck. Whether ingestion should instead be automatic
  for shell-driven agents is a design question, not a bug fix — the VS Code
  sensors attribute to the editor's own session, which is a different identity
  from the one holding the claim.
  Reproduced from a second angle, which narrows where the repair belongs:
  `canonical-push` already refuses to publish without a live claim (ADR-0048),
  so the one path in this repository that *does* enforce the ledger runs on
  every publication — and it still writes nothing to the Memory Plane. A task
  claimed, validated (613 tests and clippy clean), and published through that
  gate answered `needs_human` minutes later. So this is not only agents that
  forget to ingest; the instrumented path does not close the loop either. That
  makes `canonical-push` the cheapest place to ingest the commit it just
  pushed, which would make evidence a by-product of publishing rather than a
  separate discipline nobody remembers. Deliberately *not* worked around by
  hand-ingesting after the verdict: ingesting in order to satisfy a gate that
  has just reported no evidence produces a receipt that proves nothing, and a
  green conformance chain that means less than the refusal it replaced.
  — *A second cause with the same symptom and none of the same remedies,
  measured 30 Jul 2026.* Everything above assumes the work left a trace
  somewhere and the trace failed to reach the graph, so every remedy is some
  form of "ingest it". A **ledger-only act** produces the identical refusal for
  the opposite reason: it mutated nothing. `design_register`, `attribute`,
  `supersede`, `grant_waiver` and `resolve` all land entirely in the Lodestar
  ledger, so there is no commit, no execution and no changed node — the bundle
  is empty because the work genuinely changed no file, and no amount of
  ingestion can populate it. `task:680b14565a8f` registered ADR-0073 in the
  design ledger; check 369 answered `needs_human` with exactly
  `evidence contains no provenance-bearing mutation`, and the task rests in
  `in_review` awaiting a human `resolve`. Under the current model that is the
  only terminus available to it: a ledger-only task can never certify, however
  correctly it is claimed.
  — *Why this needs saying separately.* An agent that hits this refusal and
  reads only the paragraphs above will conclude it forgot to ingest, and go
  looking for an ingestion step that would not have helped. The two causes are
  distinguishable at the point of failure — absent ingestion means git has a
  commit the graph lacks, a ledger act means git has nothing to find.
  — *Do not manufacture a commit to clear it.* Touching a file so the bundle
  has something in it launders a ledger act as a code change, and defeats the
  same guard this entry already argues is correct. Submit the empty bundle,
  take the `needs_human`, and let a person resolve it. Not fixed this run, and
  deliberately not fixed here: admitting a first-class ledger-act evidence kind
  is a design question for an ADR, not a change to smuggle in beside a gap
  entry. Worth deciding only if these become common — today the honest routing
  to human review is also the substantively right answer, since ADR-0023 keeps
  an agent from accepting its own design in any case.
