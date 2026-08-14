- **The broken-link guard now checks ADRs too, so a citation into `gaps.d/`
  cannot dangle unnoticed.** `docs/adr/` was excluded while a backlog of
  cross-references pointed at renamed or never-written decisions; that backlog
  is closed, and the exclusion had outlived it. `adr-link-check.mjs` matches
  only sibling `NNNN-slug.md` targets, so an ADR citing a gap fragment kept
  resolving after the fragment was deleted — which is exactly what closing a gap
  is supposed to do.
