# ADR-0014: Per-project decay configuration

- Status: Accepted
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
2. **Committable per-project file** — `[decay]` and
  `[decay.half_life_hours]` sections in `<workspace>/.mindleak.toml`. This path
  is intentionally outside the ignored `.mindleak/` data directory so a team
  can commit its rhythm and share it across agents/worktrees. A non-empty
  `MINDLEAK_CONFIG` selects another path, resolved relative to the workspace.
  `MINDLEAK_WORKSPACE` explicitly identifies that project root for MCP hosts
  whose process working directory is not the workspace; otherwise startup uses
  the process working directory.
3. **Built-in defaults** — the current constants.

The file schema is explicit and rejects unknown keys so misspellings cannot
silently select defaults:

```toml
[decay]
prune_threshold = 0.05

[decay.half_life_hours]
modified = 24
failed_on = 24
imports = 168
```

Configuration is read **once at startup** into a resolved decay policy passed to
the `GraphStore`; it is not re-read per query (effective weight stays a cheap
pure function). The `GraphStore` uses the resolved half-life and threshold in
every active-edge, signal-handoff, count, snapshot, export, and prune decision.

### Guards (non-negotiable)

- **Decay stays on.** A half-life must be finite and `> 0`; individual wrong
  types, values `<= 0`, `NaN`, or environment parse failures are rejected
  (logged at `warn`) and the next valid layer wins. Malformed TOML syntax and
  unknown keys fail startup. There is no "infinite half-life" escape hatch —
  that would be disabling decay, which the invariants forbid.
- **Bounded range.** Finite positive half-lives clamp to `1h .. 8760h` and a
  finite positive prune threshold clamps to `0.001 .. 0.999`. Rejected values
  fall through; out-of-range positive values clamp rather than crashing.
- **Derived, not stored.** Effective weight remains computed at read time; no
  stored weight is rewritten when config changes. Re-tuning takes effect on the
  next query, retroactively, because weight is a pure function of `base`, the
  resolved half-life, and age.
- **Stored compatibility.** Existing `edges.half_life_hours` values remain the
  fallback when a relation has no file/environment override. A configured
  relation overrides that legacy value at read time; no migration or row rewrite
  is required.

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

## Scope boundary

Configuration is per relation only. Per-node-type and per-edge configuration are
deliberately excluded until measured use demonstrates a need.
