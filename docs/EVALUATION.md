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

## Structural impact unlock

ADR-0006 phase 1 added deterministic JavaScript/TypeScript imports, package
nodes, and named cross-file call resolution. The strict fixture now requires all
three structural outcomes:

| Scenario | After ADR-0007 | After ADR-0006 phase 1 |
|---|---|---|
| Stale structure retraction | Pass | Pass |
| Importing artifact discovered | Fail | Pass |
| Typed `imports` edge present | Fail | Pass |
| Named cross-file `calls` edge present | Fail | Pass |
| Co-imported sibling excluded | Not measured | Pass |
| Comment/member-call false edge excluded | Not measured | Pass |
| Mixed/index/explicit consumer-first stub promoted | Not measured | Pass |
| Scoped `require` shadowing respected | Not measured | Pass |

Machine-readable result:
[2026-07-22-js-ts-import-impact.json](../benchmarks/results/2026-07-22-js-ts-import-impact.json).

This proves the supported JS/TS fixture only. It is not yet a multi-language
precision/recall benchmark and does not satisfy the broader product threshold.

## Type hierarchy proof

ADR-0006 phase 2 adds deterministic simple named JS/TS `extends` and
`implements` edges. The fixture covers same-file and named-import targets,
consumer-first promotion, generic-constraint exclusion, unsupported mixin
expressions, reverse-direction exclusion, and retraction on re-ingest.

| Metric | Gate | Observed | Result |
|---|---:|---:|---|
| Hierarchy relation precision | >= 0.95 | 1.00 (5/5) | Pass |
| Hierarchy relation recall | >= 0.90 | 1.00 (5/5) | Pass |
| Derived-type impact precision | >= 0.80 | 1.00 (2/2) | Pass |
| Derived-type impact recall | >= 0.85 | 1.00 (2/2) | Pass |
| Parent reached from changed child | Must be absent | Absent | Pass |
| Removed hierarchy survives re-ingest | Must be absent | Absent | Pass |

This is a reviewed deterministic fixture, not a claim of complete TypeScript or
multi-language parsing. Default/namespace heritage and expression-based mixins
remain outside the supported truth set.

## Manifest dependency proof

ADR-0006 phase 3 adds deterministic artifact-to-package `depends_on` edges. The
fixture covers Cargo renamed and target dependencies, npm direct/dev/peer/
optional sections, Go single/block requirements, canonical PEP 508 names,
incoming impact, retraction, and fail-closed malformed manifests. Workspace
catalogs, npm overrides, Go replacements, and requirement directives are
negative controls rather than dependencies.

| Metric | Gate | Observed | Result |
|---|---:|---:|---|
| Manifest relation precision | >= 0.95 | 1.00 (4/4) | Pass |
| Manifest relation recall | >= 0.90 | 1.00 (4/4) | Pass |
| Package reaches dependent manifest | Required | Present | Pass |
| Manifest reaches package in impact direction | Must be absent | Absent | Pass |
| Removed dependency survives re-ingest | Must be absent | Absent | Pass |
| Catalog/override-only package emitted | Must be absent | Absent | Pass |

This proves direct dependencies for the four supported manifest families. It
does not infer transitive dependencies or claim lockfile/resolver coverage.

## Passive sensor proof

ADR-0011 raises the extension floor to VS Code 1.93 and adds shell-execution,
workspace-mutation, and built-in Git commit sensors. Component fixtures fire
mocked VS Code terminal/Git events and assert that the sensor itself invokes
`ingest_execution`/`ingest_commit`; output redaction, secret-command suppression,
path exclusion/capping, exit-code handling, and visible degradation are covered
without an agent-authored ingestion call.

The initial before/after Git status design failed its gate: one subprocess
snapshot measured 71.7 ms p95, before the second snapshot or ingestion. It was
replaced by one in-process workspace watcher. A second bottleneck was then found
in per-fact SQLite writes; batching each execution into one transaction moved the
full 200-file/8 KiB processing + MCP + SQLite path below the target.

| Metric | Gate | Observed | Result |
|---|---:|---:|---|
| End-to-end local capture p50 | Report | 22.352 ms | Pass |
| End-to-end local capture p95 | < 50 ms | 28.651 ms | Pass |
| End-to-end local capture max | Report | 30.096 ms | Pass |
| Terminal event fixture invokes ingestion | Required | Yes | Pass |
| Git commit fixture invokes ingestion | Required | Yes | Pass |

Machine-readable result:
[2026-07-22-passive-sensor-overhead.json](../benchmarks/results/2026-07-22-passive-sensor-overhead.json).
Reproduce after building the extension and MCP server with
`node scripts/evaluate-sensors.mjs` (also included in `make bench`). The timing
fixture is local and deterministic; actual shell integration remains dependent
on the user's shell and is reported as degraded when absent.

## Signal-weighted decay proof

ADR-0012 completes ADR-0005 with one derived signal path used by traversal,
impact, snapshots, counts, agent activity, and prune. The adversarial benchmark
constructs real graph evidence and compares 400 same-session green-build
reinforcements with one failure corroborated by structure, a related commit and
decision, and a later successful execution of the same command. An unrelated
green command is a negative control and earns no consequence term.

| Scenario | Observed | Result |
|---|---:|---|
| Same-session spam multiplier | 1.000x | Pass |
| Same-session spam after six days | 0.015625, inactive | Pass |
| Resolved failure multiplier | 7.245x | Pass |
| Resolved failure after six days | 0.563233, active | Pass |
| Resolved failure after sixty days | 0.003213, inactive | Pass |
| Expired failure reaches handoff and remains queued | Present/retained | Pass |
| Maximum multiplier | 8.000x | Pass |
| 200-edge snapshot p95 | 16.757 ms | Pass |

The ablation isolates each multiplier: baseline 1.000, span reinforcement 1.448,
source diversity 2.500, consequence 3.500, surprise 1.750, structural centrality
2.000, and deliberate attention 2.250. Consequence and independent sources
therefore outweigh repetition as required.

Machine-readable result:
[2026-07-22-signal-weighted-decay.json](../benchmarks/results/2026-07-22-signal-weighted-decay.json).
Reproduce with `node scripts/evaluate-signal.mjs` or `make bench`.

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

The original failures were expected baseline behavior, not flaky tests:

- file ingestion currently reinforces newly observed structure but does not
  retract facts absent from the latest file snapshot;
- source extraction currently emits in-file symbols and calls but no cross-file
  import relation.

Both original gates are now green without weakening their expected values.
Imports, hierarchy, and direct manifest fixtures are green; broader language and
real-repository truth sets remain required for the product-wide impact claim.

## Validation limitation

Unit Test MCP 1.3.6 currently reports zero executed tests for both an explicitly
discovered Vitest file and successful Cargo custom runs, and it emits no Rust
coverage data. It does surface Cargo compile/test failures. The exact limitation
is tracked in [DEVELOPERS.md](../DEVELOPERS.md#known-gaps). Until result
accounting is repaired, this black-box harness plus compile/lint gates provide
additional executable evidence, while CI remains the unit-test authority.

## Real agent-loop outcome

The product decision gate uses GitHub Copilot CLI 1.0.63 with pinned
`claude-haiku-4.5` on one composite task: resume an interrupted typed-session
regression, avoid a recorded failed string-conversion approach, preserve a
governing invariant, fix hidden/public behavior, and identify all impacted
production files. Four randomized arms run three times each in fresh workspaces
and databases. Each run uses an isolated Copilot home containing authentication
state only; personal skills, MCP configuration, memory, sessions, built-in
GitHub MCP, and network tools are absent/disabled.

| Arm | Success | Regression rate | Median exploration calls | Median output tokens | Median duration |
|---|---:|---:|---:|---:|---:|
| none | 0.0% | 100.0% | 11 | 3,502 | 72.060 s |
| flat history | 0.0% | 100.0% | 11 | 3,034 | 61.273 s |
| MindLeak | 66.7% | 33.3% | 9 | 2,284 | 53.370 s |
| MindLeak + Lodestar | 100.0% | 0.0% | 10 | 2,275 | 50.877 s |

Against the no-memory control, the best MindLeak arm reduces median exploration
by 18.2%, crossing the 15% primary threshold. MindLeak improves success by 66.7
percentage points; MindLeak+Lodestar improves it by 100 points, with no
correctness regression. Impacted-file F1 is 1.00 in every arm after the deliverable
was made explicit, so success differences come from hidden invariant/boundary
behavior rather than reporting ambiguity.

Machine-readable result:
[2026-07-22-agent-loop-outcome.json](../benchmarks/results/2026-07-22-agent-loop-outcome.json).
Reproduce with `make agent-bench`; this consumes premium model requests. The
artifact records source, fixture, runner/model, executable hashes, randomized
schedule, per-run tool names, tokens, duration, hidden checks, and aggregate
variance without storing prompts or model reasoning.

This passes the go/no-go threshold for productization, not universal efficacy.
It is one engineered composite scenario with three repetitions per arm and a
single model/runner. Cross-file repair, impact, resume, failed-approach, and
invariant behaviors are represented; broader repositories, models, and the
two-agent duplicate-work scenario remain required before general claims.

## Memory-arm context precision

Separate from the completed agent-loop outcome above, this deterministic
benchmark asks a narrower question: before an agent can act, does its memory
surface the right context at all? It is a retrieval proxy, not another
productivity claim, and it runs with no model dependency.

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
multi-language precision/recall benchmark. Broader replication of both this
proxy and the completed end-to-end agent outcome remains future work.
