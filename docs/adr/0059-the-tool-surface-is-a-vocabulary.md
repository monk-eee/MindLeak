# ADR-0059: The tool surface is a vocabulary, not an inventory

- Status: Accepted
- Date: 2026-07-28
- Accepted: 2026-07-29 by monk-eee (`design:0059-the-tool-surface-is-a-vocabulary`)
- Related: [ADR-0004](0004-intent-plane-spec-brain.md) (Intent Plane spec brain),
  [ADR-0046](0046-agents-talk-through-the-durable-thread.md) (agents talk
  through the durable thread),
  [ADR-0053](0053-the-graph-records-events-not-conclusions.md) (recall can say
  nothing)

## Context

The two MCP servers advertise **116 tools**, and every agent that connects loads
all of them before doing any work:

| Server | Tools | `tools/list` | Approx. tokens |
|---|---|---|---|
| `mindleak-mcp` | 27 | 11.7 KB | ~3,000 |
| `lodestar-mcp` | 89 | 48.9 KB | ~12,500 |
| **Combined** | **116** | **60.7 KB** | **~15,500** |

Fifteen thousand tokens is spent before the first question is asked, on every
session, in every worktree of a fleet. That is the measurable cost. The
unmeasurable one is worse: a caller choosing among 116 options chooses badly,
and a human trying to learn the surface cannot.

The naming does not help, because there is no vocabulary to learn. Across 116
tools there are **77 distinct leading verbs, 49 of which are used exactly once**.
The design lifecycle alone is fifteen tools:

```
register_design    accept_design      reject_design
promote_design     retire_design      supersede_design
list_designs       design_board       reconcile_designs
design_promotion   plan_design_promotion   revise_design_promotion
design_materialization_history   reopen_undecided_design
attribute_design_decision
```

Both `promote_design` and `design_promotion` exist and do different things.
`reopen_undecided_design` and `attribute_design_decision` are exact complements
— the same guard with its condition negated — and nothing in either name says
so. That pairing was introduced deliberately (ADR-0051) and is still, on the
evidence of this list, undiscoverable.

This is the shape the repository already recognises elsewhere. ADR numbers, the
ADR index and the changelog were each a hand-maintained thing that should have
been computed, and each got the same fix: **stop maintaining by hand what has a
rule.** The tool surface is the same failure in a different medium — a per-verb
tool for every state transition, when the states and transitions are already a
model we own.

The growth is not anyone's mistake. Each tool was the smallest honest addition
at the time, and the discipline of "one tool, one job" is right. What is missing
is the counter-pressure: nothing in the repository treats *the size of the
advertised surface* as a cost that must be paid down, so it only ever grows.

## Decision

**A tool is a verb in a vocabulary, not a row in an inventory.** Two rules:

### 1. Lifecycle transitions take a state argument; they do not each get a tool

Where a cluster of tools moves one entity through a state machine, they collapse
to one tool whose argument names the transition. The state machine is already
explicit in `model.rs`; the tool surface should reflect it rather than enumerate
it.

Design cluster, 15 → 4:

| New | Replaces |
|---|---|
| `design_register` | `register_design`, `reconcile_designs` |
| `design_decide` | `accept_design`, `reject_design`, `attribute_design_decision`, `reopen_undecided_design`, `supersede_design`, `retire_design` |
| `design_promote` | `promote_design`, `plan_design_promotion`, `revise_design_promotion` |
| `design_query` | `list_designs`, `design_board`, `design_promotion`, `design_materialization_history` |

`reconcile_designs` belongs to `design_register` and not, as an earlier draft of
this table had it, to `design_promote`. Reconciliation registers designs — it
imports ADR metadata and is explicitly forbidden from creating tasks — so filing
it under promotion would reproduce, inside the new vocabulary, exactly the
`promote_design`/`design_promotion` confusion this ADR exists to remove. It is
the batch shape of registration: one design when given `adr_path` and `title`, a
set when given `designs`, and passing both is refused rather than guessed.

The constitution/policy-pack cluster (~25) collapses the same way and by the
same rule.

Task cluster, 26 → 4. It is twenty-six rather than the sixteen estimated above:
counting the tools that answer questions *about* tasks, not only those that move
one, gives the true size of what a session has to hold.

| New | Replaces |
|---|---|
| `task_create` | `create_task`, `decompose_goal` |
| `task_claim` | `claim_task`, `renew_lease`, `release_task`, `recover_claim` |
| `task_transition` | `complete_task`, `resolve_task`, `block_task`, `reopen_task`, `abandon_task`, `pause_task`, `resume_task`, `ask_question`, `answer` |
| `task_query` | `board`, `next_task`, `task_scope`, `existing_work`, `check_overlap`, `stalled_work`, `task_qa`, `pending_questions`, `questions_for_a_human`, `draft_questions`, `claim_transfer_history` |

`task_claim` stays separate from `task_transition` even though claiming does
move a task between `open` and `claimed`. The distinction that earns it a verb
is compare-and-swap: a claim can be *lost* to another agent, and answering
"you did not get it, and here is why" is not the same kind of act as asserting
a status. Ownership and the lease are one family; what the owner then does with
the work is another.

Crucially, `design_decide` keeps the ADR-0051 guards intact — attribution still
refuses to overwrite a recorded name, reopening still defers to materialisation.
The guards move from *choosing between tool names* to *validating an argument*,
which is where a caller can actually be told why the request was refused.

### 2. The default profile is the common path; specialist tools are opt-in

`recall`, `working_set`, `next_task`, `claim_task`, `complete_task` and their
neighbours are what a session uses. The constitution, waiver, ratchet and policy
pack machinery is real and load-bearing, and is used by a small minority of
calls. It is advertised only when asked for.

Target: **under 40 tools and under 6,000 tokens in the default profile.**

## Consequences

**This is a breaking change to a published surface.** v0.1.3 is released and the
tool names are in agent transcripts, scripts and habits. It is worth doing
anyway, and doing now rather than later, because the surface only grows and
every release makes the break more expensive. It is not worth doing quietly:
each collapsed cluster ships with its old names accepted for one minor version,
answering with the new call to make — a deprecation that *teaches*, not a
back-compat shim that hides. That window closes on a named version, in this ADR,
and the removal ships in the same release train that opens it.

That deliberately contradicts the repository's default hostility to transitional
code (AGENTS.md, prime directive). The distinction is that a shim exists to
avoid migrating callers, and this exists to migrate callers we do not control:
an agent mid-session cannot read a changelog. The test is whether the transition
has a removal date in a committed document. This one does.

**Guards must not be lost in the collapse.** Every refusal that a separate tool
name currently encodes becomes an argument validation with the same message.
The existing tests move with them; a cluster is not collapsed until its tests
pass against the single tool.

**Documentation shrinks with it.** README currently carries 94 rows of tool
tables and is, in effect, the reference manual. Four verbs per cluster fit in a
paragraph.

**Tool count becomes a tracked number.** A surface with no budget grows without
anyone deciding to grow it. The token cost of `tools/list` is measurable in one
command, and belongs in the benchmark set beside the others.

## Alternatives considered

**Leave it and document better.** Documentation cannot fix fifteen verbs where
four suffice: the cost is paid in the model's context window and in the caller's
choice, neither of which reads the docs. This is the status quo and it is what
produced the complaint.

**Only tier the surface, do not consolidate.** Cheaper and helps the token cost
immediately, but leaves the vocabulary incoherent — `promote_design` and
`design_promotion` are equally confusing in a small profile as a large one. Tier
without consolidation also picks a favourite subset without fixing why the rest
is hard, so it should be done *with* rule 1, not instead of it.

**Consolidate to one `lodestar` tool with a command argument.** Maximum
compression, and it destroys the thing MCP is good at: a typed schema per verb
that the model can be held to. One mega-tool moves all validation to runtime and
all discoverability to prose.

**Bump to 0.2.0 and break cleanly with no deprecation window.** Honest, and the
repository's instincts favour it. Rejected because the callers are agents in
flight, not humans reading release notes — a session mid-task cannot be asked to
re-read anything. The one-version window exists for callers who cannot read.
