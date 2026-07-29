- **A wrapped `Status:` line silently lost its reference — FIXED.** — ADR-0032
  writes `- Status: Superseded by` and puts `[ADR-0038](...)` on the next line.
  `scripts/adr-files.mjs` read to the end of the line, so the file parsed as a
  bare `Superseded by`. That value went into the ADR index table, into
  `make design-audit`, and into a question put to a human as "nobody can tell
  what replaced it" — when the answer was one line further down, and the
  superseding commit (`8c6f0a1`, authored by the maintainer) names ADR-0038 in
  its `DECISION:` line. Impact: a tool's blind spot was escalated as a knowledge
  gap. Fixed by reading indented continuation lines, with a regression test in
  `editors/vscode/scripts/adr-files.test.mjs`. Worth remembering as a class:
  **check whether the tool can see it before concluding the information is not
  there.**
