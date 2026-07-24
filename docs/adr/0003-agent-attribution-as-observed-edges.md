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

Attribution is an **edge**, not a column. As amended by ADR-0030, a client first
registers a session; identity-bearing ingest and focus operations upsert an
`agent:<session-identity>` node and a decay-weighted `observed` edge to the
primary node they wrote. `MINDLEAK_AGENT` supplies only the readable base label.

## Consequences

- **Merges, never clobbers.** A shared node stays shared and is reinforced by
  every agent; each agent's *attention* is its own edge. A column would be
  last-writer-wins on the shared row — destroying the multi-writer merge.
- **Attention decays like everything else.** `observed` edges are subject to the
  same half-life, so "who is working here *now*" naturally fades to "who worked
  here once". A column is static and would need a separate timestamp + sweep.
- **Queryable by the general graph engine.** `graph_multi_hop_query("agent:<id>")`
  traverses `observed` edges and `list_agents` gives the roster. Impact analysis
  deliberately excludes observations because shared attention is not dependency.
- **Registration is explicit.** Unknown or omitted session tokens fail before
  identity-bearing dispatch, so observations cannot be silently misattributed.
- **Isolation stays per-database.** Attribution does not add cross-repo id
  namespacing; sharing one DB across different repos remains unsupported (path
  ids collide). That is a separate, larger change if ever needed.
