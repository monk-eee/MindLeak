- **A failed git command in the merge-verification fixture names its cause,
  instead of trailing off after a colon.** The fixture's `git()` helper asserted
  with git's stderr alone, so a non-zero exit that wrote nothing produced
  `git ["commit", "-m", "base"] failed:` and no more.
  Measured 2026-08-14: six `merge_tests` failed that way during a
  `cargo test --all` run while several builds were running concurrently, and
  only one of the six happened to reveal the real cause,
  `fatal: Out of memory, malloc failed`. All eleven passed under
  `--test-threads=1`.
  The message now always carries git's exit status — the one fact that is
  always available — reports stdout as well as stderr, and says explicitly when
  git wrote nothing at all, naming memory, disk or a killed process as the
  likely cause and suggesting `--test-threads=1`. A termination by signal is
  reported as such rather than rendering as an absent exit code.
  This matters because the tests that fail this way are named
  `merge_evidence_*` and are unrelated to whatever the developer was editing, so
  a silent failure reads as "my change broke something I do not understand" when
  the cause is the machine.
