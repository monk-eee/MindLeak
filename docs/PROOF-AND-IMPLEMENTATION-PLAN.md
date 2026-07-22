# MindLeak Proof, Improvement, and Implementation Plan

## 1. Objective

Turn MindLeak from a credible prototype into a product whose core claims are
measured and defensible:

1. stale facts are removed rather than accidentally refreshed;
2. high-value development telemetry is captured passively;
3. impact analysis is cross-file, relation-aware, and accurate;
4. Lodestar conformance evaluates actual change evidence;
5. signal-weighted decay preserves consequence and corroboration, not repetition;
6. agents complete real tasks more reliably or with materially less exploration.

This plan follows the repository rule: **design/spec first, implementation
second, proof before promotion**. A capability remains labelled "in build" until
its phase exit gate passes.

## Progress - 2026-07-22

- **Phase 0 is in progress.** The repository now has an MIT license, committable
  Lodestar constitution export, pinned extension compiler/API versions, strict
  compile/lint/format gates, and a repeatable black-box graph baseline. A first
  baseline commit now anchors the result; Linux validation and comparative agent
  fixtures remain open.
- **Phase 1 graph lifecycle is implemented.** ADR-0007 adds artifact-owned
  structural snapshots, transactional reconciliation, legacy ownership
  migration, orphan cleanup, and node-only focus attention. The stale-structure
  scenario moved from fail to pass; the pre-change baseline remains immutable.
- **Unit-test result accounting remains blocked externally.** Unit Test MCP 1.3.6
  surfaces Cargo failures but reports zero successful tests and no Rust coverage;
  CI remains the complete-suite authority until the adapter is repaired.
- **ADR-0006 phases 1-3 are green for the supported fixtures.** The strict evaluator
  proves imports, named cross-file calls, and local/imported type hierarchy while
  retaining the ADR-0007 stale-fact guarantees. The hierarchy truth set reaches
  100% relation and impacted-type precision/recall; direct Cargo, npm, Go, and
  Python manifest dependencies reach 100% relation precision/recall. Broader
  language and real-repository truth sets remain open.
- **Phase 2 passive capture is implemented behind VS Code 1.93 shell
  integration.** Component fixtures drive terminal/Git events into MCP ingestion
  without test-authored ingestion calls. Output is opt-in/redacted/bounded,
  capture health is visible, and the 200-file/8 KiB local path measures 28.651 ms
  p95 against the 50 ms gate. A live Extension Host shell-integration smoke run
  remains an operator validation, not a deterministic CI fixture.
- **Phase 5 signal-weighted decay is green.** ADR-0012 derives bounded evidence
  from consequence, source diversity, surprise, structural degree, deliberate
  decisions, and weak span-qualified reinforcement. In the adversarial fixture,
  400 same-session reinforcements expire at six days while a resolved,
  corroborated failure remains active and still expires by sixty days.

## 2. Product Claims and Proof Gates

| Claim | Current truth | Required proof before claiming shipped |
|---|---|---|
| Stale context fades | Time decay works, but removed structure is not retracted and focus refreshes incident edges | Removed symbols/edges disappear on the next ingest; focus cannot revive obsolete facts |
| Passive episodic memory | VS Code 1.93 terminal/Git component fixtures pass; unsupported shells visibly degrade | Live fixture sessions capture execution/failure/commit evidence; p95 remains < 50 ms |
| Impact analysis | Bidirectional depth-2 graph proximity; mostly in-file structure | Cross-file truth-set precision >= 0.80 and recall >= 0.85 on supported fixtures |
| Conformance enforcement | A covering task is treated as aligned without inspecting actual changes | Violations, drift, missing evidence, and aligned changes each reach distinct tested outcomes |
| Decay noise, not signal | Adversarial and ablation fixtures pass with a bounded 1x-8x derived multiplier | Repetitive green-build noise fades before rare consequential/corroborated evidence |
| Better agents | No comparative evaluation | >= 15% fewer exploration tool calls or >= 10% higher task success, with no correctness regression |
| Collision-free local coordination | SQLite claim CAS has a concurrent race test | Zero duplicate winners across 10,000 repeated multi-connection claim races |

The agent-effect threshold is a **go/no-go product threshold**, not a unit-test
threshold. If neither agent metric improves after two tuning iterations, stop
expanding the architecture and reassess the product wedge.

## 3. Terminology

- **Authoritative structural snapshot:** the complete set of symbols and
  structural edges extracted from one file at one revision. A new snapshot
  replaces the previous snapshot owned by that file.
- **Episodic evidence:** an execution, failure, commit, decision, or observation.
  It accumulates and decays; it is not replaced by a file snapshot.
- **Fact owner:** the artifact whose extractor emitted a structural edge. This
  allows precise retraction even when the edge itself is symbol-to-symbol.
- **Evidence bundle:** actual changed nodes, failures, timestamps, and provenance
  supplied to Lodestar for conformance.
- **Truth set:** a reviewed list of expected dependencies or impacted files for a
  fixture repository and change scenario.
- **Control:** the same agent/task run without MindLeak, or with flat recent
  history only.

## 4. Target Flow

```text
editor / terminal / git
        |
        v
portable passive sensors ---- explicit MCP ingestion (other clients)
        |                                  |
        +----------------+-----------------+
                         v
              deterministic extractors
                         |
          +--------------+---------------+
          |                              |
 structural snapshot replacement     episodic append
          |                              |
          +--------------+---------------+
                         v
                decay-weighted graph
                 |                 |
       relation-aware impact   evidence bundle
                 |                 |
                 v                 v
               agent           Lodestar check
                                    |
                         aligned / drift / violation /
                              needs_human

Every phase also emits benchmark data; documentation claims advance only after
that phase's gate passes.
```

## 5. Phase 0: Establish a Reproducible Baseline

**Purpose:** preserve the current behavior, make validation trustworthy, and
measure before changing algorithms.

### Tasks

| Task | Implementation | Files | Effort |
|---|---|---|---|
| Create the first reviewed baseline | Stage named paths, review the complete diff, add MIT `LICENSE`, then create one conventional baseline commit | repository root | Low |
| Repair repository drift | Remove duplicate README license section; make `.lodestar/CONSTITUTION.md` genuinely committable; align TypeScript and ESLint parser versions | `README.md`, `.gitignore`, extension package files | Low |
| Restore trustworthy test execution | Fix Unit Test MCP custom root/config and make zero-test discovery fail CI | `.vscode/settings.json`, extension test config, CI | Medium |
| Add benchmark crate | Create deterministic fixture runner and JSON result schema; no LLM dependency | `crates/mindleak-eval/`, root `Cargo.toml` | High |
| Add fixture repositories | Small Rust, TypeScript, and Python projects with reviewed dependency and mutation truth sets | `fixtures/eval/` | Medium |
| Capture baseline | Run controls and current MindLeak; commit machine-readable results and a short report | `benchmarks/baseline/`, `docs/EVALUATION.md` | Medium |

### Evaluation result schema

```json
{
  "scenario": "typescript-cross-file-auth",
  "variant": "none|flat-history|mindleak",
  "task_success": true,
  "expected_impacts": 6,
  "returned_impacts": 8,
  "true_impacts": 5,
  "tool_calls": 14,
  "tokens": 18320,
  "duration_ms": 42100
}
```

### Exit gate

- Fresh checkout builds and validates on Linux and Windows.
- A zero-test run is a failure, not a pass.
- Baseline results are reproducible within documented tolerance.
- The repository has a reviewed commit history and an actual license file.

## 6. Phase 1: Fix Graph Fact Lifecycle

**Purpose:** make forgetting semantically correct before adding more edges.

### Design decision required

Add an ADR for **structural snapshot ownership and retraction**. Structural facts
are replaced by their owning artifact; episodic facts continue to accumulate and
decay. Focus updates attention only and never refreshes unrelated evidence.

### Model and storage changes

| Change | Implementation | Can copy from | Effort |
|---|---|---|---|
| Edge ownership | Add nullable `owner_id` for extractor-owned structural edges; migrate existing `contains`/`calls` rows where ownership is derivable | Existing additive migrations in `db.rs` | Medium |
| Transactional reconciliation | Replace all structural facts owned by an artifact with one extracted snapshot in a transaction | `GraphStore::upsert_edge` patterns | High |
| Orphan cleanup | Remove stale symbol nodes only when no current `contains` edge or other live reference remains | Existing prune queries | Medium |
| Focus semantics | `boost()` updates node access only; `observed` records agent attention; no incident-edge refresh | Existing `observe()` path | Low |

### Interfaces

| Method | Responsibility | Can copy from | Effort |
|---|---|---|---|
| `GraphStore::replace_structure(owner_id, nodes, edges, now)` | Atomically diff/retract/upsert one authoritative snapshot | Need to implement | High |
| `GraphStore::delete_orphaned_symbols(candidate_ids)` | Remove symbols no longer represented or referenced | Existing orphan execution cleanup | Medium |
| `MindLeak::ingest_file(path, content)` | Extract first, then call one reconciliation transaction | Existing method | Medium |

### Required tests

- Delete a function and re-ingest: its node, `contains`, and owned `calls` edges
  disappear.
- Rename a function: old identity disappears and new identity appears.
- Remove a call while retaining both functions: only the call edge disappears.
- Focus a file after a failure edge ages: focus does not reset the failure clock.
- A failed reconciliation rolls back without leaving a partial snapshot.

### Exit gate

All stale-structure mutation fixtures reconcile exactly, and repeated focus does
not alter structural or episodic edge timestamps.

## 7. Phase 2: Capture Execution and Git Evidence Passively

**Purpose:** remove dependence on agents voluntarily reporting what happened.

### Components

| Component | Trigger | Output | Effort |
|---|---|---|---|
| Terminal sensor | VS Code shell-execution start/end events | command, exit code, bounded output, changed files, timestamp | High |
| Git sensor | HEAD/ref change or repository state event | SHA, message, changed files, timestamp | Medium |
| Portable change detector | VS Code workspace create/change/delete events during a shell execution | normalized changed-file set | Medium |
| Privacy/size guard | Before MCP submission | redaction, allow/deny patterns, output cap | Medium |

### Files

- Create `editors/vscode/src/terminalSensor.ts`.
- Create `editors/vscode/src/gitSensor.ts`.
- Create `editors/vscode/src/changeDetector.ts`.
- Modify `editors/vscode/src/extension.ts` to register and dispose sensors.
- Modify extension settings for opt-in output capture, exclusions, and size caps.
- Keep `ingest_execution` and `ingest_commit` as the shared MCP boundary.

### Rules

- Use VS Code and portable `git` APIs/commands only; no platform-specific shell.
- Never capture environment variables, tokens, or command input marked sensitive.
- If shell integration is unavailable, report degraded sensing visibly; do not
  pretend capture is active.
- Bound output before it crosses MCP.
- Use the stable VS Code 1.93 shell-execution API and built-in Git extension API
  as specified by ADR-0011; do not scrape terminal buffers or `.git` internals.

### Exit gate

Fixture sessions produce execution, failure, modified-file, and commit evidence
without direct ingestion calls. Capture overhead is p95 < 50 ms excluding the
executed command itself.

## 8. Phase 3: Implement ADR-0006 and Relation-Aware Impact

**Purpose:** earn the cross-file impact-analysis claim.

### Delivery order

1. `imports` + `package` nodes + cross-file `calls`.
2. `extends` + `implements`.
3. manifest `depends_on`.
4. relation-aware impact traversal and ranking.

### Model and extraction interfaces

| Method/type | Responsibility | Can copy from | Effort |
|---|---|---|---|
| `NodeType::Package` | External dependency identity | Existing enum mappings | Low |
| `RelationType::{Imports,DependsOn,Extends,Implements}` | Typed structural relations and half-lives | Existing enum mappings | Low |
| `Extraction.imports` / `Extraction.hierarchy` | Return structured facts, not direct writes | Existing symbols/calls extraction | Medium |
| `manifest::extract(path, content)` | Parse supported manifests with structured parsers where available | Shipped for Cargo, npm, Go, and requirements files | High |
| `resolver::resolve_import(source, specifier, index)` | Resolve workspace artifacts or package stubs deterministically | Need to implement | High |
| `GraphStore::impact_radius(seed, policy, now)` | Traverse only relation/direction pairs relevant to impact | Existing traversal | High |

### Impact traversal policy

| Relation | Direction from changed node | Meaning |
|---|---|---|
| `contains` | both | move between artifact and owned symbols |
| `calls` | incoming | callers may be affected |
| `imports` | incoming | importing artifacts may be affected |
| `extends` / `implements` | incoming | derived/conforming types may be affected |
| `failed_on` | incoming | historical failure evidence |
| `refactored` / `relates_to` | incoming | explanatory context, lower rank |
| `observed` | excluded from impact | attention is not dependency |

Return impact results grouped as `structural`, `historical_evidence`, and
`context`; do not flatten all proximity into one score.

### Exit gate

- Structural fixture precision >= 0.95 and recall >= 0.90 per supported parser.
- Impact-result precision >= 0.80 and recall >= 0.85.
- Unsupported/ambiguous imports are explicitly reported, never silently treated
  as high-confidence dependencies.

## 9. Phase 4: Make Lodestar Conformance Real

**Purpose:** compare actual evidence against governing intent rather than treating
presence of a task as proof of alignment.

### Design decision required

[ADR-0009](adr/0009-evidence-backed-conformance.md) records the evidence
contract without modifying immutable ADR-0004. The stores remain separate.
MindLeak emits a bounded evidence bundle; Lodestar validates and records it. No
shared tables or cross-database transaction are introduced.

### Evidence contract

```text
ConformanceEvidence {
  schema_version, task_id, agent_id, started_at, ended_at,
  changed_node_ids[], failed_node_ids[],
  execution_ids[], successful_execution_ids[], commit_ids[],
  summary, provenance[]
}
```

`observed` establishes agent attribution but never counts as a change. The first
implementation derives changed nodes from `modified` and commit-backed
`refactored` evidence; uncommitted editor changes require an explicit episodic
mutation signal rather than reinterpreting focus.

### Interfaces

| Method | Responsibility | Can copy from | Effort |
|---|---|---|---|
| `MindLeak::evidence_for(agent, since, until)` | Build a provenance-bearing bundle from attributed episodic edges in the claim window | Existing traversal/query methods | High |
| `check_conformance(evidence, task_id)` | Detect ungoverned drift, task/goal mismatch, missing evidence, and semantic contradiction | Existing evaluator, rewritten | High |
| `complete_task(task_id, agent, evidence)` | Guard ownership, evaluate evidence, then transition based on verdict | Existing completion guard | High |
| `record_conformance(...)` | Persist evidence/provenance with verdict | Existing audit insert | Medium |

### Verdict rules

| Condition | Verdict / transition |
|---|---|
| No evidence for a claimed implementation task | `needs_human`; remain `in_review` |
| Governed node changed without a covering task | `drift`; remain `in_review` |
| Task covers a different governing goal | `drift`; remain `in_review` |
| Active `forbid_change` binding changed | `violation`; block |
| Normative semantic check unavailable or uncertain | `needs_human`; remain `in_review` |
| Evidence matches task and deterministic checks pass | `aligned`; complete |

An LLM receives a bounded change summary/diff evidence, not a comma-separated
list of node IDs.

### Exit gate

Every verdict is reachable through an integration test. A covering task alone
can never produce `aligned`, and missing model access can never fabricate
alignment.

## 10. Phase 5: Complete ADR-0005 Signal-Weighted Decay

**Purpose:** retain rare consequential evidence while repetitive noise fades.

The former count-plus-span graduation is retained as one weak term inside the
completed ADR-0012 evidence model.

### Signal components

| Component | Deterministic evidence | Weighting intent |
|---|---|---|
| Span-qualified reinforcement | count across >= 48h | weak corroboration only |
| Source diversity | execution + commit + structure + decision converge | stronger than repetition |
| Consequence | failure followed by related change and later success | strong |
| Surprise | failure on a previously stable path | medium |
| Structural centrality | bridge/in-degree under ADR-0006 relations | medium, capped |
| Deliberate attention | explicit decision or human boost | medium, never permanent |

### Interfaces

| Method | Responsibility | Can copy from | Effort |
|---|---|---|---|
| `GraphStore::signal_evidence(edge, now)` | Compute source diversity, consequence, and structural proxies | Shipped | High |
| `decay::signal_multiplier(evidence)` | Pure bounded mapping from evidence to half-life multiplier | Shipped | Medium |
| `GraphStore::expiring_signal_candidates(now)` | Find proven signal near expiry for consolidation | Shipped | High |
| `prune()` | Surface eligible signal before deleting noise; optional consumer consolidates | Shipped handoff | High |

Effective weight remains derived. Stored fields are raw evidence/provenance only.

### Adversarial proof

- 400 same-session green builds do not earn durable signal.
- One isolated failure fades normally.
- A failure linked to a code change and later green execution outlives repetitive
  green-build noise.
- Independent execution, commit, and structural evidence outranks same-source
  repetition.
- Signal multipliers are bounded and eventually decay without reconfirmation.

### Exit gate

The adversarial suite passes and an ablation report shows the contribution of
each signal component. No global half-life inflation is permitted.

## 11. Phase 6: Agent Outcome Evaluation

**Purpose:** decide whether this is a product, not merely an elegant substrate.

### Experimental design

Run the same pinned agent/model and task set in randomized order:

- **A:** no memory tool;
- **B:** flat recent execution/commit history;
- **C:** MindLeak memory plane;
- **D:** MindLeak + Lodestar for parallel tasks.

Use fresh worktrees and databases for each run. Repeat each scenario enough times
to report medians and variance. Keep evaluator truth hidden from the agent.

### Scenario set

- Locate and fix a cross-file regression.
- Predict tests/files impacted by an API change.
- Resume an interrupted task after context reset.
- Avoid repeating a previously failed approach.
- Two agents partition related work without duplicate ownership.
- Change governed code while violating an invariant.

### Metrics

- task success and regression count;
- impacted-file precision/recall;
- exploration tool calls and files opened;
- tokens and wall-clock time;
- repeated failed approach count;
- duplicate work/claim count;
- ingestion/query p50 and p95 latency;
- database size and active-edge count over simulated time.

### Decision gate

Proceed to packaging only if MindLeak achieves at least one primary improvement:

- >= 15% fewer exploration tool calls, or
- >= 10% higher task success,

with no statistically meaningful correctness regression and acceptable local
overhead. Otherwise run at most two diagnosis/tuning iterations, then narrow or
stop the product rather than adding more planes or relations.

## 12. Phase 7: Productization

Only after Phase 6 passes:

- one-command cross-platform installation and server registration;
- signed/versioned binaries for Windows, macOS, and Linux;
- VS Code package with server health, capture health, and degraded-mode status;
- migration and backup documentation;
- retention/privacy controls and graph reset/export;
- a combined onboarding path for the two MCP servers without merging their
  stores or invariants;
- release notes containing benchmark results, limitations, and supported
  language matrices.

## 13. Parallel Work and Dependencies

```text
Track A: baseline/eval harness -------------------------------+
                                                               |
Track B: graph lifecycle -> passive sensors                    |
                                                               +-> agent evaluation -> release
Track C: ADR-0006 structure -> relation-aware impact ----------|
                         |                                     |
                         +-> ADR-0005 complete signal model ----|
                                                               |
Track D: evidence contract -> Lodestar conformance ------------+
```

### Safe parallelism

| Track | Can run concurrently with | Shared hotspots |
|---|---|---|
| Baseline/evaluation | All implementation tracks after baseline capture | root workspace, CI |
| Graph lifecycle | Lodestar conformance design | `mindleak-core` graph/schema |
| Passive sensors | Lodestar core work | extension activation/client |
| ADR-0006 | Lodestar conformance | `model.rs`, `ast.rs`, graph traversal |
| Lodestar evidence contract | ADR-0006 implementation | MCP schemas and shared evidence types |
| ADR-0005 completion | Passive sensors | graph SQL; depends on ADR-0006 centrality |

### Critical path

Baseline -> graph lifecycle -> ADR-0006 -> relation-aware impact -> complete
signal model -> comparative agent evaluation -> productization.

## 14. Effort Summary

| Phase | Complexity | Indicative single-engineer duration |
|---|---|---|
| 0. Baseline and evaluation harness | High | 1-2 weeks |
| 1. Graph lifecycle correctness | High | 1 week |
| 2. Passive execution/Git capture | High | 1-2 weeks |
| 3. ADR-0006 and impact ranking | High | 2-3 weeks |
| 4. Real Lodestar conformance | High | 1-2 weeks |
| 5. Complete ADR-0005 | High | 2 weeks |
| 6. Agent evaluation | High | 1-2 weeks after harness |
| 7. Productization | High | 2+ weeks, conditional |

With parallel ownership, Phases 1-4 can overlap after the baseline is frozen.
The minimum credible proof milestone is roughly 6-8 focused engineering weeks;
full productization is conditional on the evaluation gate.

## 15. Definition of Done

MindLeak is ready to call a product when:

- repository state and test execution are reproducible;
- file ingestion retracts stale structural facts transactionally;
- executions and commits are captured passively with visible degradation;
- cross-file impact meets its truth-set thresholds;
- conformance evaluates evidence and all verdicts are reachable;
- signal decay beats repetitive-noise adversarial cases;
- controlled agent experiments cross a primary product threshold;
- documentation labels implemented, measured, and planned behavior accurately.

Until then, describe MindLeak as an experimental local context-graph engine, not
as a proven replacement for agent memory systems.
