# ADR-0066: Retrieval rides on the question already asked

- Status: Accepted
- Date: 2026-07-29
- Related: [ADR-0024](0024-preflight-overlap-detection.md) (pre-flight overlap
  detection), [ADR-0029](0029-proactive-constitutional-advice.md) (proactive
  constitutional advice), [ADR-0046](0046-agents-talk-through-the-durable-thread.md)
  (agents talk through the durable thread),
  [ADR-0053](0053-the-graph-records-events-not-conclusions.md) (the graph
  records events, not conclusions)

## Context

MindLeak exists so that an agent about to change a file can learn what the
repository already knows about it. Measured on this repository's own durable
telemetry, that has not been happening.

| Path | Lifetime tool calls |
|---|--:|
| Dashboard / self-observation (`graph_stats` 16,522, `telemetry_snapshot` 12,567, `graph_snapshot` 3,891) | **32,980** |
| Writes (`ingest_execution` 4,085, `ingest_file` 3,703, `ingest_commit` 321) | **8,109** |
| Reads at decision time (`recall` 49, `graph_multi_hop_query` 10, `working_set` 4, `get_impact_radius` 3) | **66** |

That is roughly **123 writes for every read**, and about 500 dashboard polls
for every read. `graph_stats` alone has consumed 3,405 seconds — 57 minutes of
cumulative compute answering "how many nodes are there", a question that has
never changed a decision. `graph_multi_hop_query` is currently in a failing
state, and its most recent call was malformed (`missing required argument:
seed_entity`); nobody noticed, because nothing depends on it.

The benchmarks are not wrong, and that is the trap. `docs/EVALUATION.md`
measures the graph at mean F1 0.77 against 0.44 for a vector arm, and 1.00 on
the structural impact question. That answers *if you ask, is the answer good* —
and never *does anyone ask*. Both facts hold at once: good retrieval, near-zero
retrieval.

The cause is structural, not carelessness. `get_impact_radius` is described in
its own tool definition as answering "what is structurally connected to a file
or symbol **you are about to edit**". It is exactly the right question, and it
needed a call of its own, made at the moment the work feels ready to start and
attention has already committed to the change. Nothing in this repository that
depended on remembering has ever been adopted: ADR-0046 measured the same zero
for a capability that needed a separate call, and the fix that worked there was
to hang the obligation off something already being done.

There is corroboration that does not appear in the telemetry at all. The
operational lessons this fleet actually relies on — the pre-commit stash race,
the `Copy-Item` mtime trap, the session-token identity collapse — were each
learned by burning a session, and each was written into a flat markdown file
rather than the graph. The memory system that is load-bearing for this
repository is a flat log. That is this product's own thesis failing on its own
codebase.

## Decision

**A pre-flight answers the whole pre-flight question.** `check_overlap` already
takes the paths and symbols an agent is about to touch — the exact seed set the
impact traversal needs — and already runs at exactly the right moment, because
the before-you-write checklist in `AGENTS.md` already mandates it. It now
returns, in one call:

- `footprints` — other agents' decay-active footprints (ADR-0024, unchanged);
- `impact` — dependents, previously failing executions, and related intents;
- `unknown` — requested ids the graph has never seen;
- `requested` — the ids the paths and symbols resolved to.

No new tool is introduced. Adding a sixth retrieval tool beside five that are
already unused would repeat the failure rather than fix it. `check_overlap`
keeps its name: the scarce thing here is the habit of calling it, and renaming
would spend that habit to buy tidiness.

**`unknown` is reported separately from an empty `impact`.** "The graph has
never seen this file" and "nothing depends on this file" are different facts,
and a caller that cannot distinguish them reads silence as reassurance. This is
the same failure shape as a conformance receipt that is aligned over an empty
bundle.

**The write path is untouched.** Every part of the pre-flight is a graph read;
no LLM enters the ingest or query path.

## Consequences

The impact half has a measured limit, and the checklist says so rather than
overselling it. On a real Rust file in this repository the traversal returns the
commit intents recorded against the file and the symbols it contains, but **no
cross-file dependents**, because Rust ingestion produces no inter-file import or
call edges. So it answers "what was decided about this file" well and "what
breaks if I change it" not at all for Rust. That limit is recorded in the Known
gaps of `DEVELOPERS.md`.

`AGENTS.md` previously excluded `get_impact_radius` from the checklist on the
grounds that it, like `recall`, "returns plausible strangers". That reason was
never substantiated in Known gaps and does not survive measurement: `recall`
answers by embedding similarity and genuinely can return a stranger, whereas the
impact radius is a deterministic traversal over recorded edges. The two were
conflated. `recall` remains off the checklist; the impact traversal is now on
it, with its real limitation stated.

Retrieval volume becomes measurable against write volume for the first time. If
the read-to-write ratio does not move, this decision failed and should be
revisited rather than supplemented with a seventh tool.
