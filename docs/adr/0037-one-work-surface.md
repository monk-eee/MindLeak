# ADR-0037: One Work surface with advanced proof

- Status: Accepted
- Date: 2026-07-27
- Deciders: MindLeak maintainers
- Implements: [ADR-0027](0027-extension-led-progressive-disclosure.md)
  (extension-led product shell)
- Revises: [ADR-0031](0031-exportable-conformance-evidence.md) (default placement
  of the Evidence Board only; evidence, export, and CI semantics are unchanged)
- Related: [ADR-0009](0009-evidence-backed-conformance.md) (conformance),
  [ADR-0023](0023-design-board-accept-bridge.md) (human review workflow)

## Context

MindLeak's engines have crossed the usefulness threshold: claims prevent duplicate
work, overlap checks surface collisions, and conformance catches real drift. The
extension still presents those capabilities as architecture, however. A developer
must move between the Intent Board and Evidence Board, translate internal task
states, and invoke a separate MCP tool to decide work that needs human review.

This is visible complexity without additional authority. The Evidence Board reads
the same task and conformance stores as the Intent Board; `resolve_task`,
`reopen_task`, and `conformance_history` already provide the decisions and proof.
Adding another view made the audit visible but left the operating loop split.

ADR-0027 already says the extension is a progressive-disclosure shell over the
granular engines. This decision applies that principle to daily coordination:
**delete visible complexity, not capability.**

## Decision

1. **The existing task board becomes the default `Work` surface.** It remains a
   projection of Lodestar's `board`; the extension stores no task state.
2. **Review decisions happen where review work appears.** An `in_review` row
   exposes three primary actions:
   - **Accept** calls `resolve_task(task_id, human)` and moves the task to `done`;
   - **Retry** calls `reopen_task(task_id)` and returns it to claimable work;
   - **Inspect proof** reads `conformance_history` without changing state.
3. **Internal protocol names are translated at the UI boundary.** `open` is
   `Ready`, `claimed` is `In progress`, `in_review` is `Review needed`, and
   `done` is `Verified`. MCP schemas and stored values do not change.
4. **Proof remains complete but becomes advanced history.** The Evidence Board,
   export action, tokens, claim windows, and complete audit chain remain intact.
   The board is hidden by default and can be restored through VS Code's Views
   menu; it is not deleted or replaced with extension-owned state.
5. **Primary actions have a budget.** A new default view or top-level action must
   replace an existing surface or be required to complete the main workflow.
   Diagnostics, export, telemetry, and raw proof stay available on demand.

This change adds no completion semantics. The extension orchestrates existing
typed MCP tools and immediately refreshes the authoritative board after a
decision.

## Acceptance gates

- A human can accept or retry `Review needed` work from its Work row without
  copying a task id or invoking an MCP tool manually.
- Accept preserves the original conformance verdict and proof; Retry preserves
  history and returns the task to the claimable pool.
- The default tree shows action-oriented language, while context values continue
  to use stable protocol states for command eligibility.
- The Evidence Board remains reachable and exportable when explicitly enabled.
- Controller behavior is covered by Vitest; compile, lint, and the configured
  coverage thresholds remain green.

## Consequences

- The common path becomes pick up, work, verify, decide, in one surface.
- Review is more prominent than ready work because it is already waiting on a
  human decision.
- Existing headless clients and MCP workflows are unaffected.
- Historical documentation may still name the Intent Board or Evidence Board as
  the surface that existed when its decision was made; current product docs use
  `Work`.
- `resolve_task` currently validates that the reviewer differs from the worker
  but does not durably store the reviewer identity. This is recorded as a Known
  Gap and must be fixed before claiming an attributed review audit.

## Rejected alternatives

- **Delete the Evidence Board and export tools.** This removes audit capability,
  not just visible complexity.
- **Add a fifth review dashboard.** It repeats the split workflow that caused the
  problem.
- **Teach users the internal state machine.** Stable protocol names are useful to
  agents and APIs; they do not need to dominate the human interface.
- **Duplicate resolution state in the extension.** Lodestar remains the only
  authority for task state and conformance history.
