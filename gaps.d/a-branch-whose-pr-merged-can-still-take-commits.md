- **A branch whose pull request already merged has no tooling signal that
  further commits on it will strand — OBSERVED 2026-08-19, GUARDED.**
  `canonical-push.mjs` checks for a live Lodestar claim, a clean tree, and
  divergence from the remote, but never asked GitHub whether the current
  branch already has a *merged* pull request. Neither does
  `worktree-owner.mjs --adopt-worktree`, nor any pre-commit hook. A branch
  is not terminal the way a task or a PR is, so nothing refused a commit
  onto one whose PR closed weeks ago.

  Observed twice in one session, on the same underlying branch:

  1. A prior session implemented ADR-0103 (`clients/node/mindleak-client`,
     Lodestar `task:7d5fe15d6eee`, completed `done`/`aligned`) and pushed it
     as two commits (`ac7ed12`, `53b6882`) directly onto
     `docs/phase1-audit-gap-adrs` -- but that branch's own PR (#523, docs
     only) had already merged. The commits sat on the remote branch with no
     open PR, invisible to `gh pr list --state open`, `next_task`, and
     `existing_work`'s path-matching. They surfaced only by chance: a
     completed-but-code-absent Lodestar task's `prior_work` cross-reference
     led back to the branch, and `git branch -r --contains <sha>` confirmed
     the commits existed only there.
  2. Minutes after fixing (1), while implementing ADR-0104 in the very
     next task, I continued working in worktree
     `MindLeak-adr0103-docs` (branch `docs/adr-0103-docs-update`) --
     whose own PR (#529) had, by then, also already merged. New files
     (`scripts/reference-consumer.mjs` and its test) were written into that
     worktree before I ran `git log` and noticed the merge commit for #529
     sitting at `HEAD`'s immediate ancestor. Caught before committing;
     recovered by moving the uncommitted files to a fresh worktree/branch.

  Impact: a `done`/`aligned` Lodestar task is evidence that conformance ran
  against *some* commit, never proof that commit reached `main`. Continued
  work on the same branch/worktree after its PR merges has no path back to
  review except a brand-new PR (`gh pr create` from the same branch, same
  head, new PR object) -- and nothing prompts for that until an agent
  either notices by hand or accidentally rediscovers the stranded work
  later, at unbounded cost. The closest existing signal,
  `worktree-reclaim.mjs`'s "landed" classification, runs only when someone
  chooses to reclaim; it is never consulted before a commit.

  Left OPEN: no fix attempted this run beyond the two manual recoveries
  above. A structural fix would add a check --  in `canonical-push.mjs`,
  a pre-commit hook, or both -- that queries whether the current branch's
  most recent pull request (`gh pr list --head <branch> --state all --limit 1`)
  is already `MERGED`, and warns (or refuses) before accepting a new commit
  on it, the same way `abandon_task` already refuses to retire a task whose
  recorded branch might still carry open work.

  GUARDED: `canonical-push.mjs` now checks the current branch's most recent
  pull request (`scripts/merged-branch-warning.mjs`, ordered by PR number so
  API response order can't hide a later one) and warns, naming the merged PR
  and the `gh pr create --head <branch>` remedy, right before pushing.
  A warning, not a refusal: these new commits are not themselves harmful,
  and opening a fresh PR from the same branch afterward is the documented,
  working way to publish them -- the harm this guards is nobody remembering
  to take that step. Still relies on the rescuer/committer reading the
  warning; does not (and by design cannot) stop a commit from landing in the
  first place, since the earlier two incidents were both caught only by
  accident well before any push.
