- **Nothing declares `merge=union` any more, so phantom conflicts stop.**
  ADR-0056 took the driver off `CHANGELOG.md` but kept it on
  `docs/adr/README.md`, on the grounds that the index is generated and
  hook-guarded so union was "a convenience, not the mechanism". That was right
  about correctness and wrong about cost. **The convenience exists only in a
  checkout; the phantom conflict is what everyone else sees.** Within hours a
  pull request whose *only* both-sides file was the ADR index reported
  `CONFLICTING`, merged clean locally, and could not be repaired with
  `gh pr update-branch` — because that is a server-side merge too. Its
  auto-merge sat armed and silently stopped working, which is exactly the
  failure "armed means finished" (ADR-0045) exists to rule out. Six hand
  reconciliations in one day, and one duplicated `## [0.1.3]` heading that union
  merged happily into a release changelog.
  There is also a defect specific to a *generated* file: union merging a
  generated table can produce a duplicated or misordered one, and the hook then
  regenerates it anyway — so the wrong resolution was being computed and thrown
  away. A generated file is regenerated, never merged. The test asserts the
  declared set is now empty rather than being deleted, because the failure it
  guards is invisible locally: any file added back here merges cleanly in every
  checkout and blocks its pull request on GitHub with no way to clear it.
