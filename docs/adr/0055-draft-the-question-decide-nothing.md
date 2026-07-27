# ADR-0055: Draft the question, decide nothing

- Status: Accepted
- Date: 2026-07-28
- Deciders: MindLeak maintainers
- Refines: [ADR-0046](0046-agents-talk-through-the-durable-thread.md) (agents
  talk through the durable thread)
- Related: [ADR-0009](0009-evidence-backed-conformance.md) (evidence-backed
  conformance), [ADR-0024](0024-preflight-overlap-detection.md) (pre-flight
  overlap detection),
  [ADR-0054](0054-identity-is-the-session-not-the-process.md) (identity is the
  session), [ADR-0053](0053-the-graph-records-events-not-conclusions.md) (a verb
  nothing reaches for may as well not exist)

## Context

ADR-0046 built agent-to-agent dialogue properly: addressed rows on the durable
thread, discovery by query, wait-cycle deadlock detection, no channel and no
delivery. It works. Nobody has ever used it.

Measured on 2026-07-27, after an eight-hour session across roughly thirty
worktrees and fifteen merged pull requests:

```
pending_questions  ->  []
stalled_work       ->  5 x awaiting_human   0 x awaiting_agent   0 x deadlocked
```

Every stalled task was waiting on a person; one had been waiting seventy-five
hours. Not one agent had ever addressed a question at a peer.

This is the same shape as [ADR-0053](0053-the-graph-records-events-not-conclusions.md):
`record_knowledge` also exists, is also correct, and was also called zero times.
A verb nothing in the loop reaches for may as well not exist. The gap is not
capability, it is that **nothing surfaces that there is a question to ask.**

Most of what looks like a need to talk is a need to read, and that part should
stay a read. `check_overlap` already answers "is anyone else on this file";
`board` answers "who owns this"; `fleet_view` answers "where is everyone".
Asking an agent for a fact the ledger holds is a worse version of looking it up —
slower, and it parks the asker. A question about a readable fact is a design
smell.

The one thing no query can answer is **intent**: what a peer is about to do, and
whose change should land first. `check_overlap` can see the files two claims
share; it cannot see that one agent is midway through a rename that will move
them. That is the question worth asking, and today an agent has to notice the
collision, infer that intent is the open variable, and compose the question
itself — three steps at which it simply does not bother.

## Decision

**A collision the ledger already holds becomes a drafted, addressed question.
Drafting decides nothing and sends nothing.**

1. **`draft_questions(task_id)` proposes; it never acts.** It records nothing,
   parks nothing, and addresses nothing. `ask_question` remains the only thing
   that changes task state, so a draft nobody sends leaves no trace. That is
   what allows drafting to be generous with suggestions without polluting the
   durable thread.
2. **The collision is found deterministically.** Peers come from
   `check_claim_overlap` over declared scope (ADR-0024) — no model, no
   inference. If no live claim intersects, there is nothing to propose and the
   answer is empty.
3. **Only the phrasing is model-assisted, and it is optional.** A local
   OpenAI-compatible model may phrase the question from the two task titles and
   the shared scope; when none is reachable, a deterministic template carries it.
   The capability never depends on a model (invariant 4), and every draft
   reports `drafted_by: model | template` so a phrased sentence is never
   mistaken for a recorded fact.
4. **The model is asked to draft, never to arbitrate.** Its instructions permit
   questions about intent and ordering and forbid deciding who is right,
   assigning the work, or asserting facts it was not given. A model verdict
   carries no evidence, and ADR-0009 makes evidence the basis of every verdict
   in this system; an arbitrating model would be the one unauditable judgement
   in an otherwise auditable design. **No LLM decides anything here, and none
   ever should.**
5. **The template asks about ordering, not about facts.** It names the shared
   scope and asks "are you changing it, or shall I?" — deliberately the one
   thing the ledger cannot answer for itself.
6. **An agent is never told to ask itself.** ADR-0046 clause 6 refuses a
   self-addressed question because it parks a task waiting on the only agent
   that cannot act while parked. One agent holding two overlapping tasks is
   ordinary, so this is a skip rather than an error.

## Consequences

- The metric to move is `stalled_work`: `awaiting_human` down, `awaiting_agent`
  up, `deadlocked` visible rather than silent. A fleet that asks its peers stops
  halting every time its human is asleep.
- **Drafts will sometimes be unnecessary.** Two claims can share a glob without
  really conflicting, and the proposal costs a call to read and discard. That is
  the correct direction to err: an ignored suggestion is free, whereas an
  unasked question costs a parked task and a week of parking grace.
- **A phrased question can be wrong.** A model given two titles may draft
  something that misreads the work. It is a draft — the owning agent sends,
  edits, or discards it, and the provenance field says who wrote it.
- This does not make agents talk; it makes the question visible. If the working
  loop still never calls `draft_questions`, the outcome will be exactly
  ADR-0053's: a correct verb nobody reaches for. **The honest test is whether
  `awaiting_agent` is ever non-zero**, and that should be measured rather than
  assumed.
- Drafting depends on identity being stable (ADR-0054). A draft addressed at a
  forked identity is delivered to nobody, which is why that fix came first.
