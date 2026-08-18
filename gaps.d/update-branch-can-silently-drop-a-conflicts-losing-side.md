- **`gh pr update-branch` can silently drop a real conflict's losing side instead
  of failing — MEASURED 2026-08-18, left OPEN, not fully diagnosed.** The
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

  **What this changes for now:** do not trust "the PR merged with green checks"
  as proof a diff landed intact when the branch went through an automated
  `update-branch` step against a `main` that moved concurrently. Diff the
  actual files on `origin/main` against what the PR's own commits show,
  especially for any file two recently-merged PRs both touched.
