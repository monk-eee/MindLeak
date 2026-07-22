# Evaluation

MindLeak's claims advance only when a repeatable scenario crosses its declared
gate. This document records measured behavior; it does not infer agent benefit
from implementation completeness.

## Current baseline

Captured on 2026-07-22 against server version `0.1.0` before graph-lifecycle or
ADR-0006 work. Baseline revision: `7ca97a7` (`feat: initial commit - MindLeak
TCGE and Lodestar intent plane`).

| Scenario | Expected | Observed | Result |
|---|---|---|---|
| Stale structure retraction | A removed symbol is absent after re-ingestion | Removed symbol remains queryable | Fail |
| Cross-file impact | A file importing the changed file appears in its impact radius | Importing file is absent | Fail |

Machine-readable result:
[graph-correctness.json](../benchmarks/baseline/graph-correctness.json).

## First improvement

ADR-0007 introduced artifact-owned structural snapshots and transactional
reconciliation. Running the same harness produced this controlled delta:

| Scenario | Baseline | After ADR-0007 | Delta |
|---|---|---|---|
| Stale structure retraction | Fail | Pass | Removed symbols and call edges are retracted |
| Cross-file impact | Fail | Fail | Unchanged; remains gated on ADR-0006 |

Machine-readable result:
[2026-07-22-structural-reconciliation.json](../benchmarks/results/2026-07-22-structural-reconciliation.json).

This is a correctness improvement, not evidence of agent productivity. The
baseline remains immutable so later results cannot erase the pre-change state.

## Reproduce

From the repository root:

```bash
node scripts/evaluate-graph.mjs --allow-failures
```

The harness builds the server, clears inherited agent attribution, creates a
fresh temporary SQLite database, drives the binary over newline-delimited
MCP/stdio, and emits the source revision plus executable SHA-256 before removing
the database. It has no network or model dependency and runs unchanged on
Windows, macOS, and Linux. Omit `--allow-failures` to use it as a gate: any red
scenario returns a nonzero exit code.

## Interpretation

These are expected baseline failures, not flaky tests:

- file ingestion currently reinforces newly observed structure but does not
  retract facts absent from the latest file snapshot;
- source extraction currently emits in-file symbols and calls but no cross-file
  import relation.

The stale-structure scenario is the Phase 1 correctness gate. The cross-file
scenario is the first ADR-0006 capability gate. Each result must turn green
without weakening its expected value.

## Validation limitation

Unit Test MCP 1.3.6 currently reports zero executed tests for both an explicitly
discovered Vitest file and successful Cargo custom runs, and it emits no Rust
coverage data. It does surface Cargo compile/test failures. The exact limitation
is tracked in [DEVELOPERS.md](../DEVELOPERS.md#known-gaps). Until result
accounting is repaired, this black-box harness plus compile/lint gates provide
additional executable evidence, while CI remains the unit-test authority.

Agent-outcome comparisons (`none`, `flat-history`, `mindleak`, and
`mindleak+lodestar`) have not started. No productivity or task-success claim is
supported by this baseline.