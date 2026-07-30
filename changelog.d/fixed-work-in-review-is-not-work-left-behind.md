- **The merged-branch audit called work in review "lost", and turned `main`
  red.** A commit pushed onto a branch after its pull request merged was
  reported as never having reached `main`, with the instruction to open a
  follow-up pull request — when the commit was already sitting in an open one.
  That instruction cannot be carried out: the follow-up exists, and nothing
  anybody does will satisfy the audit until that pull request merges. It is the
  same defect this audit was rewritten to remove, in a new costume. An audit
  with no green move available gets switched off, and switching this one off
  takes the check that catches genuinely stranded work with it. Measured on the
  live repository: commit `ffab86ea`, held against PR #213's merged branch, is
  an ancestor of the open PR #231, and three consecutive `main` builds failed on
  it. The audit now asks whether any open pull request still carries the commit.
  If one does, the commit is reported as in review, named against that pull
  request, and does not fail the build. If none does, it fails exactly as
  before, so nothing is weakened — proven by a fixture where an unrelated open
  branch does not rescue a stranded commit. Failing to list open pull requests
  degrades to the old, noisier behaviour rather than to a clean bill of health,
  because an unreachable `gh` must not be able to silence the audit. Pushing to
  an already-merged branch is still reported and still a mistake: that pull
  request will never reopen, so the commit survives only for as long as
  something else happens to carry it.
