# Working Loop

Use this loop after both planes pass setup verification.

## 1. Open One Identity

Mint one 128-bit lowercase hexadecimal token and call `open_session` on the
Memory and Intent planes. Reuse it throughout the session. When Git facts are
available, declare branch, head SHA, expected base, and dirty state on both.

## 2. Establish Scope Before Editing

Collect concrete workspace-relative paths and stable symbol ids. Then run:

```text
lodestar.task_query(view="overlap", paths=[...], symbols=[...], session_id=...)
mindleak.check_overlap(paths=[...], symbols=[...], session_id=...)
lodestar.advise(node_ids=["artifact:<path>", "symbol:<id>", ...])
```

Interpret the results carefully:

- Lodestar reports intersections with active declared task scopes.
- MindLeak reports decay-active footprints, structural impact, prior failures,
  related intent, and ids it has never observed.
- `unknown` is not an all-clear. A quiet graph under-reports rather than guesses.
- On overlap, coordinate, serialize same-file work, narrow scope, or stop. Do
  not edit first and negotiate after the conflict.

## 3. Join the Intent Workflow When One Exists

If the user names a task, or the request clearly belongs to an existing task,
query it before creating anything new. Claim it with the current session and
the same path/symbol scope. If there is no task, ordinary repository work may
continue without manufacturing one.

Renew a held claim:

- after a build or test step;
- between files in a multi-file change;
- before a long-running command;
- whenever the next step could outlast the lease.

If a claim lapsed, use the server's explicit same-owner reclaim/recovery path;
never conceal the gap by changing identity.

## 4. Gather Evidence Proportionate to Risk

Prefer deterministic reads:

- `evidence_for` for facts and provenance about an artifact or symbol;
- `get_impact_radius` or graph traversal for likely dependents;
- task scope, governing clauses, and conformance history for intent;
- `working_set` for current session context.

Use semantic `recall` only when meaning-based discovery adds value. It may
abstain and must not replace direct inspection, exact search, or impact checks.

## 5. Change and Validate

Follow the repository's own instructions and test policy. Keep the edit within
the declared scope, renew any claim at step boundaries, and retain concrete
validation output for completion evidence.

## 6. Write Back Deliberately

For headless clients, explicitly ingest changed files after successful writes.
Record executions only when their outcome teaches a reusable fact, and never
include credentials or unfiltered sensitive output. Ingest the resulting commit
when one is created.

Use `record_architectural_decision` only for an actual design choice with a
useful decision and rationale. Routine implementation details do not become
architecture merely because the tool exists.

Promote expiring proven signals only through the cross-plane candidate and
promotion tools; do not manually turn one session's guess into durable knowledge.

## 7. Complete or Hand Off

For claimed work:

1. Assemble the changed artifacts, validations, and relevant evidence window.
2. Run the exposed conformance check for the task and current session.
3. Transition to complete with the evidence/check payload required by the
   running server schema.
4. If the verdict requires review, a waiver, or a human decision, report that
   state honestly instead of declaring completion.

For unfinished work, pause or release through Lodestar and leave a durable
reason. For same-file successors, complete the first task before the next owner
claims it; symbol scopes are not text locks.

Finish by reporting what changed, validation results, remaining gaps, task
state, and any follow-up addressed to another agent or a human.
