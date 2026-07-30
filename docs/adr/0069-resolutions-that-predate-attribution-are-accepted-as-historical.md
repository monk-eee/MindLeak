# ADR-0069: Resolutions that predate attribution are accepted as historical

- Status: Accepted
- Date: 2026-07-29
- Deciders: monk-eee
- Related: [ADR-0009](0009-evidence-backed-conformance.md) (evidence is the basis
  of every verdict), [ADR-0031](0031-exportable-conformance-evidence.md) (a receipt must
  resolve), [ADR-0048](0048-a-lapsed-lease-holes-the-window-it-does-not-move-it.md)
  (a window survives a lapse, holes and all)

## Context

`resolve_task` accepts an `in_review` task to `done` on a human's judgement,
overriding a conformance verdict that declined to affirm the work. For most of
this project's life it validated the `human` argument — non-empty, not equal to
the acting agent's id — and then discarded it. The store call recorded only that
resolution had happened, never by whom.

The columns now exist and populate. Measured on the live board:

| | |
|---|---|
| tasks on the board | 268 |
| status `done` | 147 |
| resolver recorded | **17** |
| **no resolver recorded** | **130** |

The earliest recorded resolution is unix `1785285644` — today. Every completion
accepted before that carries no resolver and never will, because the identity was
never written down anywhere: not in the task row, not in `task_qa`, not in the
conformance record it overrode.

An earlier audit put this at 57 of 101. That figure was measured on 28 July
against a different criterion (the verdict on the receipt, not the presence of a
resolver) and is superseded by the count above. Both describe the same defect.

This matters because `ARCHITECTURE.md` calls the conformance chain "the only
trustworthy proof that the agents did the sanctioned work — every other signal is
narration an agent can fabricate". A human acceptance that names nobody is
narration. For 130 completions, the override is real and the overrider is
unknown.

## Decision

**Those 130 completions are accepted as historical. Their resolvers will not be
reconstructed, annotated, or re-attested.**

The set is defined by predicate, not by a number: a task whose status is `done`
and whose `resolved_by` is null was accepted before attribution existed.

Two things follow, and both are part of the decision rather than commentary on
it:

- `resolved_by IS NULL` on a completed task means **"predates attribution"**. It
  does not mean "accepted by nobody", and a report that renders it as an absence
  of authority is wrong about what happened.
- Every resolution from `1785285644` onward records its resolver, and a future
  audit may treat that boundary as sharp.

## Alternatives considered

**Annotate each task.** Writing a `task_qa` note on 130 tasks saying the resolver
was not recorded adds no information that this ADR does not already carry, and it
would make the threads look like they contain a finding when they contain a
restatement. Rejected: 130 copies of one sentence is not a record, it is noise
with a timestamp.

**Re-verify.** Asking the maintainer to accept 130 tasks again would populate
`resolved_by` with a real identity — attesting today to judgements made across
weeks, most of them about work nobody now remembers. That manufactures exactly
the attribution the ledger is missing, and it would be indistinguishable
afterwards from acceptances that really happened at the time. Rejected as worse
than the gap: a wrong attribution is more expensive than an absent one, because
it cannot be detected.

**Silently normalise.** Leave the 130 unremarked and let `resolved_by IS NULL`
be read as it happens to be read. Rejected because that is the failure this ADR
exists to close: an unrecorded decision becomes an accident nobody agreed to.

## Consequences

- The historical gap is permanent and now documented. Anyone auditing completion
  attribution has one place naming what is missing, how much, and why it cannot
  be recovered.
- Attribution is not retrofittable in general. This is the second time a column
  added after the fact left a population that can never be filled — the first
  being the evidence window on tasks claimed before `claim_started_at` was
  recorded. A field that identifies *who* or *when* is worth adding before it is
  needed, because the rows written without it are lost at the moment they are
  written.
- The remaining items of the audit that produced this — making a zero-node
  receipt visible as such wherever completion is reported, and deciding what
  `human` is allowed to mean when the guard is a string comparison rather than
  proof of a person — are untouched by this decision and still open.
