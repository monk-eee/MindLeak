- **A restored file with an older timestamp is silently not rebuilt — OBSERVED,
  FIXED BY HABIT.** Cargo decides what to recompile by mtime, and PowerShell's
  `Copy-Item` gives the destination the *source's* timestamp. Backing a file up
  before a red/green probe and copying it back therefore restores the content
  with an mtime older than the compiled artifact, so cargo keeps the previous
  object and the test runs against the code you thought you had just restored.
  Impact: cost most of a session on ADR-0060. The same fix, restored two
  different ways, gave `aligned` once and `needs_human` twice, which read as a
  flaky test and is not one — and cargo still prints `Compiling <crate>` for the
  *other* files you touched, so the log looks like a real rebuild. Use
  `git checkout -- <path>` and `git stash pop` (both write fresh timestamps) for
  probes, or touch the file after any `Copy-Item` restore.
