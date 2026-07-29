- **Accepting a design wrote `Accepted` into whichever worktree resolved the ADR
  path first — FIXED.** — Observed Jul 2026 while ADR-0044 was still an unmerged
  proposal on its own branch. Two ADR files in this checkout changed from
  `Status: Proposed` to `Status: Accepted` in the working tree, uncommitted,
  while the agent that owned the checkout was mid-pull-request and had accepted
  nothing. One of the two, ADR-0043, belonged to a different branch entirely —
  which is the tell: the write landed in a checkout that had no relationship to
  the decision being accepted. The write itself is human-triggered (`accept()` /
  `reject()` prompt for a name before calling `alignAdrStatus`), so this was
  never a status flipping itself; the defect was *where the write went*.
  `resolveAdrUri` ([`editors/vscode/src/designBoardController.ts`](editors/vscode/src/designBoardController.ts))
  walked `workspace.workspaceFolders` and wrote to the first folder containing
  the path. Under ADR-0038 several worktrees on different branches share one
  `spec.db` and are commonly open together, so "first folder containing this
  path" was close to arbitrary. — Medium impact: no data loss, but an ADR's
  declared status is evidence of a human decision, and this could plant that
  evidence on a branch nobody decided anything about — or, as here, drop an
  uncommitted edit into another agent's tree mid-commit, which is also the
  pre-commit stash race described below. Caught only because the owning agent
  read `git status` before pushing rather than trusting it. — Fixed Jul 2026:
  `chooseAdrTarget` never picks. One matching checkout writes as before; several
  ask the reviewer which one, and cancelling aborts without writing; none keeps
  the existing clear error. Deliberately *not* fixed by binding a design record
  to a worktree — that would put a machine-specific path in a database ADR-0038
  shares across checkouts, and it answers a question the reviewer is better
  placed to answer while already standing in the prompt.
