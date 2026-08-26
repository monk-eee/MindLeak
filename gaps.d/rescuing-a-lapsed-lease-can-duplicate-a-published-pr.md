- **Rescuing a lapsed-lease task can duplicate a PR the original owner already
  published — OBSERVED 2026-08-13, GUARDED, still not enforced.**
  `task:ac8c36dc5da1`'s lease lapsed at 23:32Z while its owner was actively
  finishing up: they had already pushed a validated fix commit and opened PR
  #414 for it at 23:30:12Z, two minutes before the lapse. `gh pr list --search
  "hydrate offline"` run shortly after found nothing (search-index lag or
  query mismatch, not confirmed which), so a rescue looked clean: the task
  read as claimable, the branch's own worktree showed a clean committed tree
  with no open PR by that search, and cherry-picking the commit onto a fresh
  branch produced PR #418 — a byte-identical diff to #414, opened fourteen
  minutes after it.

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
  original, deleted its branch, removed its worktree.

  GUARDED: `scripts/worktree-owner.mjs --adopt-worktree` now runs `gh pr list
  --head <branch> --state all` itself and prints a named warning when the
  branch already has one, degrading visibly (never silently) when `gh` is
  unavailable or unauthenticated. Still NOT a hard gate — a closed/abandoned
  PR, or a stale/unauthenticated `gh` call, must not block a genuine rescue,
  so a rescuer who ignores the printed warning can still reproduce this exact
  incident. Remains open whether it should escalate further (e.g. refusing
  without an explicit override) once this advisory has been observed in use.

  NARROWED 2026-08-26 — the guard's coverage boundary is ADOPTION, and the
  rescuer is not always someone else. Same incident shape, reached by a path
  the guard cannot see: one agent resumed its OWN worktree, on its OWN branch,
  after a context compaction. Its lease had lapsed, and its own PR (#748) was
  already pushed with auto-merge armed. Nothing was adopted, so
  `--adopt-worktree` never ran and its `gh pr list` check never fired. The
  agent then ran `git log --all -S <symbol> -- <file>`, which returned nothing
  for a symbol that branch's own pushed commit adds; combined with `git status`
  showing the files as modified, that read as "this was never committed", and
  a duplicate changelog fragment was committed onto the armed PR. Caught before
  the push and reverted with `git reset --mixed HEAD~1`, so the cost was one
  commit rather than a restarted check run on an armed branch.

  So the duplicate does not require two agents, only two contexts — and a
  compaction reliably supplies the second. The three signals that made the
  first incident visible (a different owner, an unfamiliar worktree, an
  adoption step to hang a check on) are all absent here.

  OPEN, and the residual is detection rather than enforcement. The two checks
  that do settle it are cheap and need no adoption step: `gh pr list --head
  <branch> --state all`, and the network-free `git log --oneline
  origin/main..origin/<branch>`. Neither is wired into any gate — `scoped-
  commit.mjs` is deliberately git-only and network-free, so adding a `gh` call
  to every commit is not obviously the right trade, and "this branch has an
  open PR" is far too common to warn on unconditionally. A narrower trigger
  worth considering is an ARMED pull request specifically, since "armed means
  finished" (ADR-0045) makes a new commit there rare and genuinely worth
  questioning. Recording the boundary rather than guessing the mechanism.
  What is settled: `git status` plus a pickaxe miss is not evidence that work
  is unpublished — those answer whether the tree differs from HEAD, not
  whether the branch already shipped it.
