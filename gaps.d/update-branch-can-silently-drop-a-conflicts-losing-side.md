- **`gh pr update-branch` can silently drop a real conflict's losing side instead
  of failing — MEASURED 2026-08-18, GUARDED 2026-08-19, OPEN: the root cause in
  GitHub's own merge algorithm is still undiagnosed.** The
  delivery queue (`scripts/delivery-queue.mjs`) brings an armed PR's branch up
  to date by calling `gh pr update-branch <number>`, and its own comment says
  "`update-branch` fails when GitHub cannot merge main in cleanly" — treating a
  clean return as proof the merge is a correct, content-preserving one.

  That assumption failed for PR #507 (`feat/federated-overlap-source`). Both my
  branch and `main` (via the already-merged PR #505, RecoverClaim) had each
  independently added a new RPC method to the same three files
  (`node_sync.proto`, `claim_service.rs`, `claim_store.rs`) at the same
  insertion point — a real, textual conflict. Reproducing the identical merge
  locally (`git fetch` + `git merge origin/<branch>`) genuinely conflicted, as
  expected. But the commit `gh pr update-branch` had already produced on the
  remote (`ad9f7da`, "Merge branch 'main' into feat/federated-overlap-source")
  reported no error and merged cleanly — yet its tree kept only `main`'s side
  (RecoverClaim) and silently dropped the branch's side (`ListActiveClaims`),
  even though `ad9f7da`'s own first parent (my branch tip) plainly had it.
  PR #507 then auto-merged that lossy commit into `main` with all checks green,
  so the completed, tested work (`task:3780fd036ae6`) never actually reached
  `main` — confirmed by `git show origin/main:crates/ackplane-server/src/
  claim_store.rs` having no `list_active` after the "merge".

  **Impact:** any two concurrent PRs adding non-overlapping-in-intent but
  textually-adjacent code to the same file can have one side vanish without
  either agent, reviewer, or CI ever seeing a conflict marker or a failed
  check — the losing PR's own commit still shows the code; only the post-merge
  tree on `main` is short. Recovered here as `task:c962d19d0cf1` / PR #510, a
  fresh commit re-applying the dropped hunk cleanly against current `main`.

  **Not fully diagnosed:** whether this is a genuine bug in GitHub's
  `update-branch` merge algorithm, a timing artifact (the branch update
  happening before/after PR #505 landed in an order that let a 3-way merge
  degrade to a fast-forward-shaped resolution), or something specific to this
  push sequence, is unconfirmed. Left open rather than asserting a root cause
  the evidence does not actually establish.

  **Guarded 2026-08-19:** `scripts/delivery-queue.mjs`'s own `update-branch`
  call now computes the expected post-merge tree locally
  (`git merge-tree --write-tree origin/main origin/<branch>`) immediately
  before calling it, and compares the branch's actual tree against that
  expectation once it returns. A mismatch is reported loudly
  (`updated #N BUT ITS TREE DOES NOT MATCH THE EXPECTED MERGE`) instead of the
  plain "updated #N" line, so this exact call site can no longer repeat PR
  #507's failure unnoticed (`updateBranchMismatch`, unit- and
  sabotage-verified in `delivery-queue.test.mjs`). This catches the symptom at
  its one known call site, not the cause: the underlying behaviour in
  GitHub's `update-branch` remains undiagnosed.

  **Guarded 2026-08-20 for the manual call site too:** `scripts/update-branch-
  safely.mjs <pr-number>` is the same check, reusing `updateBranchMismatch`/
  `expectedMergeTree`/`actualMergeTree` verbatim, for anyone reconciling a
  branch by hand outside the queue. What remains open is the underlying
  GitHub behaviour itself, and a merge produced any other way (for example
  the web UI merge button), which no local guard can see.

  **CLOSED 2026-08-29 for detection, at every call site including the web UI
  merge button.** Both guards above are prospective and site-local: they check
  the merge they are about to make. `scripts/merge-audit.mjs` now makes the same
  comparison retrospectively, against merge commits already on `main`, in the CI
  job that runs on every push there — so it does not matter which button made
  the merge, or whether any local script was involved at all. For each merged
  pull request it recomputes `git merge-tree --write-tree <merge>^1 <merge>^2`
  and compares that against the merge commit's own tree; a mismatch fails the
  build and names both trees (`mergeIsFaithful`). Two properties make this the
  right shape rather than a third copy of the same idea:

  - **It survives branch deletion.** A merge commit keeps both parents, so this
    works after GitHub deletes the branch — unlike the `git cherry` half of the
    same audit, which was measured on CI run 99036902566 silently skipping 10 of
    the 30 pull requests it was asked about while reporting only the 20 it
    managed. That summary line now states the whole population and names the
    branches it could not fully check.
  - **It is quiet.** Verified across `main`'s last 120 merge commits: 115
    compared and matched, 5 honestly uncomparable (no second parent, or a real
    conflict a human resolved, where the tree is *supposed* to differ),
    0 mismatches. An audit that cries wolf gets switched off.

  A first attempt at this recovered the deleted branch's tip from `<merge>^2`
  and ran `git cherry` against it. That was discarded before it shipped, and the
  test is why: the recovered tip is an ancestor of the base *by construction*, so
  the comparison could only ever answer "clean". It would have read as new
  coverage while checking nothing — worse than the gap it replaced.

  **Still open:** the root cause in GitHub's own merge algorithm, which remains
  undiagnosed. Detection is now retrospective and complete for merge-commit
  merges; prevention still depends on GitHub. A squash or rebase merge has no
  second parent and cannot be checked this way at all, which is one more reason
  those buttons stay disabled on this repository.

  **What this changes for now:** "the PR merged with green checks" is now
  actually verified after the fact, so a lossy merge fails `main`'s own build
  rather than sitting undetected. Until that build has run, still diff the
  actual files on `origin/main` against what the PR's own commits show,
  especially for any file two recently-merged PRs both touched.
