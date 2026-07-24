# ADR-0031: Exportable conformance evidence, the Evidence Board, and a CI conformance gate

- Status: Proposed
- Date: 2026-07-24
- Deciders: MindLeak maintainers
- Related: [ADR-0009](0009-evidence-backed-conformance.md) (evidence-backed
  conformance), [ADR-0010](0010-observability-and-resilience.md) (observability),
  [ADR-0004](0004-intent-plane-spec-brain.md) (intent plane),
  [ADR-0030](0030-discrete-per-agent-identity.md) (per-agent identity),
  [SPEC-INTENT.md](../SPEC-INTENT.md)

## Context

The evidence-backed conformance loop ([ADR-0009](0009-evidence-backed-conformance.md))
is the single most valuable guarantee in the system: **an agent cannot mark work
"done" by asserting it.** `complete_task` refuses self-reported narration and
consumes only a bounded, provenance-bearing evidence bundle (`evidence_for`,
MindLeak) that a separate `check_conformance` scores against the goal's code
bindings. Aligned completes; drift/uncertainty park for a human; violation blocks.
Every check writes a durable, token-sealed record to an append-only
`conformance_history`.

This was verified live on 2026-07-24 against `task:01c9d1675d3b`
(`goal:adr-0030-unique-per-process-agent-identity`, bound to both MCP server
`main.rs` files). The loop enforced, in order:

1. **No evidence, no completion.** `evidence_for` was empty until real executions
   and commits were attributed to the acting agent.
2. **Identity.** A `claim_task` for a different agent than the configured identity
   was rejected.
3. **The claim window, twice.** Evidence starting before the claim, and evidence
   ending in the future, both failed *"falls outside the live claim."*
4. **Cross-agent contamination.** A first check returned **`drift`** —
   *"governed code changed without a covering task"* — because a *second* agent's
   commit bled into the window (the [ADR-0030](0030-discrete-per-agent-identity.md)
   aliasing bug, caught live). The loop would not rubber-stamp it.
5. **Completion only on clean, aligned, attributed proof** — with a resolvable
   receipt (`token: 35a244ab…`) traceable to `execution → modified → main.rs`.

The loop works. What is missing is that **the proof cannot leave the SQLite
ledger.** `conformance_history` is reachable only through a live MCP tool call
against `.lodestar/spec.db` — a gitignored, regenerable, local file. In a full
agentic workflow the evidence is *the only artifact that proves the fleet did the
sanctioned thing* — every other signal (the agent's summary, a green check, a PR
body) is narration an agent can fabricate. If that proof cannot be exported,
reviewed, gated on, or audited, its power is trapped.

## Decision

Make conformance evidence a **portable, verifiable, first-class artifact**, and
surface it where humans and CI actually look. Three parts, one design.

### 1. `export_evidence` — the portable proof (core)

A new facade method + MCP tool, mirroring the existing `export_constitution`
([`facade/constitution.rs`](../../crates/lodestar-core/src/facade/constitution.rs)):

- `export_evidence(scope, path?) -> String` renders the conformance record chain
  for a scope — a single `task_id`, a `goal_id` (all its tasks), or the whole
  ledger — as a **committed-friendly artifact**: Markdown for human/PR review plus
  a companion JSON with the machine-checkable fields.
- Each entry carries the **resolvable, tamper-evident anchor**: the check `token`,
  `verdict`, `findings`, `checked_at`, the acting `agent_id`, the claim window,
  and the evidence summary (executions, successful, commits, changed nodes) with a
  content hash over the recorded evidence JSON. A reviewer can replay
  `token → evidence → executions/commits` without trusting any agent's word.
- Deterministic and model-free; it only reads the durable audit trail.
- Written to a path (e.g. `.lodestar/evidence/<goal>.md`) it becomes a normal,
  reviewable, diffable file — the same move that made the constitution reviewable.

### 2. The Evidence Board (VS Code extension)

A fourth tree view beside the Context Graph, Intent Board, and Design Board,
distinct from the executive board:

- Lists completed and in-review tasks with their **latest verdict** (aligned /
  drift / needs-human / violation), the acting agent, and the claim window.
- Expanding a row shows its `conformance_history` chain — every check, its verdict
  and findings, and the resolvable token (the drift→aligned story from the live
  run is exactly what a reviewer needs to see).
- An **Export** action calls `export_evidence` to write the reviewable artifact.
- Pure board/threading logic lives in
  [`editors/vscode/src/util.ts`](../../editors/vscode/src/util.ts) with vitest;
  vscode-coupled code stays thin (the established extension pattern).

### 3. The CI conformance gate

The exported artifact is what lets CI enforce the loop, because `.lodestar/spec.db`
is gitignored and absent in CI:

- A portable Node runner (`scripts/conformance-gate.mjs`, cross-platform per the
  toolchain rule) reads the committed evidence artifact and the PR's changed
  files, and **fails the build when a changed, governed code node has no `aligned`
  conformance receipt** covering it. Documentation nodes are exempt exactly as at
  conformance read time (`is_documentation_node`).
- Wired as a job in the existing CI workflow; advisory-first (report) before it is
  made blocking, so adoption is a ratchet, not a cliff.

### Why exportable proof is load-bearing (README / ARCHITECTURE)

[README.md](../../README.md) and [ARCHITECTURE.md](ARCHITECTURE.md) gain a short,
prominent explanation that this is the point of the intent plane: in an agentic
fleet the **conformance evidence chain is the proof-of-work** — provenance-anchored
to real executions and commits, bounded by the claim, token-sealed, and now
exportable for review, CI, and audit. This is the counterweight to decay: episodes
fade, but the durable record of what conformed survives and can leave the machine.

## Consequences

- "Done" becomes a portable, verifiable claim: a PR can ship its own proof-of-
  conformance; CI can block merges that changed governed code without an aligned
  receipt; an auditor can replay tokens without trusting an agent.
- The Evidence Board makes the loop *visible* — today its power is invisible unless
  you call the tools by hand, which is why its importance is easy to miss.
- No new completion semantics: `export_evidence` only renders the existing durable
  records; the gate only reads them. The gate is advisory before blocking.
- Distinct per-agent identity ([ADR-0030](0030-discrete-per-agent-identity.md)) is
  a prerequisite: without it, contamination like the live `drift` above pollutes
  every actor's bundle.

### Rejected alternatives

- **Leave evidence tool-only.** Trapped proof cannot gate a PR, brief a reviewer,
  or satisfy an audit — the very workflows that make the loop worth having.
- **Trust the agent's own "done" summary.** That is the narration the whole loop
  exists to replace; it is unverifiable by construction.
- **A model on the export/gate path.** Non-deterministic and unnecessary — export
  and gating read the durable record; no judgement is re-litigated.
- **Regenerate `.lodestar/spec.db` in CI.** The DB is local, gitignored, and would
  require replaying the whole session; the committed artifact is the portable
  contract instead.

This ADR carries no behavioural code; it is the design-first predecessor for the
implementation task. Implementation touches
`crates/lodestar-core/src/facade/conformance.rs` (+ `tools/conformance.rs`) for
`export_evidence`, `editors/vscode/src/{evidenceBoardViewProvider.ts,util.ts,
extension.ts}` and `package.json` for the board, `scripts/conformance-gate.mjs`
plus the CI workflow for the gate, and the README / ARCHITECTURE evidence
narrative. Implementation was deferred from the ADR commit because the conformance
sources and those docs were mid-rewrite in a divergent shared tree; it lands once
that settles.
