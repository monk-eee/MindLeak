# ADR-0005 — Signal-weighted decay ("decay noise, not signal")

- **Status:** Accepted
- **Date:** 2026-07-22

## Context

[ADR-0002](0002-sqlite-decay-over-vector-llm.md) established **pure time-decay**:
`W_eff = W_base · 2^(−Δt/half_life)`. The only thing resisting the clock is
*recency of reinforcement*. That makes time (and repetition) the sole judge of
what survives — and it is a weak, sometimes **inverted**, proxy for value:

- A build that runs green 400 times is **reinforced** every run, so it survives.
  Pure noise, protected by repetition.
- A one-off `failed_on` that revealed a real coupling between two modules — rare,
  never repeated because it was *fixed* — decays on the same 24h clock and is
  gone in days. Pure signal, discarded for being quiet.

So the current mechanism can preserve the most boring thing in the graph and
forget the most valuable. **Frequency is not signal; recency is not signal.
Consequence and corroboration are.**

We also want hard-won "experience" to persist. But simply lengthening half-lives
reintroduces exactly the graph rot ADR-0002 exists to prevent. The resolution is
not "decay less" — it is **decay noise, not signal**: keep decay ruthless on raw
episodics, but let *signal* resist it and be consolidated before the specifics
fade. Decay is not the enemy of signal — it is the **test signal must keep
passing.**

## Decision

- **Decay stays the point** (ADR-0002 holds), but effective weight gains a
  **signal term**: `W_eff = W_base · 2^(−Δt/half_life) · f(signal)` — or,
  equivalently, signal raises the half-life tier. The invariant is *signal
  resists decay; noise does not*. The exact functional form is an implementation
  choice deferred to the consolidation worker.
- **Signal is estimated from observable proxies, weighting consequence and
  corroboration OVER frequency:**
  1. **consequence / outcome-coupling** — sat on a causal chain that *resolved*
     something (failures → fix → green);
  2. **surprise / prediction error** — broke a *previously stable* path;
  3. **corroboration / convergence** — independent sources (execution + commit +
     symbol) implicate the same node;
  4. **structural centrality** — bridges otherwise-separate clusters;
     load-bearing, not a leaf;
  5. **deliberate attention** — human focus / a recorded decision.
- **Reinforcement graduates the half-life, but only across a span** (weeks), not
  within a single session — this defeats the frequency trap (build spam cannot
  buy permanence).
- **Decay drives consolidation, not just pruning.** The maintenance pass routes
  proven-signal clusters that are *about to expire* to the consolidation worker
  (distil → durable **learned-knowledge** node with provenance) and hard-deletes
  only un-distilled noise. "Consolidate before decay." See
  [SPEC-INTENT.md §5](../SPEC-INTENT.md).
- **Effective weight remains derived, never stored** (invariant 3 intact): the
  signal term is computed at query/maintenance time from graph structure and
  evidence, not written back onto rows.

## Consequences

- **Do not** use raw frequency/recency as the sole salience signal — it protects
  repetition and forgets rare insight. Weight consequence and corroboration.
- **Do not** "fix" lost experience by disabling decay or inflating half-lives
  globally — that is graph rot by another name. Consolidate the signal instead.
- **Learned knowledge is durable but not immortal.** The consolidation output
  carries a *long-but-finite* half-life and is revalidated by fresh evidence —
  distinct from the authored Constitution, which never decays. A regularity that
  stops being true self-corrects.
- **New failure mode to test against: laundering coincidence into permanence.**
  Promotion is gated by evidence + span + provenance and is reversible; a
  low-evidence pattern must not graduate.
- **Open:** the exact form of `f(signal)`, the promotion thresholds, and whether
  signal multiplies `W_eff` or shifts half-life tiers — settled when the
  consolidation worker is built (SPEC-INTENT Phase 4).
