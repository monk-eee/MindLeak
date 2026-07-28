- **Pushing a release tag works again, which the documented release step
  required and the guard had made impossible.** `canonical-push`'s pre-push hook
  judged every push as a branch publication, so `git push origin vX.Y.Z` —
  step 3 of the release procedure in `DEVELOPERS.md` — was rejected three ways
  at once: `symbolic-ref` fails when tagging from a detached HEAD, tagging while
  on `main` tripped the protected-branch refusal, and the publisher flag is only
  set when the script pushes a branch. v0.1.3 could only be cut by setting an
  undocumented environment variable, which is folklore rather than a procedure.
  A tag is now judged on its own terms and against a single invariant: it must
  name a commit already on `origin/main`, because tagging is how a release is
  chosen and an unmerged commit would ship code that never passed review. Branch
  pushes are unaffected and still require a live claim.
