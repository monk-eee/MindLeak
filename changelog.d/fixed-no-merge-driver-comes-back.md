- **A merge driver can no longer return to `.gitattributes`.** A driver runs
  only in a local checkout. GitHub's "Update branch", the merge queue, and the
  merge itself all run server-side with no driver configured — so an attribute
  promising "keep both sides" silently does not apply, and the branch reports a
  conflict in the very file the driver was supposed to keep conflict-free. That
  is worse than having no driver: an ordinary conflict is expected and resolved,
  while this one contradicts the repository's own configuration, in a file
  everybody edits, and invites you to distrust the merge rather than the
  attribute. `merge=union` on `CHANGELOG.md` cost an evening of phantom
  conflicts before the cause was found, and deleting it was only half the fix —
  nothing stopped the next reader from acting on the same "keep both" wish.
  A pre-commit guard now refuses any `merge=` declaration, naming the file and
  line, and points at per-change fragment files instead: two changes never write
  the same path, so they never collide. Comments are exempt, so `.gitattributes`
  can still record why the rule exists.
