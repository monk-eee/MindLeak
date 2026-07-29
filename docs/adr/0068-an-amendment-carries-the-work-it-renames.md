# ADR-0068: An amendment carries the work it renames

- Status: Proposed
- Date: 2026-07-29
- Deciders: MindLeak maintainers
- Related: [ADR-0063](0063-a-migration-may-tidy-the-past-never-the-present.md)
  (a migration may tidy the past, never the present),
  [ADR-0025](0025-authoritative-checked-conformance.md) (authoritative checked
  conformance),
  [ADR-0029](0029-proactive-constitutional-advice.md) (proactive constitutional
  advice),
  [ADR-0009](0009-evidence-backed-conformance.md) (evidence-backed conformance),
  [ADR-0060](0060-work-whose-product-is-not-code-must-still-conform.md) (non-code
  work must still conform)

## Context

Amending the constitution copies each clause forward under a new id —
`goal:{slug}@{constitution version}` — and retires the outgoing row. The retiring
was a bare status flip:

```sql
UPDATE goals SET status = 'superseded'
 WHERE constitution_version = ?1 AND status = 'active'
```

`superseded_by` was left NULL. Since the amendment *renames* every clause it
carries forward, nothing could follow the rename, and everything that named a
clause by id was silently left behind.

Measured on this repository after `constitution:v2` was adopted:

| | |
|---|---|
| active clauses | 25 |
| active clauses holding code bindings | **0** |
| bindings held by superseded clauses | **156 of 156** |
| tasks under superseded clauses | **217 of 217** |
| superseded clauses recording a successor | **1 of 26** |

The governance layer was inert, and — this is the part that let it survive — it
read as healthy:

- `governing_goals` resolves through `active_bindings_for_node`, which filters
  `g.status = 'active'`. It reported "nothing governs this" for files that were
  demonstrably bound. Indistinguishable from a clean file.
- `advise` answered *"no active clause governs this change; proceed"* for every
  change. That reads as approval. It was the constitution being disconnected.
  No `forbid_change` lock could fire.
- `in_scope` is computed from the same active-only lookup, so a task whose goal
  is superseded can never have in-scope evidence — which is every task on the
  board. Correct work audited as `evidence does not touch code bound to the task
  goal`.
- `drift` could never fire at all, because `governing.other` is drawn from
  active clauses only. The drift half of conformance was structurally dead while
  the rest kept producing normal-looking verdicts.

The same function already carried **controls** forward, matched by slug, with a
comment explaining that an orphaned control "reads exactly like a working one".
That reasoning is right and was applied to one table out of three. Bindings and
tasks were missed.

## Decision

**An amendment records where each clause went, and carries the work with it, in
one transaction.**

1. **The successor is recorded.** For each outgoing clause with a same-slug
   incoming clause, `superseded_by` names it. `slug` is documented in the schema
   as "stable identity across versions", so this is exact, not inferred. A clause
   the amendment drops has no successor and is left alone.

2. **Bindings and in-flight work move with the clause.** `goal_code` rows and
   non-terminal tasks are re-pointed at the successor.

3. **It is one transaction, and that is the substance of this decision, not
   tidiness.** The two halves are unsafe apart:
   - Move bindings while tasks still name the outgoing clause, and every live
     task's evidence becomes `governed code changed without a covering task` —
     drift, reported against work nobody touched.
   - Move tasks while bindings lag, and conformance goes blind instead, because
     no clause binds the file that changed.

   There is no tool to retarget a task's goal, and there should not be one: the
   only legitimate reason a task changes clause is that the clause it served was
   renamed by an amendment. That makes the amendment the correct and only home
   for this.

4. **Finished work does not move.** A `done` or `abandoned` task keeps naming the
   clause it was actually judged under. Rewriting that would rewrite the audit
   (ADR-0025).

5. **A live claim does not move either — including in the repair migration.**
   A task that is `claimed` with an unexpired lease keeps its clause. Its goal is
   what conformance judges the holder's evidence against, so moving it mid-claim
   changes the rule beneath someone doing the work. ADR-0063 already settled this
   shape for `tasks.owner`; `goal_id` is live state by the same argument. On this
   repository 3 of the 56 affected tasks were held under a live lease when the
   repair was measured.

6. **Existing databases are repaired by a `run_once` migration** applying the
   same rules. Recorded by name in `schema_migrations`, per ADR-0063 §3, because
   pattern-idempotence is not idempotence when a live writer can recreate the
   pattern.

## Consequences

The constitution governs code again, and `advise` can mean something. Dry-run
against a copy of this repository's ledger: 156 stranded bindings → 0, 56 live
tasks → 0 remaining on superseded clauses, 26 successors recorded, all 178
finished tasks untouched.

**Clauses held under a live lease at migration time stay stranded.** The
migration runs once and skips them, so unlike ADR-0054's heal they do not get a
second attempt "one lease later". They move at the next amendment, which is an
attributed act rather than a side effect of opening a file. That is the correct
trade: a handful of stranded rows is recoverable, a claim changing meaning under
its holder is not.

**This does not repair the verdicts already recorded under the disconnected
constitution.** Audits that returned `needs_human` because `in_scope` could not
resolve are historical records, and ADR-0025 says we do not rewrite them. They
need re-auditing or explicit disposition — the same problem ADR-0058 raises for
work that shipped without leaving the board, and it should be solved once for
both.

**A slug rename is not carried.** If an amendment renames a clause's slug as well
as its version, this sees no successor and leaves the old clause stranded, which
is the safe direction: inventing a mapping between differently-named clauses is
exactly the guess that produced the original defect.

## Alternatives considered

**Repair the ledger with a script, using `link_goal_to_code`.** This was the
first plan and it is wrong. It can move bindings but has no way to move tasks, so
it produces the drift storm in §3 — 57 live tasks reporting drift at once — and
it writes a governance change with no attribution and no audit trail. The absence
of a task-retargeting verb is a design signal, not a gap to work around.

**Make conformance follow `superseded_by` at read time** instead of moving
anything. Less invasive, and it leaves the ledger permanently describing a world
that no longer exists: every read pays a hop, and the hop is easy to forget in
the next query that joins `goal_code` directly — which is how `governing_goals`
came to disagree with `check_conformance` in the first place.

**Bind the v2 clauses in addition to the v1 ones**, leaving both. Drift again:
`governing.other` counts any governing clause no covering task serves, so the
duplicate immediately reports every change as ungoverned drift.
