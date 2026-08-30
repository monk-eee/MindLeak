- **Identity migrations can still reset an evidence window invisibly — OPEN.**
  [ADR-0063](docs/adr/0063-a-migration-may-tidy-the-past-never-the-present.md)
  fixed live-claim ownership rewrites, but one residual remains. Across a single session
  holding one client-minted token, `open_session` (both planes) returned
  `session:v1:copilot:b4baf280…`, while `board` reported the task's owner as
  `session:v1:b4baf280…` — the same hash, with and without the label
  (ADR-0054 removed it).
  The owner string **flipped between two consecutive `board` reads with no
  intervening claim** (labelled at `1785234086`, unlabelled at `1785234449`),
  which points at more than one `lodestar-mcp` build attached to the same
  `spec.db` rather than at anything the task did. **Not a stale deployment** —
  that was the first diagnosis and it was wrong. Driving the same session token
  through each binary on disk, both repository release builds *and* the
  installed extension binary returned the collapsed id. Only the **live**
  extension-hosted processes returned the labelled one: they had been started
  from an earlier build and the file underneath them was replaced while they
  kept running. Restarting the server was the whole remedy; rebuilding and
  reinstalling would have changed nothing. The tell is a live process whose
  start time predates the mtime of its own binary (ADR-0063).
  Impact, measured on `task:f6daad456855`, is that the whole closing loop is
  unreachable for such a session:
  - `check_conformance` refuses with *"evidence agent does not own the task"*;
  - `ask_question` returns `needs_input: false` — the owner guard rejects it, so
    the task cannot even be **parked** with an explanation;
  - the task stays `claimed` until the lease lapses, with no receipt and no
    durable note, which is the one outcome the ledger exists to prevent.
  A second, independent effect compounds it: a re-claim after a lapse only
  preserves `claim_started_at` for the *same* owner (ADR-0048), so a changed id
  reads as a different agent, opens a **fresh** window, and reports
  `claim_lapses: 0` as if nothing happened. Work committed at `1785223462` under
  a window started at `1785223449` fell outside a window later moved to
  `1785234086`, and `check_conformance` refused the real evidence with
  *"evidence interval falls outside the live claim"*.
  Two things worth deciding rather than patching: whether identity should be
  pinned per session against the *token* rather than whatever the current binary
  formats, and whether a window reset should be visible (it currently looks
  identical to a first claim). Do not "fix" this by re-committing work into a
  fresh window, or by completing on an empty in-window bundle — both assert
  proof the ledger never saw.
  **Fixed:** ADR-0063 stops the collapse rewriting the owner of a live claim and
  records identity migrations once per database, and `ask_question` now says why
  it refused instead of returning `needs_input: false` for every reason at once.
  The *tell* is no longer left to the reader either: `open_session` now carries
  a `replaced_binary` notice on both planes when the file the process started
  from has since been replaced on disk, which is exactly the condition that
  produced this incident. It is a different question from `stale_build` and
  cannot be answered by it — a live process keeps reporting the build sha it was
  compiled with, so it can match HEAD perfectly while the code actually
  answering has been superseded — and the notice says to restart rather than to
  rebuild, because rebuilding was the first diagnosis here and it was wrong.
  **Still open:** whether identity should be pinned per session against the
  *token* rather than whatever the running process formats remains undecided,
  and detection is still not prevention — an agent is told to restart, and
  nothing restarts for it.
  **Closed 2026-08-30: a window reset is no longer invisible.** `claim_window`
  now carries `replaced`, naming the window the current one displaced, when it
  opened, the holes it had, and whether the owner changed. A fresh window opened
  because the owner id changed no longer reads as a first claim — which was the
  reading that made this incident so expensive, since work committed under the
  earlier window falls outside the current one and nothing said so. Derived from
  the log like the rest of `ClaimWindow`, never stored, and `is_continuous()` is
  deliberately unchanged: replacing a window is legitimate (release and
  re-claim, a recorded handover), so this reports the fact rather than adding a
  refusal that would reject work ADR-0048 permits.
  Writing the tests found a second defect in the same computation: a release is
  itself a window change, to an *unowned* state, so `alice → release → bob`
  produces two changes rather than one and the second displaces the gap rather
  than alice. Reporting that would have said a genuine handover replaced nobody
  — the exact failure this field exists to prevent, arriving by a different
  route. An unowned state now parks what it displaced until something claims it.
