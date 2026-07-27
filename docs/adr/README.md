# Architecture Decision Records

This log captures decisions that are **hard to reverse or surprising** — the
kind someone might otherwise "simplify" back into a bug. Each ADR is dated and
immutable; supersede rather than edit.

Format: [MADR](https://adr.github.io/madr/)-lite. Keep them short.

| ADR | Title | Status |
|---|---|---|
| [0001](0001-record-architecture-decisions.md) | Record architecture decisions | Accepted |
| [0002](0002-sqlite-decay-over-vector-llm.md) | SQLite + half-life decay over vector-only / per-event LLM memory | Accepted |
| [0003](0003-agent-attribution-as-observed-edges.md) | Agent attribution as decay-weighted `observed` edges | Accepted |
| [0004](0004-intent-plane-spec-brain.md) | Intent Plane: a durable "spec brain" separate from the decay graph | Accepted |
| [0005](0005-signal-weighted-decay.md) | Signal-weighted decay ("decay noise, not signal") | Accepted |
| [0006](0006-structural-dependency-edges.md) | Structural & dependency edges (graph enrichment for impact analysis) | Accepted |
| [0007](0007-structural-snapshot-reconciliation.md) | Structural snapshots replace owned facts | Accepted |
| [0008](0008-semantic-recall-embedding-index.md) | Optional semantic recall via a local embedding index (complements, never replaces, the decay graph) | Accepted |
| [0009](0009-evidence-backed-conformance.md) | Evidence-backed conformance across the memory and intent planes | Accepted |
| [0010](0010-observability-and-resilience.md) | Observability, telemetry, and network resilience | Accepted |
| [0011](0011-passive-terminal-and-git-sensors.md) | Passive terminal and Git evidence sensors | Accepted |
| [0012](0012-derived-signal-evidence.md) | Derived bounded signal evidence | Accepted |
| [0013](0013-local-data-lifecycle.md) | Local data backup, export, and reset lifecycle | Accepted |
| [0014](0014-per-project-decay-configuration.md) | Per-project decay configuration | Accepted |
| [0015](0015-advisory-symbol-leases.md) | Progressive task handoffs before advisory symbol leases | Accepted (no symbol-lease primitive) |
| [0016](0016-platform-packaging-and-registration.md) | Platform packaging and workspace registration | Accepted |
| [0017](0017-working-memory-and-autonomous-consolidation.md) | Working-memory tier and autonomous consolidation cycle | Accepted (implemented) |
| [0018](0018-conflict-safe-concurrent-editing.md) | Conflict-safe concurrent editing in a shared working tree (worktrees optional) | Superseded by [ADR-0032](0032-single-checkout-fleet-integration.md) |
| [0019](0019-task-retention-and-board-hygiene.md) | Task retention and board hygiene - hide, never delete | Accepted |
| [0020](0020-task-lifecycle-states.md) | Task lifecycle state machine — `needs_input` and `paused` | Accepted |
| [0021](0021-node-lifecycle-and-reaping.md) | Node lifecycle and maintenance reaping semantics | Accepted |
| [0022](0022-learned-knowledge-loop.md) | The learned-knowledge loop — promotion, revalidation, and advisory conformance | Accepted |
| [0023](0023-design-board-accept-bridge.md) | Design items, the Design Board, and reviewed materialization | Accepted |
| [0024](0024-preflight-overlap-detection.md) | Pre-flight work-overlap detection across both planes | Accepted |
| [0025](0025-authoritative-checked-conformance.md) | Authoritative checked conformance | Accepted |
| [0026](0026-constitutional-policy-over-mechanistic-ratchets.md) | Constitutional policy over mechanistic ratchets | Accepted |
| [0027](0027-extension-led-progressive-disclosure.md) | Extension-led progressive disclosure over MCP primitives | Accepted |
| [0028](0028-external-adoption-evidence-gate.md) | External adoption evidence before broad product claims | Accepted |
| [0029](0029-proactive-constitutional-advice.md) | Proactive constitutional advice (ask-before-act) | Accepted |
| [0030](0030-discrete-per-agent-identity.md) | Discrete per-agent identity for concurrent coordination | Accepted |
| [0031](0031-exportable-conformance-evidence.md) | Exportable conformance evidence, the Evidence Board, and a CI conformance gate | Accepted |
| [0032](0032-single-checkout-fleet-integration.md) | Single-checkout, single-publisher fleet integration | Superseded by [ADR-0038](0038-isolated-worktrees-shared-repository-state.md) |
| [0033](0033-copilot-cli-registration.md) | First-class GitHub Copilot CLI registration for both planes | Accepted |
| [0034](0034-typed-controls-and-enforcement-ceilings.md) | Typed controls, workflow scope, and enforcement ceilings | Accepted |
| [0035](0035-fleet-management-heuristics.md) | Fleet management heuristics and feedback | Accepted |
| [0036](0036-forbid-change-is-its-own-consequence.md) | A `forbid_change` lock is its own consequence declaration | Accepted |
| [0037](0037-ratchet-baselines-are-not-self-adopted.md) | A ratchet never sets its own baseline | Accepted |
| [0038](0038-isolated-worktrees-shared-repository-state.md) | Isolated worktrees, shared repository state, reviewed convergence | Accepted |
| [0039](0039-waivers-end-amendments-change.md) | Every waiver ends; changing the rule is an amendment | Accepted |
| [0040](0040-one-work-surface.md) | One Work surface with advanced proof | Accepted |
| [0041](0041-cross-cutting-work-is-declared.md) | Cross-cutting work is declared, not waived | Accepted |
| [0042](0042-designs-are-retired-by-a-person.md) | A design is retired by a person, never by a missing file | Accepted |
| [0043](0043-adoption-into-active-constitution-is-an-amendment.md) | Adopting a pack clause into an active constitution is an amendment | Accepted |
| [0044](0044-declared-context-is-durable.md) | Declared context is durable, and staleness is declared too | Accepted |
| [0045](0045-a-fleet-is-a-distributed-system.md) | An agent fleet is a distributed system, not a team | Accepted |
| [0046](0046-agents-talk-through-the-durable-thread.md) | Agents talk through the durable thread, never to each other | Accepted |
| [0047](0047-a-status-is-not-a-decision.md) | A status reflects a decision; only a decision records one | Accepted |
| [0048](0048-a-lapsed-lease-holes-the-window-it-does-not-move-it.md) | A lapsed lease holes the evidence window, it does not move it | Accepted |
| [0049](0049-publication-requires-a-claim.md) | Publication requires a claim; the ledger is not optional | Accepted |
| [0050](0050-a-superseded-decision-is-not-a-stale-one.md) | A superseded decision is not a stale one | Accepted |
| [0051](0051-a-decision-already-made-can-still-be-signed.md) | A decision already made can still be signed | Accepted |
| [0052](0052-a-lease-is-a-heartbeat-not-a-deadline.md) | A lease is a heartbeat, not a deadline | Accepted |
| [0053](0053-the-graph-records-events-not-conclusions.md) | The graph records events, not conclusions | Proposed |
| [0054](0054-identity-is-the-session-not-the-process.md) | Identity is the session, not the process that hosts it | Accepted |
| [0055](0055-draft-the-question-decide-nothing.md) | Draft the question, decide nothing | Accepted |
| [0056](0056-the-changelog-is-assembled-not-edited.md) | The changelog is assembled, not edited | Accepted |

## Writing a new ADR

1. Copy an existing file to `NNNN-short-title.md` (next number).
2. Fill in Context / Decision / Consequences.
3. Link it from the code or doc it constrains.

**Do not edit the table above by hand** — it is generated from the ADR files by
`scripts/adr-index.mjs`, and a pre-commit hook fails if it drifts. Run
`make adr-index` (or the script directly) after adding or restatusing an ADR.
Hand-maintaining it meant every branch appended a row to the same table, so
every merge conflicted on it; it also drifted, with ten of forty-five rows wrong
when the generator was introduced.

"Next number" is not obvious when several branches are in flight: your working
tree cannot see a sibling branch's unmerged ADR. A pre-commit hook
(`scripts/adr-number-guard.mjs`) checks the number against every ref and names
the first free one, so a collision costs seconds here instead of a renumber
across the file, its cross-links, and every commit message citing it. Run it
directly to pick a number before you start:

```bash
node scripts/adr-number-guard.mjs docs/adr/0042-my-decision.md
```
