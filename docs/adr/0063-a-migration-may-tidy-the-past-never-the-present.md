# ADR-0063: A migration may tidy the past, never the present

- Status: Accepted
- Date: 2026-07-28
- Deciders: MindLeak maintainers
- Related: [ADR-0054](0054-identity-is-the-session-not-the-process.md) (the collapse),
  [ADR-0030](0030-discrete-per-agent-identity.md) (session identity),
  [ADR-0048](0048-a-lapsed-lease-holes-the-window-it-does-not-move-it.md) (evidence windows),
  [ADR-0009](0009-evidence-backed-conformance.md) (evidence-bound verdicts),
  [ADR-0020](0020-task-lifecycle-states.md) (parking with a question)

## Context

ADR-0054 removed the label from the agent id and shipped a migration to heal
databases written under the old shape: rewrite every `session:v1:{name}:{fp}`
to `session:v1:{fp}`. It was written to be idempotent, and its doc comment says
so — the `GLOB` matches only ids that still carry a label, "so this is
idempotent and a second open rewrites nothing".

That is idempotence **by pattern**: rewrite whatever still looks unmigrated. It
holds only while nothing else is producing rows that look unmigrated. In a fleet
sharing one per-repository `spec.db`, something was: a **running server process
older than the file it was loaded from**, still minting labelled ids. Every open
by a newer binary re-fired the rewrite, and each firing looked exactly like the
first.

That cause is worth stating precisely, because the obvious diagnosis is wrong and
this ADR originally recorded it. Measured after the fact, driving the same
session token through each binary on disk: both repository release builds **and**
the installed extension binary returned the collapsed `session:v1:{fp}`. Nothing
stale was deployed anywhere. Only the *live* extension-hosted processes returned
the labelled id — they had been started from an earlier build, and the file
underneath them was replaced while they kept running. Rebuilding and reinstalling
would have changed nothing; restarting the process was the whole remedy.

So the hazard is not "someone deployed an old build", which sounds like a
deployment discipline problem and would be fixed by more discipline. It is that a
process outlives the file it was loaded from, and neither the file's timestamp
nor its contents can tell you what the running process actually is. In a fleet
where agents rebuild and redeploy the servers all day against one shared
database, that is ordinary, not exotic. The tell is a live process whose start
time predates the mtime of its own binary.

The rewritten set included `tasks.owner`.

Observed on 2026-07-28 (`task:f6daad456855`). One session, one client-minted
token. `open_session` returned `session:v1:copilot:b4baf280…` on both planes
while `board` reported that task's owner as `session:v1:b4baf280…` — same
fingerprint, no label — and the two flipped between consecutive reads with no
claim in between.

The holder was locked out of its own task, silently and completely:

- `check_conformance` refused: *"evidence agent does not own the task"*;
- `ask_question` returned `needs_input: false` — the owner guard rejected it, so
  the task could not even be **parked** with an explanation;
- a re-claim read as a *different* owner, which under ADR-0048 opens a fresh
  evidence window and reports `claim_lapses: 0`, indistinguishable from a first
  claim. Work committed inside the old window fell outside the new one.

The result was a task that could be neither proved nor parked, whose real work
shipped as a pull request with no receipt. That is precisely the outcome the
ledger exists to prevent.

## Decision

1. **`tasks.owner` is live state, not a historical record.** Every other column
   the collapse rewrites — `task_qa.author`, `constitution_versions.created_by`,
   `task_claim_transfers.*` — is a record of something that already happened, and
   rewriting one changes only how the past reads. `owner` is what
   `check_conformance`, `ask_question`, `renew_lease` and `complete_task` compare
   the caller against. Editing it mid-claim does not adjust a record; it
   **transfers ownership**. Ownership changes by claim, by release, or by an
   audited transfer — never as a side effect of opening a file.

2. **A live claim is never rewritten.** The collapse now skips any task that is
   `claimed` with an unexpired lease. A claim nobody is holding is safe to tidy;
   a claim someone is holding is not ours to touch. The heal still happens, one
   lease later, without taking the task off the agent doing the work.

3. **Migrations that touch identity or ownership run exactly once per database,
   recorded by name** in `schema_migrations`. Pattern-idempotence is not
   idempotence when a live writer can recreate the pattern. Recording the run by
   record rather than by shape makes "has this already happened?" a fact rather
   than an inference from the data the migration itself is editing.

4. **A rejected park says why.** `ask_question` returned `Ok(false)` for every
   failure at once — wrong owner, wrong status, no such task — which the MCP
   surface reported as `needs_input: false`, indistinguishable from a successful
   no-op. It now errors and names the caller, the status, and the holder. An
   agent whose identity has drifted must be able to find that out from the tool
   it reached for, not from a board read it had no reason to make.

## Consequences

An agent's claim survives another process opening the database. The heal from
ADR-0054 still lands, just never on a task in flight, and never twice.

Two things this deliberately does **not** do:

- **It does not stop a process running older code from minting labelled ids.** It
  cannot: the guard would have to live in that process, which by definition
  predates it. Restarting the server is the remedy, and nothing in the system can
  tell you it is needed — the binary on disk looks correct because it *is*
  correct. What changes is the failure mode — a split identity is *visible* (the
  fleet sees two agents) rather than *destructive* (a claim silently changes
  hands). A version handshake between processes sharing a database is a real
  option, and it protects only against divergences that start after both sides
  have it; that is a separate decision.
- **It does not repair the task it was found on.** `task:f6daad456855` shipped
  as PR #115 with no conformance receipt, and manufacturing one now — by
  re-committing the work into a fresh window, or completing on an empty in-window
  bundle — would assert proof the ledger never saw. It stays unproved, on the
  record, as the cost of the bug.

Migrations recorded in `schema_migrations` cannot be re-run by deleting rows and
reopening; a genuine re-run is a deliberate act, which is the point.
