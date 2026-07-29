### Changed

- **Known gaps are fragments, so recording one never conflicts.** The Known gaps
  section of `DEVELOPERS.md` was a single shared append-only list of 81 entries,
  so every branch that recorded a gap edited the same lines. Almost every pull
  request collided there, and each conflict expressed no disagreement whatever —
  two agents adding two unrelated observations to the same paragraph. It was
  hand-resolved four times in one session.

  ADR-0056 already solved this shape for `CHANGELOG.md`: a fragment is a new
  file per item, and two branches never write the same path. Gaps now live in
  `gaps.d/`, one file per gap, with `node scripts/gaps.mjs --list` to read them
  and `--check` in the pre-commit hook to refuse a malformed one.

  One deliberate difference from `changelog.d/`: a changelog fragment folds into
  the file at release and is deleted, but a gap has no release event — it is
  open until it is fixed. Folding would put the shared list, and the conflict,
  straight back, so the fragments are the source of truth permanently and
  `DEVELOPERS.md` points at them instead of holding a generated copy. Closing a
  gap deletes its fragment in the commit that fixes it, so the fix and the
  retraction are one reviewable change.

  `--check` fails on an empty `gaps.d/` rather than reporting success. An empty
  Known Gaps section is almost always a lie, and a validator that passed over a
  directory which had quietly lost every gap ever recorded would give the one
  answer it must never give.
