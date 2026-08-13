- **Rescuing a lapsed-lease task can duplicate a PR the original owner already
  published — OBSERVED 2026-08-13, left OPEN.** `task:ac8c36dc5da1`'s lease
  lapsed at 23:32Z while its owner was actively finishing up: they had already
  pushed a validated fix commit and opened PR #414 for it at 23:30:12Z, two
  minutes before the lapse. `gh pr list --search "hydrate offline"` run shortly
  after found nothing (search-index lag or query mismatch, not confirmed
  which), so a rescue looked clean: the task read as claimable, the branch's
  own worktree showed a clean committed tree with no open PR by that search,
  and cherry-picking the commit onto a fresh branch produced PR #418 — a
  byte-identical diff to #414, opened fourteen minutes after it.

  Impact: `view=next`/`view=overlap` answer from the *task* ledger and the
  *branch's* git state, both of which were genuinely clean — neither one
  reads GitHub. A lapsed lease with real, already-published progress is
  indistinguishable from a lapsed lease with nothing published unless the
  rescuer also checks `gh pr list --head <branch> --state all` (exact branch
  name, not a text search) before publishing a parallel fix. Caught here only
  because both PRs carried an identical title and diff, making the duplicate
  obvious at a glance; a rescue that changed the fix even slightly would have
  produced a genuine merge conflict instead of an easy close.

  Recovery was cheap: closed the later PR (#418) with a comment naming the
  original, deleted its branch, removed its worktree. Left for later: whether
  the rescue workflow should mandate `gh pr list --head <branch> --state all`
  (not a text search) as a required pre-flight before cherry-picking a
  lapsed-lease branch's commits onto a fresh one.
