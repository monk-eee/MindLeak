# ADR-0002 — SQLite + half-life decay over vector-only / per-event LLM memory

- **Status:** Accepted
- **Date:** 2026-07-22

## Context

Most "agent memory" tools fall into two traps:

1. **Vector-only memory** — dump every event into embeddings and run cosine
   similarity. Great for fuzzy recall, blind to structure and sequence, and it
   accumulates stale "junk" that dilutes every query over time (graph rot).
2. **Per-event LLM extraction** — call a model on every command to extract
   triples. Slow (seconds per event), expensive, and it stalls the editor.

MindLeak needs multi-hop structural reasoning ("what breaks if I change this?")
and it needs to forget stale context, cheaply and locally.

## Decision

- **Storage is SQLite** (bundled, single file, FTS5 + recursive CTEs) — not a
  vector DB or an embedded graph DB. Zero setup, portable, regenerable, and rich
  enough for graph traversal via adjacency queries.
- **Edges carry an exponential half-life; effective weight is computed at query
  time**, never stored: `W_eff = W_base · 2^(−Δt/half_life)`. Stale edges fade
  below a threshold and are pruned. This is the mechanism that prevents graph
  rot — it is the point of the system, not an optimisation.
- **The write path is deterministic and zero-token.** All ingestion is pattern
  matching (regex, path, exit code). Any LLM use is confined to an optional,
  asynchronous consolidation layer that is never on the hot path.

## Consequences

- **Do not** "simplify" decay to a fixed weight or disable it to "fix" stale
  context — that reintroduces graph rot. Tune `RelationType::default_half_life_hours`
  instead.
- **Do not** add a background job that rewrites edge weights row-by-row; the whole
  design relies on weight being derived.
- **Do not** add an LLM call to ingestion or query. If synthesis is needed, it
  belongs in `consolidate.rs`, off the hot path.
- Symbol/`calls` extraction is deterministic and heuristic: definitions and call
  sites are found by pattern matching, and `calls` edges are resolved **in-file**
  (a definition body referencing another symbol defined in the same file). This
  keeps the build dependency-light (no C grammars) and honours the zero-token
  rule. A Tree-sitter backend is the intended **precision** upgrade for
  cross-file and scope-accurate resolution; the `ingest::ast` interface is
  structured to allow the swap without touching callers.
