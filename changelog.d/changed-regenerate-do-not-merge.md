- **`DEVELOPERS.md` now says what to do when a generated file conflicts.**
  `docs/adr/README.md` is derived from the ADR files, so every branch that adds
  an ADR appends a row at the same place and conflicts on every merge from
  `main` — three separate branches hit it in one session with nothing in the
  docs to say the resolution is `make adr-index`, not a hand-merge. Keeping
  "both sides" of a generated table produces a duplicated index that the
  pre-commit check then rejects, so hand-resolving it is discarded work.
