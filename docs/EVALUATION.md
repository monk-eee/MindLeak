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

## Memory-arm context precision

The end-to-end task-success comparison named above has not started. What *has*
started is a narrower, deterministic benchmark of **context precision**: before
an agent can act, does its memory surface the right context at all? This is a
proxy for agent benefit, not a productivity claim, and it runs with no model
dependency.

Two experiments run under `make bench`.

**Impact precision vs lexical similarity.** On an adversarial JS/TS fixture where
distractors share the changed file's vocabulary but do not import it, the graph
answers "what breaks if I change X?" exactly while similarity ranks the
vocabulary-sharing distractors:

| Method | Precision | Recall | F1 |
|---|---|---|---|
| MindLeak (graph impact) | 100% | 100% | 1.00 |
| Similarity (TF-IDF top-k) | 25% | 25% | 0.25 |

An optional live `nomic-embed-text` arm runs when a local `/v1/embeddings` server
is reachable and is skipped otherwise, so the core run stays deterministic. The
machine-readable result is printed to stdout.

**Four memory arms across three task shapes.** One workspace (structural imports
+ a failing execution + an architectural decision) is queried three ways; each
arm returns at most `K=5` context items, scored precision@K / recall / F1 against
deterministic ground truth:

| Memory arm | impact | debug | rationale | mean F1 |
|---|---|---|---|---|
| none | 0.00 | 0.00 | 0.00 | 0.00 |
| flat (recency) | 0.25 | 0.00 | 0.00 | 0.08 |
| vector (TF-IDF / embeddings) | 0.00 | 0.57 | 0.75 | 0.44 |
| mindleak (decay graph) | 1.00 | 0.57 | 0.75 | 0.77 |

The graph matches similarity on semantic recall (debug, rationale) and dominates
on the structural question (impact) that recency and pure similarity cannot
answer. The vector arm auto-upgrades from TF-IDF to the `recall` embedding index
when a local model is available. Machine-readable result:
[2026-07-22-agent-outcome-context-precision.json](../benchmarks/results/2026-07-22-agent-outcome-context-precision.json).

### Reproduce

```bash
make bench
```

This is a retrieval-quality proxy on small, engineered fixtures. It is not a
multi-language precision/recall benchmark, and the end-to-end task-success
comparison (with an agent in the loop, and eventually `mindleak+lodestar`)
remains future work.
