# ADR-0022: The learned-knowledge loop — promotion, revalidation, and advisory conformance

- Status: Proposed
- Date: 2026-07-23
- Deciders: MindLeak maintainers
- Related: [ADR-0004](0004-intent-plane-spec-brain.md) (intent plane; durable but
  revalidated), [ADR-0005](0005-signal-weighted-decay.md) /
  [ADR-0012](0012-derived-signal-evidence.md) (signal, not coincidence),
  [ADR-0009](0009-evidence-backed-conformance.md) (evidence-backed conformance),
  [SPEC-INTENT.md](../SPEC-INTENT.md) §2, §5

## Context

SPEC-INTENT §2/§5 promise a loop: MindLeak's signal-weighted decay promotes proven
regularities into durable **learned knowledge** before the raw episodes fade, and
that knowledge **informs conformance**. Verified 2026-07-23: the loop is **dormant**.
`lodestar_stats` reported `active_knowledge: 1`, and that single entry was
hand-authored via `record_knowledge` — the automated path has produced nothing.
Reading the code, two links are missing:

1. **Promotion is manual.** `Lodestar::consolidate` (facade/knowledge.rs) is a
   correct *gated* promoter — it refuses anything below `MIN_EVIDENCE_COUNT` (3)
   nodes or `MIN_EVIDENCE_SPAN_SECS` (3 days), mirroring
   [ADR-0005](0005-signal-weighted-decay.md)'s "don't launder coincidence". But
   nothing **feeds** it: MindLeak's proven `signal_candidates` (surfaced by
   `prune_graph` / `consolidate_signal`) are never handed to it. An operator must
   call `consolidate` by hand.
2. **Conformance never reads knowledge.** `evaluate_conformance` (facade/conformance.rs)
   scores evidence against *goal code bindings* only; it does not consult
   `active_knowledge`. So knowledge is **write-only** — even when present it cannot
   influence a verdict.

The consequence: every agent re-learns the same lessons from scratch; agent A's
hard-won regularity ("changes to X break Y") never steers agent B. This is the
feature that makes a *fleet* of agents compound rather than run as N amnesiac
sessions — and it is unwired. The building blocks already exist and must be
connected, not reinvented: `consolidate` (gate), knowledge revalidation decay
(`KNOWLEDGE_DEFAULT_HALF_LIFE_HOURS` = 720h ≈ 30 days), `reconfirm_knowledge`,
`prune_knowledge`, `active_knowledge`.

## Decision

Close the loop with two seams, keeping the existing gate and decay untouched.

### 1. Promotion: feed proven signal into the existing gate

A promotion pass hands MindLeak's proven-signal candidates to
`Lodestar::consolidate` — it does **not** invent a new threshold. Candidates come
from the deterministic signal path: an edge whose `signal_multiplier > 1`
(span-qualified reinforcement ≥ `SIGNAL_MIN_COUNT` across ≥ `SIGNAL_MIN_SPAN_HOURS`,
plus consequence / source diversity / surprise / structural centrality — decay.rs
`SignalEvidence`). Same-session spam and one-offs are rejected by the gate exactly
as today. The distilled `statement` may use the local model
(`consolidate_signal`), but promotion **works deterministically without it** (a
templated statement over the evidence node ids), so no LLM is a dependency.

### 2. The seam: MindLeak node ids → Lodestar knowledge

The promoter passes MindLeak node ids as **opaque strings** into `consolidate`'s
`evidence_node_ids`, with `first_seen` / `last_seen` from the edge provenance. No
shared tables, no shared transaction — the same loose seam as
[ADR-0004](0004-intent-plane-spec-brain.md). The stored `evidence` JSON already
carries the node ids, count, and span for later audit.

### 3. Revalidation: durable, not immortal

Fresh corroborating evidence calls `reconfirm_knowledge` (weight `+0.1`, resets
the revalidation clock — already implemented). Knowledge that is **not**
reconfirmed decays on its ~30-day half-life and `prune_knowledge` removes it once
inactive. Stale lessons fade; proven-and-repeated ones persist. Effective
knowledge weight is derived at read time, never stored
([ADR-0005](0005-signal-weighted-decay.md)).

### 4. Conformance consumption — ADVISORY only (the load-bearing rule)

`evaluate_conformance` additionally consults `active_knowledge`: when a task's
evidence changed-nodes intersect the nodes a knowledge statement references, it
**adds an advisory finding** ("a proven regularity says changes here break Y").

**Critical constraint:** knowledge is revalidated and decaying, so it MUST NOT
produce a `Violation` (hard block) the way an `invariant` goal does. Its influence
is bounded to: (a) attaching an advisory finding, and (b) at most nudging an
otherwise-`Aligned` verdict to `NeedsHuman` so a person looks — never a silent
hard fail, never `Violation`. Only the Constitution (`constraint`/`invariant`
goals) hard-fails. This keeps a stale or wrong regularity from blocking valid
work. The conformance read path stays deterministic — **no LLM**.

### Rejected alternatives

- **Knowledge as a hard constraint (`Violation`).** Conflates durable-revalidated
  knowledge with a constitutional invariant; a decayed or mistaken regularity
  would block correct work. Advisory only.
- **Non-decaying knowledge.** Rots into unchallenged dogma; the ~30-day
  revalidation half-life is the point.
- **LLM on the conformance read path.** Non-deterministic and on the completion
  hot path; distillation stays in the optional promotion step only.
- **Promoting on raw frequency / a new threshold.** The existing count-and-span
  gate already rejects same-session spam; reuse it, don't fork it.

## Consequences

- `evaluate_conformance` gains a knowledge-intersection pass that can add findings
  and escalate `Aligned → NeedsHuman`, but is structurally incapable of emitting
  `Violation` from knowledge. New tests assert exactly that ceiling.
- A promotion bridge (deterministic, model-optional) turns MindLeak proven-signal
  candidates into `consolidate` calls; it degrades cleanly with no model.
- Proof (end-to-end, no live model required): corroborated evidence (≥3 nodes over
  ≥3 days) promotes to knowledge while too-few/too-fast does not (the existing
  gate test); a later conformance check whose evidence touches the knowledge's
  nodes surfaces an **advisory** finding and never `Violation`; unreconfirmed
  knowledge decays out of `active_knowledge` and is pruned.
- SPEC-INTENT §5 (learned knowledge) and §2 (the "informs Conformance" arrow
  becomes real) are updated; README tool table gains any promotion verb;
  ARCHITECTURE.md notes the wired loop.
- This ADR carries no behavioural code; it is the design-first predecessor for the
  implementation task.
