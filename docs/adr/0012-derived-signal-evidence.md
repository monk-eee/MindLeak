# ADR-0012: Derived bounded signal evidence

- Status: Accepted
- Date: 2026-07-22
- Deciders: MindLeak maintainers
- Implements: [ADR-0005](0005-signal-weighted-decay.md)
- Related: [ADR-0006](0006-structural-dependency-edges.md),
  [ADR-0011](0011-passive-terminal-and-git-sensors.md)

## Context

ADR-0005 requires consequence and independent corroboration to outweigh raw
frequency. The first implementation only lengthened half-life for repeated edges
spread over 48 hours. It defeated same-session spam, but could not distinguish a
rare resolved failure from ordinary repetition and had no consolidation handoff.

The richer dependency graph and passive execution/commit evidence now make the
remaining proxies measurable without an LLM. Effective weight must remain
derived, and deterministic queries/pruning must never require a model server.

## Decision

Derive `SignalEvidence` from raw edge fields, node timestamps/content, and graph
relations at query and maintenance time. Apply a bounded multiplier to the base
half-life; never persist the multiplier or effective weight.

The multiplier starts at 1 and adds:

| Evidence | Contribution | Bound |
|---|---:|---:|
| Reinforcement count spread over at least 48h | `log2(count) / 8` | 1.0 |
| Independent source diversity beyond one | `0.5` per source class | 1.5 |
| Failure -> related change -> later green execution of the same command | 2.5 | 2.5 |
| First failure on an artifact at least seven days old | 0.75 | 0.75 |
| Incoming structural degree | `ln(1 + degree) / ln(17)` | 1.0 |
| Explicit architectural decision | 1.25 | 1.25 |

The final multiplier is capped at 8. Source classes are execution, commit,
structure, and explicit decision. Passive `observed` attention does not count as
deliberate attention.

All graph reads (`traverse`, impact, snapshot, counts, agent activity) and prune
use the same `GraphStore::signal_evidence`/`decay::signal_multiplier` path.
Returned weighted edges include the raw evidence, multiplier, and effective
half-life for auditability.

does not call an LLM and does not fail when no model is available.
`prune_graph` returns high-signal episodic edges below `2 * threshold` as
`signal_candidates`. Expired candidates are inactive in queries but retained
until optional `consolidate_signal` successfully stores a durable intent and
deterministic target links; only then are raw candidate edges acknowledged and
removed. Prune itself does not call an LLM and does not fail when no model is
available.

## Consequences

- Four hundred same-session green-build reinforcements cannot buy persistence.
- A failure corroborated by structure, a commit/decision, and later success can
  outlive repetitive noise, but the 8x cap and continuing time decay guarantee it
  still expires without reconfirmation.
- Query cost includes graph evidence lookups. A 200-edge snapshot measured
  16.757 ms p95 in the proof workspace; this remains a benchmarked regression
  surface.
- Consequence is a temporal proxy, not proof of causality: the same command must
  turn green after a related change, but coincidence remains possible.
  Boundedness and eventual decay limit the damage, and consolidation consumers
  retain provenance.
