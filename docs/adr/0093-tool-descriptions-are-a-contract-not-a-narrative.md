# ADR-0093: Tool descriptions are a contract, not a narrative

- Status: Proposed
- Date: 2026-08-17
- Deciders: MindLeak maintainers
- Depends on: [ADR-0059](0059-the-tool-surface-is-a-vocabulary.md) (the tool
  surface is a vocabulary; sets the under-40-tools, under-6,000-token budget)

## Context

ADR-0059 gave the default MCP tool profile a hard budget — under 40 tools and
under 6,000 tokens — and consolidated many narrow verbs into a handful of
wide ones (`task_query` with `view`, `task_claim` with `step`,
`task_transition` with `to`) to hit the tool-count half of that budget.

Measured on `origin/main` (2026-08-17,
`node scripts/measure-tool-surface.mjs --json`): the Lodestar default profile
carries 15 tools at 23,902 bytes, ~5,976 of the 6,000-token ceiling. Tool
count has enormous slack (15 of 40); tokens have almost none (24 of 6,000).
The two halves of ADR-0059's own budget are no longer in tension with reality
in the same way — consolidation traded tool-count cost for description-length
cost, and the description side is now the one that is full.

Per-tool breakdown of where the bytes went (same measurement):

| tool | total | description alone |
|---|---|---|
| `task_query` | 3,952 B | 2,403 B |
| `task_transition` | 3,804 B | 1,576 B |
| `task_claim` | 3,501 B | 1,800 B |
| `task_create` | 1,813 B | 1,017 B |

Three consolidated tools carry nearly half the entire budget in their
top-level `description` string alone. Reading those descriptions, they mix
two different kinds of content:

1. **The operational contract.** What each `view` / `step` / `to` branch
   does, and what argument it needs — the facts a calling model must have to
   route correctly and supply the right arguments. This changes model
   behaviour and must be paid for in the schema.
2. **Narrative.** ADR citations, design history, and stylistic framing
   ("that is exactly the counter-pressure ADR-0059 identifies", "not a lock",
   worked examples of past incidents). This is valuable to a human reading the
   source or `docs/TOOLS.md`, and does not change what a model does when it
   calls the tool. It is paid for anyway, in full, on every single session,
   forever, because `tools/list` has no tiering finer than the whole
   description string.

The result, recorded as `gaps.d/lodestar-default-profile-token-budget-saturated.md`:
the *next* legitimate schema addition to any default-profile tool — a new
argument, a new `view`, a corrected `items` schema — has no room to land
without first trimming an unrelated tool's prose. That gap fragment
deliberately declined to design the fix and asked for a decision. This is it.

## Decision

1. **A tool's top-level `description` field is the operational contract
   only.** For a tool with a discriminator argument (`view`, `step`, `to`,
   `action`), the description states, for each branch: what it does, and any
   argument it requires or behaves unusually with. It does not restate why
   the tool is shaped this way, does not cite the ADR that created it beyond
   a single terminal reference where genuinely load-bearing (e.g. an
   attribution rule a caller must respect), and does not narrate design
   history or past incidents.
2. **Rationale, design history, and worked examples live in `docs/TOOLS.md`
   and the deciding ADR.** Both are committed, versioned, and readable by a
   human or an agent on demand — they cost nothing until read. `docs/TOOLS.md`
   rows may be as narrative as they already are; nothing about this decision
   asks for them to shrink.
3. **The budget test gets a headroom assertion, not just a ceiling one.** The
   existing `the_default_profile_is_under_budget` test keeps asserting the
   ADR-0059 hard ceiling (under 40 tools, under 6,000 tokens) — that number
   must never be silently exceeded. It gains a second, tighter assertion that
   the profile stays at least some fixed margin under that ceiling (this
   round: 500 tokens). The tighter bound is what turns "the wall is right
   there" into "there is room, and a test tells you when there stops being
   room" — it fails on the addition that *would* saturate the budget, not
   only on the one that finally breaches it.
4. **This is a standing rule for every default-profile tool, not a one-time
   cleanup.** A future PR that adds a `view`, a `step`, or an argument to a
   default-profile tool writes its description under rule 1 from the start,
   the same way a new tool anywhere else in the surface is specialist by
   default under ADR-0059 rule 3.

## Consequences

- The wire schema gets shorter without the tool getting less capable: every
  branch and argument a model needs is still there, stated more plainly.
- `docs/TOOLS.md` becomes the one place the full rationale lives, rather than
  being duplicated (often near-verbatim) between the committed doc and the
  JSON `description` a client loads every session.
- A reviewer approving a new default-profile argument now has a concrete
  question to ask: does this description addition state a fact the caller
  needs, or does it explain why? The second kind goes in the ADR or
  `docs/TOOLS.md` instead.
- The headroom assertion is a number this repository will need to revisit
  again — future consolidations or additions can still spend it. That is the
  point: spending it is now a decision a failing test names, not something
  that happens by accident one tool at a time.

## Alternatives considered

**Raise the 6,000-token ceiling.** Cheapest, and it does not fix anything —
it moves the wall out without changing what put the surface hard against it,
and the same saturation returns at the new number on the same trajectory.
ADR-0059 chose 6,000 deliberately as a cost worth defending; raising it to
relieve pressure the first time it is felt is exactly the "grows without
anyone deciding to grow it" failure ADR-0059 exists to stop.

**Tier the three consolidated tools further (split branches back into
narrower tools).** Rejected for the same reason ADR-0059 rejected leaving the
surface untiered: it would restore tool-count pressure to relieve
token-count pressure, trading one axis of the same budget for the other
rather than reducing what is actually paid for. It is also a larger,
breaking change to a surface ADR-0059 already broke once with a committed
migration window; a second break for the same tools within one release train
is a worse cost than trimming prose.

**Move detailed branch semantics to a specialist "tool help" verb fetched on
demand.** Attractive in principle — it fully separates "enough to route
correctly" from "everything else" — but it is a new mechanism (a tool whose
job is to describe other tools) for a problem this decision's rule 1 already
solves by editing the descriptions that exist. Worth reconsidering only if
rule 1's trims prove insufficient across a future addition; not justified as
the first move.

**Do nothing and let each future PR trim whatever it can find.** This is what
happened for `constitution_define`'s `items` fix, which paid for its own 25
bytes by trimming that tool's own description — a local fix with no rule
behind it, so the next tool to hit the wall repeats the search from scratch.
