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

## Recall ranking, and what it did not fix

ADR-0075 stopped `recall` ordering by raw cosine: similarity is now weighted by
node kind, and a candidate must stand out from its own query's field. It shipped
on deterministic unit tests whose fields were synthetic and uniform. A real index
is neither, so it was measured against this repository's own: **19,317 embedded
nodes**, `nomic-embed-text`, ten queries — seven the repository genuinely had to
answer, three nonsense controls. The control arm replicates the pre-change
algorithm; the treatment arm drives the built `mindleak-mcp` binary over stdio,
so what is measured is the shipped path rather than a second implementation of
it.

| Metric | Gate | Before | After | Result |
|---|---|---:|---:|---|
| Hits naming a node the graph no longer holds | Report | 24 of 50 | **0 of 49** | Pass |
| Recorded conclusions as a share of hits served | Report | 14% | **96%** | Pass |
| A nonsense query is answered with silence | Required | No | **No** | **Fail** |

The first two are the change working as intended. Nearly half of what the caller
used to be handed was an id that could no longer be opened, and conclusions were
outnumbered five to one by symbols, executions and dangling references; now the
caller is handed conclusions and nothing stale.

The third is a negative result, and it is recorded because the fixture tests
could not see it. **A nonsense query is not answered with silence on a real
index.** Distance of the top hit above its own field is 3.11–3.90 standard
deviations for the nonsense controls and 3.71–6.21 for the real questions, so
the bands **overlap by 0.19σ** and no single threshold rejects nonsense while
keeping every real answer. The shipped cut is 1σ, far below both bands, so it
trims almost nothing on a field this diverse. The synthetic field was uniform,
which made outlier detection trivial in a way a 19,000-node index is not.

The constant is deliberately **not** tuned in response. Three nonsense samples
separated from real questions by a negative margin is precisely the "global
constant" that
[the recall floor's own measurement](../gaps.d/the-recall-floor-cannot-rank-and-raising-it.md)
warns against, and tuning to it would repeat that mistake one level up. What the
result actually says is that distinctiveness-as-a-threshold is the wrong shape
for "does this query have an answer at all", and that question remains open.

Machine-readable result:
[2026-07-30-recall-ranking.json](../benchmarks/results/2026-07-30-recall-ranking.json).
Reproduce with
`node scripts/evaluate-recall.mjs --bin <mindleak-mcp> --db <graph.db>`. It needs
a populated index and a reachable embeddings server, both optional parts of the
product (ADR-0008), and reports rather than fails when either is absent.

### Grounded abstention (2026-07-31)

The negative result above ruled out another similarity threshold, not abstention itself. The follow-up gate asks whether the returned nodes support a majority of the query's IDF-weighted informative terms. It does not move the floor or sigma cut, invoke an LLM, or make another embedding request. Queries with fewer than three informative terms retain the old fuzzy behavior.

The harness now includes four coherent natural-language questions absent from this repository alongside the three gibberish controls, and it checks explicit relevance anchors for the seven real-labelled questions. This corrects a weakness in the first measurement: a non-empty list was counted as success even when its labels were unrelated.

| Metric | Before | After | Result |
|---|---:|---:|---|
| Negative controls answered with silence | 0 of 7 | **7 of 7** | Pass |
| Genuinely relevant real-query sets retained | 5 of 5 | **5 of 5** | Pass |
| Hits naming a node the graph no longer holds | 31 of 70 | **0 of 25** | Pass |
| Served hits that are recorded intents | 7 of 70 | **25 of 25** | Pass |

Two questions previously labelled real now abstain: the PowerShell query had returned generic report-script records, and the stale-server query had returned merge commits. Their non-empty result sets were not answers, so preserving them would preserve the defect. The measured binary is identified by SHA-256 in the artifact, which records clean source revision `f8d304d` rather than relying on a mutable filename or timestamp.

Machine-readable result: [2026-07-31-recall-grounding.json](../benchmarks/results/2026-07-31-recall-grounding.json).

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

## Progressive same-file handoff

ADR-0015 compares two real Lodestar stores over separate SQLite connections.
Both arms model two subtasks touching `artifact:src/lib.rs`:

| Arm | Concurrent claims | Early successor claim | Successor after aligned completion | Collision risk |
|---|---:|---:|---|---|
| Independent tasks | 2 | n/a | n/a | Present |
| `blocked_by` handoff | 1 | Rejected | Open, then claimable | Absent |

The handoff arm transactionally clears the successor dependency with the
predecessor's conformance audit and never exceeds one same-file owner. This
proves the coordination mechanism and justifies not adding an advisory symbol
lease that could be mistaken for a text lock. It does **not** prove autonomous
agents always create dependency chains; the completion evidence is synthetic but
schema-valid. Real-agent adherence remains a future scenario.

Machine-readable result:
[2026-07-23-progressive-handoff.json](../benchmarks/results/2026-07-23-progressive-handoff.json).
Reproduce with `node scripts/evaluate-handoffs.mjs`. Reproducibility is anchored
to the declared-source SHA-256 plus locked release profile and recorded
Rust/Cargo versions. The raw executable digest is labeled build-instance-only
because Windows linker output is not guaranteed byte-identical across builds.

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
invariant behaviors are represented; broader repositories, models, and real-agent
adherence to concurrent-work advice remain required before general claims.

ADR-0028 formalizes that boundary: engineering, controlled-efficacy, and
external-adoption evidence are separate tiers, and results never inherit a
broader claim from a narrower tier. The release-gated independent developer
pilot is the first external-adoption test; its failures and limitations are
published as evidence rather than filtered out.

## v0.1.2 external-adoption pilot

**Status: recruiting (2026-07-24).** The public v0.1.2 assets and checksums are
available, and the independent-developer pilot is open. No participant or
retention result is recorded yet. The gate requires 3-5 independent developers
who already use multiple coding agents, with at least two completing seven days
of real-work use. Until that happens, MindLeak makes no external-adoption claim.

The consent/privacy rules, unsupported-assistance boundary, observation fields,
and day-0/day-1/day-7 participant template are in
[v0.1.2-external-pilot.md](pilots/v0.1.2-external-pilot.md). Recruitment and
aggregate status are tracked without source, prompts, tokens, raw databases, or
participant names in
[2026-07-24-v0.1.2-pilot-status.json](../benchmarks/results/2026-07-24-v0.1.2-pilot-status.json).
Enrollment is open in [GitHub issue #8](https://github.com/monk-eee/MindLeak/issues/8).

## Two-agent duplicate-work overlap

The agent-loop outcome above is single-agent; the **two-agent duplicate-work
scenario was the required deterministic gap before any concurrent-safety claim.
ADR-0024 now closes that mechanism-level gate without claiming a filesystem lock.

**Scenario.** Two agents, A and B, share one repository. Agent A claims a task
and begins work on a set of files/symbols — declaring that scope on its claim
and/or producing MindLeak `observed`/`modified` attribution on those nodes.
Agent B, about to start a *different* task that happens to touch an overlapping
file or symbol, runs `check_overlap(paths, symbols)` **before** claiming.

| Gate | Blind control | Overlap-aware | Result |
|---|---:|---:|---|
| Concurrent claims on `src/lib.rs` | 2 | 1 after steer | Pass |
| Alice live claim scope returned | Not checked | 1 | Pass |
| Alice live mutation footprint returned | Not checked | 1 | Pass |
| Alice footprint after 336 hours | Not checked | 0 | Pass |
| Check changed task state or graph counts | n/a | No | Pass |
| Bob claim after `blocked_by` steer | n/a | Rejected | Pass |

The locked release-profile harness creates two different tasks over the same
path. The blind arm proves task-row CAS alone allows both owners. In the aware
arm the caller combines Lodestar's live claim result and MindLeak's derived
footprint, then chooses the supported `blocked_by` handoff. The checks themselves
leave task state and graph counts unchanged; no effective weight or lock is
stored.

Machine-readable result:
[2026-07-23-two-agent-overlap.json](../benchmarks/results/2026-07-23-two-agent-overlap.json).
Reproduce with `node scripts/evaluate-overlap.mjs` (also included in
`make bench`). The artifact records source hash, locked toolchain, evaluation
time, fixture, returned overlaps, and decay control. This proves deterministic
mechanics, not that independent agents always declare accurate scope or heed an
advisory; that behavioral question remains for external multi-agent pilots.

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

## Tool surface cost

Every benchmark above measures what the graph gives an agent. This one measures
what it charges before the agent asks anything: an MCP client loads the whole
advertised tool surface at connect time, in every session, in every worktree of
a fleet.

| Server | Tools | `tools/list` | Approx. tokens |
|---|---:|---:|---:|
| `mindleak-mcp` | 27 | 11.9 KB | ~3,052 |
| `lodestar-mcp` | 91 | 51.8 KB | ~13,265 |
| **Combined** | **118** | **63.7 KB** | **~16,316** |

The servers are asked over MCP/stdio rather than parsed from source, because
the number that matters is what a client is actually served. The unit is the
compact JSON of the `tools` array — what crosses the wire — and the token
figures are bytes/4, approximate by construction; only the tool count is exact.
A server that cannot be reached fails the run rather than being omitted, since
reporting one plane as the total would halve the number and read as progress.

The measurement exists because the surface had no budget. ADR-0059 recorded 89
`lodestar-mcp` tools on 2026-07-28; the first run recorded 90, and the
reconciled run recorded **91**. Nobody decided to grow it, and without this
measurement nothing would have noticed.

Machine-readable result:
[2026-07-29-tool-surface.json](../benchmarks/results/2026-07-29-tool-surface.json).
Reproduce with `make tool-surface`, or `node scripts/measure-tool-surface.mjs`
against an existing build. This measures cost, not benefit: whether a given
tool earns its place in the context window is a judgment, so the number is
meant to be held by a ratchet reporting at review rather than by a hard block.

**The ratchet is separate follow-up work.** A ratchet must name an active clause
that authorises it, no clause covers the tool surface, and a locally authored
clause cannot yet be added through an amendment with an enforcement contract.
That platform gap is task:4cef8e361fc7; task:8000f45e0dfd will register the
reviewed budget after it lands. Until then the number is measured and published
but not enforced, rather than being papered over by binding the ratchet to an
unrelated clause.

## PR effectiveness telemetry

`scripts/evaluate-pr-effectiveness.mjs` turns the earlier one-off production
audit into a bounded, repeatable report. It reads the latest pull-request cohort
from GitHub, reads task/thread/conformance projections from Lodestar, and reads
one MindLeak telemetry snapshot. It writes timestamped JSON plus a concise
Markdown summary under `target/telemetry/`.

```bash
node scripts/evaluate-pr-effectiveness.mjs --limit=50
```

The script requires an authenticated `gh` CLI and reachable MindLeak/Lodestar
servers. `--limit` accepts 1-100 and bounds GitHub collection. Nested PR commits
and checks are fetched per PR so the default 50-item cohort stays below GitHub's
GraphQL node budget on every platform. Override the output directory with
`--output-dir=<path>`.

The report keeps three evidence tiers separate:

1. **Runtime health** reports lifetime event/error counts, current failing-tool
  count, recent latency and redacted recent errors, dashboard polling share,
  and per-session memory-read-before-first-write adoption.
2. **Production PR correlation** links a PR to tasks only through explicit
  branch equality, a PR reference in the durable task thread, or a conformance
  evidence commit present in the PR. It reports check completeness, claim
  timing, conformance receipt categories/causes, human-resolution rate, and
  reconciliation merge churn. Missing required checks are unknown, never
  green.
3. **Controlled synthetic** exercises those linkage and missing-data rules with
  deterministic fixtures. It proves aggregation behavior, not product efficacy.

MindLeak tool events do **not** carry PR ids. The production tier therefore
reports unlinked PRs and unknown claim timing rather than inferring attribution.
Historical tasks written before evidence-window timestamps commonly remain
unknown. A live validation on 2026-07-30 linked 48 of 50 PRs with zero collection
warnings, zero self-validation failures, and all three synthetic gates green;
that moving cohort is an operational sample, not a committed benchmark baseline.

The harness stores no prompts, secrets, source, or model reasoning. It does not
emit task-thread text or raw conformance evidence. Recent telemetry errors retain
only timestamp, tool name, and an explicit category. MCP reads do append the
server's normal tool-call telemetry after the snapshot; the report names that
observer effect and describes the instant immediately before it.
