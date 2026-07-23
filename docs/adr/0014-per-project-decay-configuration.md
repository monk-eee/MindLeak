# ADR-0014: Per-project decay configuration

- Status: Proposed
- Date: 2026-07-23
- Deciders: MindLeak maintainers
- Related: [ADR-0002](0002-sqlite-decay-over-vector-llm.md) (decay over vectors),
  [ADR-0005](0005-signal-weighted-decay.md) /
  [ADR-0012](0012-derived-signal-evidence.md) (signal-weighted decay)

## Context

Decay is MindLeak's load-bearing mechanism, but its rates are currently
**hard-coded**. Base half-lives live as constants in
`RelationType::default_half_life_hours` (`model.rs`):

| Relation tier | Relations | Base half-life |
|---|---|---|
| Episodic | `modified`, `failed_on` | 24h |
| Structural + intent | `calls`, `contains`, `refactored`, `imports`, `depends_on` (and `extends`/`implements` when built) | 168h |
| Attention / association | `relates_to`, `observed` | 48h |

The prune cutoff is a single constant, `decay::PRUNE_THRESHOLD = 0.05`.

Projects have different rhythms. A high-churn app may want yesterday's failures
to fade in hours; a slow-moving library may want a decision from last week to
still surface. Today the only way to change any of this is to recompile. The
project philosophy already says **tune half-lives, don't disable decay**
(AGENTS.md) — so a tuning surface is aligned with the design; it simply does not
exist yet.

This is orthogonal to signal-weighting: ADR-0005/0012 make the *effective*
half-life a function of derived evidence (corroboration, consequence). That
multiplier applies **on top of** a base half-life. This ADR is about making that
**base** (and the prune threshold) configurable per project. The two compose:

```text
effective_half_life = base_half_life(relation)      # tunable here
                      * signal_multiplier(evidence)   # derived, ADR-0005/0012
```

## Decision

Expose per-relation **base** half-lives and the prune threshold as layered
configuration. Nothing changes when nothing is set — the current constants remain
the defaults.

Precedence (highest wins):

1. **Environment variables** — per-relation and threshold overrides, consistent
   with the existing `MINDLEAK_*` surface:
   - `MINDLEAK_HALFLIFE_MODIFIED_HOURS`, `..._FAILED_ON_HOURS`, `..._CALLS_HOURS`,
     `..._CONTAINS_HOURS`, `..._IMPORTS_HOURS`, `..._DEPENDS_ON_HOURS`,
     `..._REFACTORED_HOURS`, `..._RELATES_TO_HOURS`, `..._OBSERVED_HOURS`,
     `..._EXTENDS_HOURS`, `..._IMPLEMENTS_HOURS`;
   - `MINDLEAK_PRUNE_THRESHOLD`.
2. **Committable per-project file** — a `[decay]` section in the project's
   MindLeak config file (align the exact path/format with whatever
   [ADR-0013](0013-local-data-lifecycle.md) established; otherwise
   `.mindleak/config.toml`). This lets a team commit its rhythm and share it
   across agents/worktrees.
3. **Built-in defaults** — the current constants.

Configuration is read **once at startup** into a resolved decay policy passed to
the `GraphStore`; it is not re-read per query (effective weight stays a cheap
pure function). The registered `effective_weight()` SQL function and
`GraphStore` receive the resolved half-lives rather than reading constants.

### Guards (non-negotiable)

- **Decay stays on.** A half-life must be finite and `> 0`; values `<= 0`, `NaN`,
  or absurd magnitudes are rejected (logged at `warn`) and fall back to the
  default for that relation. There is no "infinite half-life" escape hatch —
  that would be disabling decay, which the invariants forbid.
- **Bounded range.** Half-lives clamp to a sane window (e.g. 1h .. 1 year) and
  the prune threshold to `(0.0, 1.0)`. Out-of-range values clamp, not crash.
- **Derived, not stored.** Effective weight remains computed at read time; no
  stored weight is rewritten when config changes. Re-tuning takes effect on the
  next query, retroactively, because weight is a pure function of `base`, the
  resolved half-life, and age.

## Consequences

- **Positive.** Teams tune memory rhythm per project without recompiling and can
  commit it; the tuning surface makes the "tune, don't disable" philosophy
  actionable; it composes cleanly with signal-weighting.
- **Cost.** A small config-loading + validation path, and a documented schema.
  This sets a precedent for future tunables, so the schema must stay minimal and
  additive.
- **Risk.** Misconfiguration is a foot-gun in both directions — too long pollutes
  multi-hop traversals with stale facts; too short causes architectural amnesia.
  Mitigated by bounds, `warn`-on-reject, and documenting sensible ranges. The
  guard against non-positive half-lives protects the core invariant.
- **Invariants preserved.** Decay remains mandatory and derived; the prune
  threshold remains a single global; signal multipliers still apply on top.

## Open questions

- **Config file location/format** — reuse ADR-0013's mechanism if it introduced
  one, else `.mindleak/config.toml`. Decide before implementing to avoid two
  config systems.
- **Granularity** — per-relation only (this ADR), or eventually per-node-type or
  per-edge overrides? Start per-relation; resist finer granularity until a real
  need appears.
