# ADR-0003 — Agent attribution as decay-weighted `observed` edges

- **Status:** Accepted
- **Date:** 2026-07-22

## Context

Multiple agents (and multiple worktrees) can point at one MindLeak graph. We want
to record **which agent touched what** — for filtering, "who worked on this
file", and richer impact analysis — without breaking the property that makes
concurrent use safe: deterministic, content-addressed ids so parallel ingests of
the same file/run *reinforce* the same nodes instead of clobbering them.

Two shapes were considered:

1. **A column** (`origin` / `agent`) on `nodes` and `edges`.
2. **An edge**: `agent:<id> --observed--> <node>`.

## Decision

Attribution is an **edge**, not a column. When `MINDLEAK_AGENT` is set, ingest
and focus operations upsert an `agent:<id>` node and a decay-weighted `observed`
edge to the primary node they wrote.

## Consequences

- **Merges, never clobbers.** A shared node stays shared and is reinforced by
  every agent; each agent's *attention* is its own edge. A column would be
  last-writer-wins on the shared row — destroying the multi-writer merge.
- **Attention decays like everything else.** `observed` edges are subject to the
  same half-life, so "who is working here *now*" naturally fades to "who worked
  here once". A column is static and would need a separate timestamp + sweep.
- **Queryable by the existing engine.** `graph_multi_hop_query("agent:<id>")`
  and `get_impact_radius` traverse `observed` edges for free; `list_agents` gives
  the roster. A column would need bespoke query paths.
- **Zero-cost when unused.** No `MINDLEAK_AGENT` ⇒ no `agent` nodes, no
  `observed` edges — identical to the pre-attribution graph.
- **Isolation stays per-database.** Attribution does not add cross-repo id
  namespacing; sharing one DB across different repos remains unsupported (path
  ids collide). That is a separate, larger change if ever needed.
