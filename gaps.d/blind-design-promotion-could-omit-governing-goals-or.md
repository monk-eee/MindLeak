- **Blind design promotion could omit governing goals or duplicate existing work
  — FIXED.** — ADR-0024
  was correctly implemented across Lodestar, MindLeak, the extension, evaluation,
  and docs under promoted `task:46dd49254e4c`, but that task belongs only to
  `goal:local-temporal-context-graph`; exact commit evidence produced conformance
  audit `65` with `drift` for the independently governed Intent Plane and
  principled-delivery surfaces. The ADR-0018 audit confirmed the same shape:
  promoted `task:d2900fdfa41b` belongs to the graph goal while its required git
  safety scripts are governed by `goal:principled-verified-delivery`, so exact
  evidence for green commit `321cf17` produced audit `68` with `drift`. ADR-0028
  exposed the second failure mode: deterministic fallback created unblocked
  `task:735e36892ffa` even though release-gated pilot `task:7f5ae1198134` already
  represented the exact work under the Intent Plane objective. — High
  coordination impact: a design could look materialized while bypassing its real
  delivery chain. — Fixed Jul 2026 (`task:53a02c15fa67`): planning is read-only;
  humans review explicit create/link/no-work plans; create may span objectives;
  link reuses authoritative tasks; materialization is atomic/idempotent; repairs
  append attributed revisions and replace only the current projection. The bad
  ADR-0028 task was durably abandoned rather than deleted or relinked by hand.
