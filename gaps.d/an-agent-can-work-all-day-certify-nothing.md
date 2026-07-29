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
